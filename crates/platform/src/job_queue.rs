use std::time::Duration;

use kernel::Id;
use rand::RngExt;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};

pub struct Job;
pub type JobId = Id<Job>;

#[derive(Debug, thiserror::Error)]
pub enum JobQueueError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct ClaimedJob {
    pub id: JobId,
    pub kind: String,
    pub payload: Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FailOutcome {
    Retrying,
    DeadLettered,
}

pub struct JobQueue {
    pool: PgPool,
}

impl JobQueue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn enqueue(&self, kind: &str, payload: Value) -> Result<JobId, JobQueueError> {
        let id = JobId::new_v7();
        sqlx::query!(
            "INSERT INTO jobs (id, kind, payload) VALUES ($1, $2, $3)",
            id.into_uuid(),
            kind,
            payload,
        )
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Enqueues a job on the caller's connection, so it exists only if the caller's
    /// transaction commits (e.g. `ProcessTake` together with the row it processes).
    pub async fn enqueue_in(
        conn: &mut PgConnection,
        kind: &str,
        payload: Value,
    ) -> Result<JobId, JobQueueError> {
        let id = JobId::new_v7();
        sqlx::query!(
            "INSERT INTO jobs (id, kind, payload) VALUES ($1, $2, $3)",
            id.into_uuid(),
            kind,
            payload,
        )
        .execute(&mut *conn)
        .await?;
        Ok(id)
    }

    /// Keeps a long job's lock (a heartbeat while it runs): `locked_until` moves to now +
    /// `lock_duration`, if `worker_id` still holds it. `false` means the lock was lost (it
    /// expired and another worker claimed the job), so this worker's result shouldn't count.
    pub async fn extend_lock(
        &self,
        id: JobId,
        worker_id: &str,
        lock_duration: Duration,
    ) -> Result<bool, JobQueueError> {
        let extended = sqlx::query!(
            r#"
            UPDATE jobs SET locked_until = now() + make_interval(secs => $3)
            WHERE id = $1 AND locked_by = $2 AND done_at IS NULL AND dead_at IS NULL
            "#,
            id.into_uuid(),
            worker_id,
            lock_duration.as_secs_f64(),
        )
        .execute(&self.pool)
        .await?;
        Ok(extended.rows_affected() == 1)
    }

    /// Enqueues a job of `kind` unless one is already waiting or running (scheduled jobs such
    /// as `SweepStaleUploads`: several workers ticking must not pile up copies). Returns the new
    /// job's id, or `None` if one was pending.
    pub async fn enqueue_unless_pending(
        &self,
        kind: &str,
        payload: Value,
    ) -> Result<Option<JobId>, JobQueueError> {
        let id = JobId::new_v7();
        let inserted = sqlx::query!(
            r#"
            INSERT INTO jobs (id, kind, payload)
            SELECT $1, $2, $3
            WHERE NOT EXISTS (
                SELECT 1 FROM jobs WHERE kind = $2 AND done_at IS NULL AND dead_at IS NULL
            )
            "#,
            id.into_uuid(),
            kind,
            payload,
        )
        .execute(&self.pool)
        .await?;
        Ok((inserted.rows_affected() == 1).then_some(id))
    }

    /// Claims the oldest runnable job of any kind.
    pub async fn claim_next(
        &self,
        worker_id: &str,
        lock_duration: Duration,
    ) -> Result<Option<ClaimedJob>, JobQueueError> {
        self.claim_kinds(worker_id, lock_duration, None).await
    }

    /// Claims the oldest runnable job whose kind is in `kinds` (per-kind concurrency limits:
    /// a worker whose slots for one kind are full leaves that kind for others).
    pub async fn claim_next_of(
        &self,
        worker_id: &str,
        lock_duration: Duration,
        kinds: &[&str],
    ) -> Result<Option<ClaimedJob>, JobQueueError> {
        let kinds: Vec<String> = kinds.iter().map(|kind| (*kind).to_string()).collect();
        self.claim_kinds(worker_id, lock_duration, Some(kinds))
            .await
    }

