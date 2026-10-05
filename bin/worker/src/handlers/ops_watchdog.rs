use std::sync::Arc;
use std::time::Duration;

use platform::{Clock, JobQueue, RateLimiter};
use serde::Deserialize;
use sqlx::PgPool;

use crate::handler::{JobCtx, JobError, JobHandler};

/// A runnable job waiting longer than this means the worker is down or saturated.
pub const QUEUE_BACKLOG_SECS: i64 = 10 * 60;
/// A recording `processing` longer than this has lost its job somewhere.
pub const STUCK_PROCESSING_MINUTES: i64 = 90;
/// At most one email per alert kind per this long (Sentry still gets every occurrence).
const EMAIL_EVERY: Duration = Duration::from_secs(60 * 60);

/// What the database says about the health of the system right now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// How long the oldest runnable, unclaimed job has waited.
    pub oldest_waiting_secs: Option<i64>,
    /// Jobs that ran out of attempts in the last hour.
    pub dead_last_hour: i64,
    /// Recordings stuck in `processing`.
    pub stuck_processing: i64,
    /// `Some(true)`: WAL archiving is on and its latest attempt failed. `None`: archiving is off.
    pub wal_archiving_failing: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    QueueBacklog,
    DeadJobs,
    StuckProcessing,
    WalArchiving,
}

impl AlertKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::QueueBacklog => "queue-backlog",
            Self::DeadJobs => "dead-jobs",
            Self::StuckProcessing => "stuck-processing",
            Self::WalArchiving => "wal-archiving",
        }
    }

    /// The runbook that says what to do.
    pub fn runbook(self) -> &'static str {
        match self {
            Self::QueueBacklog | Self::DeadJobs | Self::StuckProcessing => {
                "docs/runbooks/worker-stuck.md"
            }
            Self::WalArchiving => "docs/runbooks/backup.md",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub kind: AlertKind,
    pub detail: String,
}

/// The alerts a snapshot raises. Pure, so every threshold is tested.
pub fn evaluate(snapshot: &Snapshot) -> Vec<Alert> {
    let mut alerts = Vec::new();
    if let Some(waited) = snapshot
        .oldest_waiting_secs
        .filter(|s| *s > QUEUE_BACKLOG_SECS)
    {
        alerts.push(Alert {
            kind: AlertKind::QueueBacklog,
            detail: format!("the oldest waiting job has waited {} minutes", waited / 60),
        });
    }
    if snapshot.dead_last_hour > 0 {
        alerts.push(Alert {
            kind: AlertKind::DeadJobs,
            detail: format!(
                "{} job(s) ran out of attempts in the last hour",
                snapshot.dead_last_hour
            ),
        });
    }
    if snapshot.stuck_processing > 0 {
        alerts.push(Alert {
            kind: AlertKind::StuckProcessing,
            detail: format!(
                "{} recording(s) have been processing for over {STUCK_PROCESSING_MINUTES} minutes",
                snapshot.stuck_processing
            ),
        });
    }
    if snapshot.wal_archiving_failing == Some(true) {
        alerts.push(Alert {
            kind: AlertKind::WalArchiving,
            detail:
                "Postgres' latest WAL archive attempt failed: point-in-time recovery is at risk"
                    .to_string(),
        });
    }
    alerts
}

pub async fn gather(pool: &PgPool) -> Result<Snapshot, sqlx::Error> {
    let oldest_waiting_secs = sqlx::query_scalar!(
        r#"
        SELECT EXTRACT(EPOCH FROM now() - min(run_at))::bigint
        FROM jobs
        WHERE done_at IS NULL AND dead_at IS NULL AND run_at <= now()
          AND (locked_until IS NULL OR locked_until < now())
        "#
    )
    .fetch_one(pool)
    .await?;
    let dead_last_hour = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM jobs WHERE dead_at > now() - interval '1 hour'"#
    )
    .fetch_one(pool)
    .await?;
    let stuck_processing = sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "n!" FROM recordings
        WHERE state = 'processing' AND trashed_at IS NULL
          AND updated_at < now() - make_interval(mins => $1::int)
        "#,
        STUCK_PROCESSING_MINUTES as i32,
    )
    .fetch_one(pool)
    .await?;
    let archive_mode = sqlx::query_scalar!(r#"SELECT current_setting('archive_mode') AS "mode!""#)
        .fetch_one(pool)
        .await?;
    let wal_archiving_failing = if archive_mode == "on" || archive_mode == "always" {
        Some(
            sqlx::query_scalar!(
                r#"
                SELECT coalesce(last_failed_time > coalesce(last_archived_time, '-infinity'), false)
                       AS "failing!"
                FROM pg_stat_archiver
                "#
            )
            .fetch_one(pool)
            .await?,
        )
    } else {
        None
    };
    Ok(Snapshot {
        oldest_waiting_secs,
        dead_last_hour,
        stuck_processing,
        wal_archiving_failing,
    })
}

