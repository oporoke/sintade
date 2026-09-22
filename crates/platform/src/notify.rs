use sqlx::PgPool;
use sqlx::postgres::PgListener;

use crate::error::PlatformError;

/// Opens a `LISTEN` connection on `channel`. Callers own the returned listener and poll it
/// with `.recv()`/`.try_recv()`.
pub async fn listen(pool: &PgPool, channel: &str) -> Result<PgListener, PlatformError> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(channel).await?;
    Ok(listener)
}
