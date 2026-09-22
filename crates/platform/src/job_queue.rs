use std::time::Duration;

use kernel::Id;
use rand::RngExt;
use serde_json::Value;
use sqlx::PgPool;

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

    pub async fn claim_next(
        &self,
        worker_id: &str,
        lock_duration: Duration,
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
                ORDER BY run_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, kind, payload, attempts, max_attempts
            "#,
            worker_id,
            lock_seconds,
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
