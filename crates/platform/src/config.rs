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
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_var(key: &'static str) -> Result<String, PlatformError> {
    std::env::var(key).map_err(|_| PlatformError::MissingEnvVar(key))
}
