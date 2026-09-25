mod app;
mod csrf;
mod error;
mod routes;
mod security_headers;
mod session;
#[cfg(test)]
mod tenant_isolation;
mod workspace_context;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = platform::Config::load()?;
    platform::init_telemetry(&config.rust_log);

    let pool = platform::connect(&config.database_url).await?;
    let queue = platform::JobQueue::new(pool.clone());
    let clock: Arc<dyn platform::Clock> = Arc::new(platform::SystemClock);
    let session_secret = identity::decode_session_secret(&config.session_secret);
    let tenancy = Arc::new(tenancy::TenancyService::new(pool.clone()));
    let identity = Arc::new(identity::IdentityService::new(
        pool.clone(),
        queue,
        clock.clone(),
        config.public_base_url.clone(),
        session_secret,
        tenancy.clone(),
    ));
    let rate_limiter = Arc::new(platform::RateLimiter::new(pool.clone()));
    let router = app::build_router(
        pool,
        identity,
        tenancy,
        rate_limiter,
        clock,
        &config.public_base_url,
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("api listening on 0.0.0.0:8080");
    axum::serve(listener, router).await?;

    Ok(())
}
