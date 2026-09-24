mod app;
mod error;
mod routes;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = platform::Config::load()?;
    platform::init_telemetry(&config.rust_log);

    let pool = platform::connect(&config.database_url).await?;
    let queue = platform::JobQueue::new(pool.clone());
    let identity = Arc::new(identity::IdentityService::new(
        pool.clone(),
        queue,
        Arc::new(platform::SystemClock),
        config.public_base_url.clone(),
    ));
    let router = app::build_router(pool, identity, &config.public_base_url);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("api listening on 0.0.0.0:8080");
    axum::serve(listener, router).await?;

    Ok(())
}