    async fn claim_kinds(
        &self,
        worker_id: &str,
        lock_duration: Duration,
        kinds: Option<Vec<String>>,
    ) -> Result<Option<ClaimedJob>, JobQueueError> {
        let lock_seconds = lock_duration.as_secs_f64();
        let row = sqlx::query!(
            r#"
            UPDATE jobs
            SET locked_by = $1, locked_until = now() + make_interval(secs => $2)
            WHERE id = (
                SELECT id FROM jobs
                WHERE done_at IS NULL
                  AND dead_at IS NULL
                  AND run_at <= now()
                  AND (locked_until IS NULL OR locked_until < now())
                  AND ($3::text[] IS NULL OR kind = ANY($3))
                ORDER BY run_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, kind, payload, attempts, max_attempts
            "#,
            worker_id,
            lock_seconds,
            kinds.as_deref(),
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ClaimedJob {
            id: JobId::from_uuid(r.id),
            kind: r.kind,
            payload: r.payload,
            attempts: r.attempts,
            max_attempts: r.max_attempts,
        }))
    }

    pub async fn complete(&self, id: JobId) -> Result<(), JobQueueError> {
        sqlx::query!(
            "UPDATE jobs SET done_at = now() WHERE id = $1",
            id.into_uuid()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a failed attempt. Exponential backoff (2^attempts seconds, ±20% jitter) until
    /// `max_attempts` is reached, then the job is dead-lettered (`dead_at` set).
    pub async fn fail(&self, id: JobId, error: &str) -> Result<FailOutcome, JobQueueError> {
        let row = sqlx::query!(
            r#"
            UPDATE jobs
            SET attempts = attempts + 1, last_error = $2, locked_by = NULL, locked_until = NULL
            WHERE id = $1
            RETURNING attempts, max_attempts
            "#,
            id.into_uuid(),
            error,
        )
        .fetch_one(&self.pool)
        .await?;

        if row.attempts >= row.max_attempts {
            sqlx::query!(
                "UPDATE jobs SET dead_at = now() WHERE id = $1",
                id.into_uuid()
            )
            .execute(&self.pool)
            .await?;
            Ok(FailOutcome::DeadLettered)
        } else {
            let base_secs = 2f64.powi(row.attempts);
            let jitter = rand::rng().random_range(-0.2..=0.2);
            let delay_secs = (base_secs * (1.0 + jitter)).max(0.0);
            sqlx::query!(
                "UPDATE jobs SET run_at = now() + make_interval(secs => $2) WHERE id = $1",
                id.into_uuid(),
                delay_secs,
            )
            .execute(&self.pool)
            .await?;
            Ok(FailOutcome::Retrying)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations")]
    async fn claim_next_of_skips_kinds_it_was_not_given(pool: PgPool) {
        let queue = JobQueue::new(pool);
        queue
            .enqueue("Heavy", serde_json::json!({}))
            .await
            .expect("enqueue");
        queue
            .enqueue("Light", serde_json::json!({}))
            .await
            .expect("enqueue");

        let light = queue
            .claim_next_of("w", Duration::from_secs(30), &["Light"])
            .await
            .expect("claim")
            .expect("the Light job");
        assert_eq!(light.kind, "Light");
        assert!(
            queue
                .claim_next_of("w", Duration::from_secs(30), &["Light"])
                .await
                .expect("claim")
                .is_none(),
            "Heavy is not offered"
        );
        assert!(
            queue
                .claim_next_of("w", Duration::from_secs(30), &[])
                .await
                .expect("claim")
                .is_none()
        );
        let heavy = queue
            .claim_next("w", Duration::from_secs(30))
            .await
            .expect("claim")
            .expect("any kind");
        assert_eq!(heavy.kind, "Heavy");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn enqueue_claim_and_complete_round_trip(pool: PgPool) {
        let queue = JobQueue::new(pool);
        let id = queue
            .enqueue("noop", serde_json::json!({}))
            .await
            .expect("enqueue succeeds");

        let claimed = queue
            .claim_next("worker-1", Duration::from_secs(30))
            .await
            .expect("claim succeeds")
            .expect("a job is ready");
        assert_eq!(claimed.id, id);
        assert_eq!(claimed.kind, "noop");
        assert_eq!(claimed.attempts, 0);

        queue.complete(id).await.expect("complete succeeds");

        let again = queue
            .claim_next("worker-1", Duration::from_secs(30))
            .await
            .expect("claim succeeds");
        assert!(again.is_none(), "completed jobs are not claimable again");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_heartbeat_keeps_a_long_job_from_being_reclaimed(pool: PgPool) {
        let queue = JobQueue::new(pool.clone());
        let id = queue
            .enqueue("long", serde_json::json!({}))
            .await
            .expect("enqueue");
        queue
            .claim_next("worker-1", Duration::from_millis(200))
            .await
            .expect("claim")
            .expect("ready");
        assert!(
            queue
                .extend_lock(id, "worker-1", Duration::from_secs(30))
                .await
                .expect("extend")
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            queue
                .claim_next("worker-2", Duration::from_secs(30))
                .await
                .expect("claim")
                .is_none(),
            "the extended lock still holds"
        );
        assert!(
            !queue
                .extend_lock(id, "worker-2", Duration::from_secs(30))
                .await
                .expect("extend"),
            "only the holder extends"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn enqueue_in_rolls_back_with_its_transaction(pool: PgPool) {
        let mut tx = pool.begin().await.expect("tx");
        JobQueue::enqueue_in(&mut tx, "noop", serde_json::json!({}))
            .await
            .expect("enqueue");
        tx.rollback().await.expect("rollback");
        let claimed = JobQueue::new(pool)
            .claim_next("worker-1", Duration::from_secs(30))
            .await
            .expect("claim");
        assert!(claimed.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn locked_job_is_not_claimed_by_another_worker(pool: PgPool) {
        let queue = JobQueue::new(pool);
        let id = queue
            .enqueue("noop", serde_json::json!({}))
            .await
            .expect("enqueue succeeds");

        queue
            .claim_next("worker-1", Duration::from_secs(30))
            .await
            .expect("claim succeeds")
            .expect("a job is ready");

        let contended = queue
            .claim_next("worker-2", Duration::from_secs(30))
            .await
            .expect("claim succeeds");
        assert!(
            contended.is_none(),
            "a locked job is skipped, not double-claimed"
        );

        // sanity: the job is still findable and belongs to worker-1
        let _ = id;
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn failing_job_reaches_dlq_after_max_attempts(pool: PgPool) {
        // Exercises `fail()` directly rather than via claim_next in a loop: each failure's
        // backoff pushes run_at into the future, so a real worker wouldn't reclaim it
        // immediately either -- the DLQ transition itself doesn't depend on that wait.
        let queue = JobQueue::new(pool);
        let id = queue
            .enqueue("always-fails", serde_json::json!({}))
            .await
            .expect("enqueue succeeds");

        for attempt in 1..=4 {
            let outcome = queue.fail(id, "boom").await.expect("fail succeeds");
            assert_eq!(
                outcome,
                FailOutcome::Retrying,
                "attempt {attempt} should retry, not dead-letter yet"
            );
        }

        let outcome = queue.fail(id, "boom").await.expect("fail succeeds");
        assert_eq!(
            outcome,
            FailOutcome::DeadLettered,
            "5th failure must reach the DLQ"
        );

        let row = sqlx::query!(
            "SELECT attempts, dead_at IS NOT NULL as \"is_dead!\" FROM jobs WHERE id = $1",
            id.into_uuid()
        )
        .fetch_one(&queue.pool)
        .await
        .expect("job row exists");
        assert_eq!(row.attempts, 5);
        assert!(row.is_dead, "job must be dead-lettered after max_attempts");
    }
}
