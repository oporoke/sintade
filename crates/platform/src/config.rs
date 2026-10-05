use crate::error::PlatformError;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub rust_log: String,
    pub public_base_url: String,
    pub s3_endpoint: String,
    /// Where browsers reach storage, if not `s3_endpoint` (ADR-0010). Presigned URLs are signed
    /// for this host.
    pub s3_public_endpoint: Option<String>,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub smtp_url: String,
    pub session_secret: String,
    /// Local disk for media processing (`WORKER_SCRATCH_DIR`, default: a `sintade-scratch`
    /// directory under the OS temp dir). Worker only.
    pub worker_scratch_dir: std::path::PathBuf,
    /// `FFMPEG_PATH` / `FFPROBE_PATH` (default: `ffmpeg` / `ffprobe` on `PATH`). Worker only.
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    /// `WORKER_CONCURRENCY` (default 2): jobs one worker runs in parallel. Worker only.
    pub worker_concurrency: usize,
    /// `WORKER_PROCESS_TAKE_CONCURRENCY` (default 1): how many of those may be `ProcessTake`.
    /// FFmpeg uses every core, so a second transcode just slows both. Worker only.
    pub worker_process_take_concurrency: usize,
    /// `SENTRY_DSN`: where errors are reported; unset in development.
    pub sentry_dsn: Option<String>,
    /// `APP_ENV` (`development`, `staging`, `production`), for error reports.
    pub app_env: String,
    /// `RELEASE`: the git SHA of the running image, for error reports.
    pub release: Option<String>,
    /// `ALERT_EMAIL`: who the operational alerts (stuck queue, dead jobs, failing backups) are
    /// mailed to, besides Sentry. Worker only.
    pub alert_email: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, PlatformError> {
        Ok(Self {
            database_url: env_var("DATABASE_URL")?,
            rust_log: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            public_base_url: env_var("PUBLIC_BASE_URL")?,
            s3_endpoint: env_var("S3_ENDPOINT")?,
            s3_public_endpoint: std::env::var("S3_PUBLIC_ENDPOINT")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            s3_bucket: env_var("S3_BUCKET")?,
            s3_access_key: env_var("S3_ACCESS_KEY")?,
            s3_secret_key: env_var("S3_SECRET_KEY")?,
            smtp_url: env_var("SMTP_URL")?,
            session_secret: env_var("SESSION_SECRET")?,
            worker_scratch_dir: std::env::var("WORKER_SCRATCH_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("sintade-scratch")),
            ffmpeg_path: env_or("FFMPEG_PATH", "ffmpeg"),
            ffprobe_path: env_or("FFPROBE_PATH", "ffprobe"),
            worker_concurrency: env_count("WORKER_CONCURRENCY", 2),
            worker_process_take_concurrency: env_count("WORKER_PROCESS_TAKE_CONCURRENCY", 1),
            sentry_dsn: env_opt("SENTRY_DSN"),
            app_env: env_or("APP_ENV", "development"),
            release: env_opt("RELEASE"),
            alert_email: env_opt("ALERT_EMAIL"),
        })
    }
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// A positive count; unset, empty, zero or unparsable falls back to `default`.
fn env_count(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|count| *count > 0)
        .unwrap_or(default)
}

fn env_var(key: &'static str) -> Result<String, PlatformError> {
    std::env::var(key).map_err(|_| PlatformError::MissingEnvVar(key))
}

impl Config {
    /// Where this process reports errors.
    pub fn error_reporting(&self) -> crate::ErrorReporting {
        crate::ErrorReporting {
            dsn: self.sentry_dsn.clone(),
            environment: self.app_env.clone(),
            release: self.release.clone(),
        }
    }
}
