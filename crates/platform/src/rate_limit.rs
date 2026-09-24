use std::time::Duration;

use sqlx::PgPool;
use time::OffsetDateTime;

use crate::error::PlatformError;

/// A Postgres-backed fixed-window rate limiter (§11: "Login 5/min/IP+email; signup 3/hour/IP").
/// One row per `(key, window)`; `check` atomically increments the counter for the caller's
/// current window and reports whether this call is still within the limit.
pub struct RateLimiter {
    pool: PgPool,
}

impl RateLimiter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns `true` if this call is within `limit` for its window (and has been counted
    /// toward it), `false` if the limit was already reached.
    pub async fn check(
        &self,
        key: &str,
        window: Duration,
        limit: u32,
        now: OffsetDateTime,
    ) -> Result<bool, PlatformError> {
        let window_secs = i64::try_from(window.as_secs()).unwrap_or(i64::MAX).max(1);
        let window_start =
            OffsetDateTime::from_unix_timestamp((now.unix_timestamp() / window_secs) * window_secs)
                .expect("window-aligned timestamp is always in range");

        let count = sqlx::query_scalar!(
            r#"
            INSERT INTO rate_limit_buckets (key, window_start, count)
            VALUES ($1, $2, 1)
            ON CONFLICT (key) DO UPDATE SET
                count = CASE
                    WHEN rate_limit_buckets.window_start = EXCLUDED.window_start
                    THEN rate_limit_buckets.count + 1
                    ELSE 1
                END,
                window_start = EXCLUDED.window_start
            RETURNING count
            "#,
            key,
            window_start,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(count <= i32::try_from(limit).unwrap_or(i32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations")]
    async fn allows_up_to_the_limit_then_rejects(pool: PgPool) {
        let limiter = RateLimiter::new(pool);
        let now = OffsetDateTime::now_utc();

        for attempt in 1..=5 {
            let allowed = limiter
                .check("test:key", Duration::from_secs(60), 5, now)
                .await
                .expect("check succeeds");
            assert!(allowed, "attempt {attempt} should be within the limit of 5");
        }

        let sixth = limiter
            .check("test:key", Duration::from_secs(60), 5, now)
            .await
            .expect("check succeeds");
        assert!(!sixth, "the 6th attempt in the window must be rejected");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn different_keys_have_independent_limits(pool: PgPool) {
        let limiter = RateLimiter::new(pool);
        let now = OffsetDateTime::now_utc();

        for _ in 0..5 {
            assert!(
                limiter
                    .check("key-a", Duration::from_secs(60), 5, now)
                    .await
                    .expect("check succeeds")
            );
        }
        assert!(
            !limiter
                .check("key-a", Duration::from_secs(60), 5, now)
                .await
                .expect("check succeeds")
        );

        // key-b is untouched by key-a's exhausted limit
        assert!(
            limiter
                .check("key-b", Duration::from_secs(60), 5, now)
                .await
                .expect("check succeeds")
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_new_window_resets_the_count(pool: PgPool) {
        let limiter = RateLimiter::new(pool);
        let now = OffsetDateTime::now_utc();

        for _ in 0..5 {
            assert!(
                limiter
                    .check("test:reset", Duration::from_secs(60), 5, now)
                    .await
                    .expect("check succeeds")
            );
        }
        assert!(
            !limiter
                .check("test:reset", Duration::from_secs(60), 5, now)
                .await
                .expect("check succeeds")
        );

        let next_window = now + time::Duration::seconds(61);
        assert!(
            limiter
                .check("test:reset", Duration::from_secs(60), 5, next_window)
                .await
                .expect("check succeeds"),
            "a new window starts a fresh count"
        );
    }
}