/// `OpsWatchdog` (docs/design.md §19 incident response): every few minutes, looks at the queue, the
/// recordings and the backup archiver, and raises what it finds as `ERROR` log lines (which Sentry
/// reports) and, with `ALERT_EMAIL` set, as an email at most once an hour per kind.
#[derive(Clone)]
pub struct OpsWatchdogHandler {
    pool: PgPool,
    queue: JobQueue,
    limiter: Arc<RateLimiter>,
    clock: Arc<dyn Clock>,
    alert_email: Option<String>,
}

impl OpsWatchdogHandler {
    pub fn new(
        pool: PgPool,
        queue: JobQueue,
        limiter: Arc<RateLimiter>,
        clock: Arc<dyn Clock>,
        alert_email: Option<String>,
    ) -> Self {
        Self {
            pool,
            queue,
            limiter,
            clock,
            alert_email,
        }
    }

    /// Checks once; returns the alerts it raised.
    pub async fn check(&self) -> Result<Vec<Alert>, JobError> {
        let snapshot = gather(&self.pool)
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        let alerts = evaluate(&snapshot);
        for alert in &alerts {
            tracing::error!(
                alert = alert.kind.name(),
                runbook = alert.kind.runbook(),
                "ALERT {}: {}",
                alert.kind.name(),
                alert.detail
            );
            self.email(alert).await?;
        }
        Ok(alerts)
    }

    async fn email(&self, alert: &Alert) -> Result<(), JobError> {
        let Some(to) = &self.alert_email else {
            return Ok(());
        };
        let key = format!("alert-email:{}", alert.kind.name());
        let allowed = self
            .limiter
            .check(&key, EMAIL_EVERY, 1, self.clock.now())
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        if !allowed {
            return Ok(());
        }
        self.queue
            .enqueue(
                "SendEmail",
                serde_json::json!({
                    "to": to,
                    "subject": format!("[Sintade alert] {}", alert.kind.name()),
                    "body": format!("{}\n\nWhat to do: {}\n", alert.detail, alert.kind.runbook()),
                }),
            )
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        Ok(())
    }
}

/// The job carries nothing: each run looks at the system as it is.
#[derive(Deserialize)]
pub struct OpsWatchdogPayload {}

impl JobHandler for OpsWatchdogHandler {
    const KIND: &'static str = "OpsWatchdog";
    type Payload = OpsWatchdogPayload;

