mod app;
mod client_ip;
mod csrf;
mod error;
mod openapi;
mod rate_limit;
mod routes;
mod security_headers;
mod session;
#[cfg(test)]
mod tenant_isolation;
mod workspace_context;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `api openapi` prints the contract and exits -- no config, database or network needed.
    if std::env::args().nth(1).as_deref() == Some("openapi") {
        use utoipa::OpenApi;
        println!("{}", openapi::ApiDoc::openapi().to_pretty_json()?);
        return Ok(());
    }

    // `api migrate` applies the migrations and exits (the deploy runs it before the new API
    // starts, docs/design.md §19). It needs only the database.
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        return migrate_command().await;
    }

    let config = platform::Config::load()?;
    // Held for the life of the process: dropping it stops error reporting.
    let _telemetry = platform::init_telemetry(&config.rust_log, &config.error_reporting());

    rate_limit::set_scale(
        std::env::var("RATE_LIMIT_SCALE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1),
    );
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
    let mut store = platform::S3ObjectStore::new(
        &config.s3_endpoint,
        &config.s3_bucket,
        &config.s3_access_key,
        &config.s3_secret_key,
    );
    if let Some(public_endpoint) = &config.s3_public_endpoint {
        store = store.with_public_endpoint(
            public_endpoint,
            &config.s3_access_key,
            &config.s3_secret_key,
        );
    }
    let store: Arc<dyn platform::ObjectStore> = Arc::new(store);
    let ingest = Arc::new(ingest::IngestService::new(
        pool.clone(),
        Arc::new(catalog::CatalogService::new()),
        Arc::new(billing::BillingService::new()),
        store.clone(),
        clock.clone(),
    ));
    let router = app::build_router(
        pool,
        identity,
        tenancy,
        ingest,
        store,
        rate_limiter,
        clock,
        &config.public_base_url,
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("api listening on 0.0.0.0:8080");
    axum::serve(listener, router).await?;

    Ok(())
}

async fn migrate_command() -> anyhow::Result<()> {
    let url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("migrate needs DATABASE_URL"))?;
    let pool = platform::connect(&url).await?;
    platform::migrate(&pool).await?;
    println!("migrations are up to date");
    Ok(())
}
