mod handler;
mod handlers;

use std::time::Duration;

use handler::{HandlerRegistry, JobCtx};
use handlers::noop::NoopHandler;
use platform::JobQueue;

const LOCK_DURATION: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = platform::Config::load()?;
    platform::init_telemetry(&config.rust_log);

    let pool = platform::connect(&config.database_url).await?;
    let queue = JobQueue::new(pool);

    let mut registry = HandlerRegistry::new();
    registry.register(NoopHandler);

    let worker_id = format!("worker-{}", std::process::id());
    tracing::info!(worker_id, "worker started");

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
