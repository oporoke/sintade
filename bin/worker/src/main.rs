mod handler;
mod handlers;
mod relay;

use std::time::Duration;

use handler::{HandlerRegistry, JobCtx};
use handlers::noop::NoopHandler;
use platform::JobQueue;
use relay::{OutboxRelay, SubscriberRegistry};
use sqlx::PgPool;

const LOCK_DURATION: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RELAY_FALLBACK_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = platform::Config::load()?;
    platform::init_telemetry(&config.rust_log);

    let pool = platform::connect(&config.database_url).await?;
    let worker_id = format!("worker-{}", std::process::id());
    tracing::info!(worker_id, "worker started");

    tokio::try_join!(
        run_job_loop(pool.clone(), worker_id),
        run_outbox_relay_loop(pool),
    )?;
    Ok(())
}

async fn run_job_loop(pool: PgPool, worker_id: String) -> anyhow::Result<()> {
    let queue = JobQueue::new(pool);
    let mut registry = HandlerRegistry::new();
    registry.register(NoopHandler);

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
