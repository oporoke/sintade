mod handler;
mod handlers;
mod limits;
mod recipients;
mod relay;

use std::sync::Arc;
use std::time::Duration;

use handler::{HandlerRegistry, JobCtx, JobHandler};
use handlers::noop::NoopHandler;
use handlers::process_take::ProcessTakeHandler;
use handlers::purge_trashed::PurgeRecordingHandler;
use handlers::send_email::SendEmailHandler;
use handlers::sweep_stale_uploads::SweepStaleUploadsHandler;
use ingest::IngestService;
use limits::KindLimits;
use media::{MediaService, MediaTools, ScratchSpace, TakeFinalizedMessage};
use messaging::{MessagingService, RecordingReadyMessage};
use platform::{JobQueue, Mailer, SmtpMailer};
use relay::{OutboxRelay, SubscriberError, SubscriberRegistry};
use sqlx::PgPool;

const FROM_ADDRESS: &str = "no-reply@sintade.app";

const LOCK_DURATION: Duration = Duration::from_secs(30);
/// How often a running job renews its lock: well inside `LOCK_DURATION`, so a long job (a
/// 30-minute take's transcode) is never reclaimed by another worker while it runs.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// Scratch directories this old at startup belong to no running job.
const SCRATCH_LEFTOVER_AGE: Duration = Duration::from_secs(6 * 60 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RELAY_FALLBACK_INTERVAL: Duration = Duration::from_secs(5);
/// How often `SweepStaleUploads` is scheduled (its thresholds are 24 h and 7 days).
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `worker transcode <input> <output.mp4> [mime]` runs the MP4 step of `ProcessTake` on local
    // files and exits -- no config, database or storage. For ops (re-making one recording by
    // hand) and the cross-browser playback e2e test.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("transcode") {
        return transcode_command(&args[2..]).await;
    }
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
        store.clone(),
        clock.clone(),
    ));
    let scratch = ScratchSpace::new(&config.worker_scratch_dir);
    match scratch.sweep_leftovers(SCRATCH_LEFTOVER_AGE).await {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "swept leftover scratch dirs"),
        Err(error) => tracing::warn!(%error, "could not sweep the scratch dir"),
    }
    let tools = MediaTools {
        ffmpeg: config.ffmpeg_path.clone(),
        ffprobe: config.ffprobe_path.clone(),
    };
    let media = Arc::new(MediaService::new(
        pool.clone(),
        store.clone(),
        Arc::new(catalog::CatalogService::new()),
        Arc::new(billing::BillingService::new()),
        clock.clone(),
        scratch,
        tools,
    ));
    let recordings = Arc::new(catalog::RecordingManager::new(
        pool.clone(),
        store,
        clock.clone(),
    ));
    let tenancy = Arc::new(tenancy::TenancyService::new(pool.clone()));
    let identity = Arc::new(identity::IdentityService::new(
        pool.clone(),
        JobQueue::new(pool.clone()),
        Arc::new(platform::SystemClock),
        config.public_base_url.clone(),
        identity::decode_session_secret(&config.session_secret),
        tenancy,
    ));
    let messaging = Arc::new(MessagingService::new(
        Arc::new(recipients::IdentityRecipients::new(identity)),
        JobQueue::new(pool.clone()),
        config.public_base_url.clone(),
    ));
    let worker_id = format!("worker-{}", std::process::id());
    tracing::info!(worker_id, "worker started");

    tokio::try_join!(
        run_job_loop(
            pool.clone(),
            worker_id,
            mailer,
            ingest,
            media.clone(),
            recordings,
            config.worker_concurrency,
            config.worker_process_take_concurrency,
        ),
        run_outbox_relay_loop(pool.clone(), media, messaging),
        run_scheduler_loop(pool),
    )?;
    Ok(())
}

async fn transcode_command(args: &[String]) -> anyhow::Result<()> {
    let [input, output, rest @ ..] = args else {
        anyhow::bail!("usage: worker transcode <input> <output.mp4> [mime-type]");
    };
    let mime = rest.first().map(String::as_str).unwrap_or_else(|| {
        if input.ends_with(".mp4") {
            "video/mp4"
        } else {
            "video/webm"
        }
    });
    let tools = MediaTools {
        ffmpeg: std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string()),
        ffprobe: std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string()),
    };
    let max_height = billing::FREE_TIER.max_resolution;
    let report = media::transcode_file(
        &tools,
        std::path::Path::new(input),
        media::Container::from_mime(mime),
        std::path::Path::new(output),
        max_height,
        0,
    )
    .await?;
    println!(
        "{output}: {:?}, {} {}x{}, {} ms, {} bytes",
        report.plan,
        report.mp4.video_codec,
        report.mp4.width,
        report.mp4.height,
        report.mp4.duration_ms.unwrap_or_default(),
        report.mp4_bytes
    );
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
        if let Some(job_id) = queue
            .enqueue_unless_pending(PurgeRecordingHandler::KIND, serde_json::json!({}))
            .await?
        {
            tracing::debug!(%job_id, "scheduled PurgeRecording");
        }
        tokio::time::sleep(SWEEP_INTERVAL).await;
    }
}

