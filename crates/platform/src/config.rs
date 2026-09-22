use crate::error::PlatformError;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub rust_log: String,
    pub public_base_url: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub smtp_url: String,
}

impl Config {
    pub fn load() -> Result<Self, PlatformError> {
        Ok(Self {
            database_url: env_var("DATABASE_URL")?,
            rust_log: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            public_base_url: env_var("PUBLIC_BASE_URL")?,
            s3_endpoint: env_var("S3_ENDPOINT")?,
            s3_bucket: env_var("S3_BUCKET")?,
            s3_access_key: env_var("S3_ACCESS_KEY")?,
            s3_secret_key: env_var("S3_SECRET_KEY")?,
            smtp_url: env_var("SMTP_URL")?,
        })
    }
}

fn env_var(key: &'static str) -> Result<String, PlatformError> {
    std::env::var(key).map_err(|_| PlatformError::MissingEnvVar(key))
}
