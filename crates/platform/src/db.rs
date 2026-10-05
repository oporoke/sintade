use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::error::PlatformError;

pub async fn connect(database_url: &str) -> Result<PgPool, PlatformError> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(PlatformError::from)
}

/// Applies every migration in `migrations/` that has not run yet (forward-only; docs/design.md
/// §19). Embedded in the binary, so a deployed image can migrate its own database
/// (`api migrate`) with no extra tooling. Safe to run repeatedly. Run it from **one** place at a
/// time (the deploy job): two runs at once can deadlock on the `CREATE INDEX CONCURRENTLY`
/// migrations, which wait for every other open transaction, including a second migrator that is
/// waiting for sqlx's advisory lock (seen when this was tried).
pub async fn migrate(pool: &PgPool) -> Result<(), PlatformError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|error| PlatformError::Migrate(error.to_string()))
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;

    #[sqlx::test(migrations = false)]
    async fn migrate_builds_the_schema_and_is_repeatable(pool: PgPool) {
        migrate(&pool).await.expect("first run");
        migrate(&pool).await.expect("second run does nothing");
        let tables = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM information_schema.tables
               WHERE table_schema = 'public' AND table_name IN ('recordings', 'share_links', 'jobs')"#
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(tables, 3);
        let applied =
            sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM _sqlx_migrations WHERE success"#)
                .fetch_one(&pool)
                .await
                .expect("applied");
        assert!(applied >= 14, "{applied}");
    }
}
