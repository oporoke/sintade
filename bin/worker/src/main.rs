mod handler;
mod handlers;
mod relay;

use std::sync::Arc;
use std::time::Duration;

use handler::{HandlerRegistry, JobCtx, JobHandler};
use handlers::noop::NoopHandler;
use handlers::send_email::SendEmailHandler;
use handlers::sweep_stale_uploads::SweepStaleUploadsHandler;
use ingest::IngestService;
use platform::{JobQueue, Mailer, SmtpMailer};
use relay::{OutboxRelay, SubscriberRegistry};
use sqlx::PgPool;

const FROM_ADDRESS: &str = "no-reply@sintade.app";

const LOCK_DURATION: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RELAY_FALLBACK_INTERVAL: Duration = Duration::from_secs(5);
/// How often `SweepStaleUploads` is scheduled (its thresholds are 24 h and 7 days).
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = platform::Config::load()?;
    platform::init_telemetry(&config.rust_log);

    let pool = platform::connect(&config.database_url).await?;
    let mailer: Arc<dyn Mailer> = Arc::new(SmtpMailer::new(&config.smtp_url, FROM_ADDRESS)?);
    let clock: Arc<dyn platform::Clock> = Arc::new(platform::SystemClock);
    // The worker talks to storage on the private network; it never presigns for browsers.
    let store: Arc<dyn platform::ObjectStore> = Arc::new(platform::S3ObjectStore::new(
        &config.s3_endpoint,
        &config.s3_bucket,
        &config.s3_access_key,
        &config.s3_secret_key,
    ));
    let ingest = Arc::new(IngestService::new(
        pool.clone(),
        Arc::new(catalog::CatalogService::new()),
        Arc::new(billing::BillingService::new()),
        store,
        clock,
    ));
    let worker_id = format!("worker-{}", std::process::id());
    tracing::info!(worker_id, "worker started");

    tokio::try_join!(
        run_job_loop(pool.clone(), worker_id, mailer, ingest),
        run_outbox_relay_loop(pool.clone()),
        run_scheduler_loop(pool),
    )?;
    Ok(())
}

/// Schedules the periodic jobs. Several workers may run this; `enqueue_unless_pending` keeps
/// one copy of each in the queue.
async fn run_scheduler_loop(pool: PgPool) -> anyhow::Result<()> {
    let queue = JobQueue::new(pool);
    loop {
        if let Some(job_id) = queue
            .enqueue_unless_pending(SweepStaleUploadsHandler::KIND, serde_json::json!({}))
            .await?
        {
            tracing::debug!(%job_id, "scheduled SweepStaleUploads");
        }
        tokio::time::sleep(SWEEP_INTERVAL).await;
    }
}

async fn run_job_loop(
    pool: PgPool,
    worker_id: String,
    mailer: Arc<dyn Mailer>,
    ingest: Arc<IngestService>,
) -> anyhow::Result<()> {
    let queue = JobQueue::new(pool);
    let mut registry = HandlerRegistry::new();
    registry.register(NoopHandler);
    registry.register(SendEmailHandler::new(mailer));
    registry.register(SweepStaleUploadsHandler::new(ingest));

    loop {
        match queue.claim_next(&worker_id, LOCK_DURATION).await? {
            Some(claimed) => {
                let ctx = JobCtx {
                    job_id: claimed.id,
                    attempt: claimed.attempts,
                };
                match registry.dispatch(&claimed.kind, ctx, claimed.payload).await {
                    Ok(()) => {
                        queue.complete(claimed.id).await?;
                    }
                    Err(error) => {
                        let outcome = queue.fail(claimed.id, &error.to_string()).await?;
                        tracing::warn!(job_id = %claimed.id, %error, ?outcome, "job failed");
                    }
                }
            }
            None => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }
}

/// Relays `outbox_events` to in-process subscribers. No subscribers are registered yet --
/// real modules register theirs here once they exist (e.g. media on `TakeFinalized`).
async fn run_outbox_relay_loop(pool: PgPool) -> anyhow::Result<()> {
    let registry = SubscriberRegistry::new();
    let relay = OutboxRelay::new(pool.clone());
    let mut listener = platform::listen(&pool, platform::NOTIFY_CHANNEL).await?;

    // Catch up on anything written before this worker started listening.
    relay.poll_once(&registry).await?;

    loop {
        tokio::select! {
            notification = listener.recv() => {
                notification?;
                relay.poll_once(&registry).await?;
            }
            () = tokio::time::sleep(RELAY_FALLBACK_INTERVAL) => {
                relay.poll_once(&registry).await?;
            }
        }
    }
}