    async fn run(&self, ctx: &JobCtx, _payload: Self::Payload) -> Result<(), JobError> {
        let alerts = self.check().await?;
        tracing::info!(job_id = %ctx.job_id, alerts = alerts.len(), "ops_watchdog job ran");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(snapshot: &Snapshot) -> Vec<AlertKind> {
        evaluate(snapshot).into_iter().map(|a| a.kind).collect()
    }

    #[test]
    fn a_healthy_system_raises_nothing() {
        assert!(kinds(&Snapshot::default()).is_empty());
        let fine = Snapshot {
            oldest_waiting_secs: Some(QUEUE_BACKLOG_SECS),
            wal_archiving_failing: Some(false),
            ..Snapshot::default()
        };
        assert!(
            kinds(&fine).is_empty(),
            "exactly at the threshold is still fine"
        );
    }

    #[test]
    fn each_threshold_raises_its_own_alert() {
        assert_eq!(
            kinds(&Snapshot {
                oldest_waiting_secs: Some(QUEUE_BACKLOG_SECS + 1),
                ..Snapshot::default()
            }),
            [AlertKind::QueueBacklog]
        );
        assert_eq!(
            kinds(&Snapshot {
                dead_last_hour: 1,
                ..Snapshot::default()
            }),
            [AlertKind::DeadJobs]
        );
        assert_eq!(
            kinds(&Snapshot {
                stuck_processing: 2,
                ..Snapshot::default()
            }),
            [AlertKind::StuckProcessing]
        );
        assert_eq!(
            kinds(&Snapshot {
                wal_archiving_failing: Some(true),
                ..Snapshot::default()
            }),
            [AlertKind::WalArchiving]
        );
        // Archiving that is off is not an alert here (development).
        assert!(
            kinds(&Snapshot {
                wal_archiving_failing: None,
                ..Snapshot::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn alerts_say_what_to_do() {
        for alert in evaluate(&Snapshot {
            oldest_waiting_secs: Some(9999),
            dead_last_hour: 3,
            stuck_processing: 1,
            wal_archiving_failing: Some(true),
        }) {
            assert!(alert.kind.runbook().starts_with("docs/runbooks/"));
            assert!(!alert.detail.is_empty());
        }
    }

    async fn insert_job(pool: &PgPool, state: &str) {
        let sql = match state {
            "old-waiting" => {
                "INSERT INTO jobs (id, kind, payload, run_at) VALUES (gen_random_uuid(), 'x', '{}', now() - interval '20 minutes')"
            }
            "locked" => {
                "INSERT INTO jobs (id, kind, payload, run_at, locked_until) VALUES (gen_random_uuid(), 'x', '{}', now() - interval '20 minutes', now() + interval '1 minute')"
            }
            "dead" => {
                "INSERT INTO jobs (id, kind, payload, dead_at) VALUES (gen_random_uuid(), 'x', '{}', now() - interval '5 minutes')"
            }
            "long-dead" => {
                "INSERT INTO jobs (id, kind, payload, dead_at) VALUES (gen_random_uuid(), 'x', '{}', now() - interval '3 hours')"
            }
            _ => unreachable!(),
        };
        sqlx::query(sql).execute(pool).await.expect("job");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn gather_reads_the_queue_and_the_recordings(pool: PgPool) {
        assert_eq!(gather(&pool).await.expect("gather"), Snapshot::default());

        insert_job(&pool, "old-waiting").await;
        insert_job(&pool, "locked").await;
        insert_job(&pool, "dead").await;
        insert_job(&pool, "long-dead").await;
        let user = uuid::Uuid::now_v7();
        let workspace = uuid::Uuid::now_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, 'w@example.com', 'W')",
            user
        )
        .execute(&pool)
        .await
        .expect("user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name) VALUES ($1, 'W')",
            workspace
        )
        .execute(&pool)
        .await
        .expect("workspace");
        for (state, minutes) in [("processing", 180), ("processing", 5), ("ready", 180)] {
            sqlx::query!(
                "INSERT INTO recordings (id, workspace_id, owner_id, title, state, updated_at)
                 VALUES (gen_random_uuid(), $1, $2, 't', $3::text::recording_state,
                         now() - make_interval(mins => $4::int))",
                workspace,
                user,
                state,
                minutes,
            )
            .execute(&pool)
            .await
            .expect("recording");
        }

        let snapshot = gather(&pool).await.expect("gather");
        assert!(
            snapshot
                .oldest_waiting_secs
                .is_some_and(|s| (1100..1300).contains(&s)),
            "{snapshot:?}"
        );
        assert_eq!(snapshot.dead_last_hour, 1, "only the recent dead job");
        assert_eq!(
            snapshot.stuck_processing, 1,
            "only the old processing recording"
        );
        assert_eq!(
            snapshot.wal_archiving_failing, None,
            "archiving is off in the test database"
        );
        let kinds: Vec<_> = evaluate(&snapshot).into_iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            [
                AlertKind::QueueBacklog,
                AlertKind::DeadJobs,
                AlertKind::StuckProcessing
            ]
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn alerts_are_emailed_at_most_once_an_hour_per_kind(pool: PgPool) {
        insert_job(&pool, "dead").await;
        let handler = OpsWatchdogHandler::new(
            pool.clone(),
            JobQueue::new(pool.clone()),
            Arc::new(RateLimiter::new(pool.clone())),
            Arc::new(platform::SystemClock),
            Some("ops@example.com".to_string()),
        );
        assert_eq!(handler.check().await.expect("first").len(), 1);
        assert_eq!(
            handler.check().await.expect("second").len(),
            1,
            "still reported"
        );
        let emails = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM jobs WHERE kind = 'SendEmail' AND payload->>'to' = 'ops@example.com'"#
        )
        .fetch_one(&pool)
        .await
        .expect("emails");
        assert_eq!(emails, 1, "one email, not two");
        let body = sqlx::query_scalar!(
            r#"SELECT payload->>'subject' AS "s!" FROM jobs WHERE kind = 'SendEmail'"#
        )
        .fetch_one(&pool)
        .await
        .expect("subject");
        assert_eq!(body, "[Sintade alert] dead-jobs");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn without_an_alert_address_nothing_is_mailed(pool: PgPool) {
        insert_job(&pool, "dead").await;
        let handler = OpsWatchdogHandler::new(
            pool.clone(),
            JobQueue::new(pool.clone()),
            Arc::new(RateLimiter::new(pool.clone())),
            Arc::new(platform::SystemClock),
            None,
        );
        handler.check().await.expect("check");
        let emails =
            sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM jobs WHERE kind = 'SendEmail'"#)
                .fetch_one(&pool)
                .await
                .expect("emails");
        assert_eq!(emails, 0);
    }
}
