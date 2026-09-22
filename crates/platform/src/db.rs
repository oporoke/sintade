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