/// Runs `concurrency` job slots, at most `process_take_concurrency` of them on `ProcessTake`.
#[allow(clippy::too_many_arguments)]
async fn run_job_loop(
    pool: PgPool,
    worker_id: String,
    mailer: Arc<dyn Mailer>,
    ingest: Arc<IngestService>,
    media: Arc<MediaService>,
    recordings: Arc<catalog::RecordingManager>,
    concurrency: usize,
    process_take_concurrency: usize,
) -> anyhow::Result<()> {
    let mut registry = HandlerRegistry::new();
    registry.register(NoopHandler);
    registry.register(SendEmailHandler::new(mailer));
    registry.register(SweepStaleUploadsHandler::new(ingest));
    registry.register(ProcessTakeHandler::new(media));
    registry.register(PurgeRecordingHandler::new(recordings));
    let registry = Arc::new(registry);
    let limits = KindLimits::new().limit(
        ProcessTakeHandler::KIND,
        process_take_concurrency.min(concurrency),
    );
    tracing::info!(concurrency, process_take_concurrency, "job slots");

    let mut slots = tokio::task::JoinSet::new();
    for slot in 0..concurrency {
        slots.spawn(run_slot(
            JobQueue::new(pool.clone()),
            format!("{worker_id}-{slot}"),
            registry.clone(),
            limits.clone(),
        ));
    }
    // A slot only returns on a queue error, which is fatal for the worker.
    match slots.join_next().await {
        Some(result) => result?,
        None => Ok(()),
    }
}

async fn run_slot(
    queue: JobQueue,
    worker_id: String,
    registry: Arc<HandlerRegistry>,
    limits: KindLimits,
) -> anyhow::Result<()> {
    let all_kinds = registry.kinds();
    loop {
        let reserved = limits.reserve();
        let kinds = reserved.kinds(&limits, &all_kinds);
        match queue
            .claim_next_of(&worker_id, LOCK_DURATION, &kinds)
            .await?
        {
            Some(claimed) => {
                // Held until the job ends; frees the other reserved kinds right away.
                let _permit = reserved.keep(&claimed.kind);
                let ctx = JobCtx {
                    job_id: claimed.id,
                    attempt: claimed.attempts,
                    max_attempts: claimed.max_attempts,
                };
                let dispatched = registry.dispatch(&claimed.kind, ctx, claimed.payload);
                let result = with_heartbeat(&queue, claimed.id, &worker_id, dispatched).await;
                match result {
                    Ok(()) => {
                        queue.complete(claimed.id).await?;
                    }
                    Err(error) => {
                        let outcome = queue.fail(claimed.id, &error.to_string()).await?;
                        tracing::warn!(job_id = %claimed.id, %error, ?outcome, "job failed");
                    }
                }
            }
            None => {
                drop(reserved);
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}

/// Runs a job while renewing its lock every [`HEARTBEAT_INTERVAL`].
async fn with_heartbeat<T>(
    queue: &JobQueue,
    job_id: platform::JobId,
    worker_id: &str,
    job: impl std::future::Future<Output = T>,
) -> T {
    tokio::pin!(job);
    let mut beat = tokio::time::interval(HEARTBEAT_INTERVAL);
    beat.tick().await; // the first tick is immediate; the claim just set the lock
    loop {
        tokio::select! {
            result = &mut job => return result,
            _ = beat.tick() => {
                match queue.extend_lock(job_id, worker_id, LOCK_DURATION).await {
                    Ok(true) => {}
                    Ok(false) => tracing::warn!(%job_id, "job lock lost; another worker may run it"),
                    Err(error) => tracing::warn!(%job_id, %error, "could not renew the job lock"),
                }
            }
        }
    }
}

/// Relays `outbox_events` to in-process subscribers: media processes finalized takes,
/// messaging emails the creator when a recording is ready.
async fn run_outbox_relay_loop(
    pool: PgPool,
    media: Arc<MediaService>,
    messaging: Arc<MessagingService>,
) -> anyhow::Result<()> {
    let mut registry = SubscriberRegistry::new();
    registry.subscribe("TakeFinalized", move |payload| {
        let media = media.clone();
        async move {
            let message: TakeFinalizedMessage = match serde_json::from_value(payload) {
                Ok(message) => message,
                Err(error) => {
                    // Retrying can't fix a malformed event; log it and move on.
                    tracing::error!(%error, "TakeFinalized: unreadable event, skipped");
                    return Ok(());
                }
            };
            match media.enqueue_processing(message).await {
                Ok(_) => Ok(()),
                Err(media::EnqueueError::InvalidManifest(error)) => {
                    tracing::error!(%error, "TakeFinalized: invalid chunk manifest, skipped");
                    Ok(())
                }
                Err(error) => Err(SubscriberError::Failed(error.to_string())),
            }
        }
    });
    registry.subscribe("RecordingReady", move |payload| {
        let messaging = messaging.clone();
        async move {
            let message: RecordingReadyMessage = match serde_json::from_value(payload) {
                Ok(message) => message,
                Err(error) => {
                    tracing::error!(%error, "RecordingReady: unreadable event, skipped");
                    return Ok(());
                }
            };
            messaging
                .recording_ready(message)
                .await
                .map(|_| ())
                .map_err(|error| SubscriberError::Failed(error.to_string()))
        }
    });
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
