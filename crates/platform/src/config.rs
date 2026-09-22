use crate::error::PlatformError;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub rust_log: String,
    pub public_base_url: String,
}

impl Config {
    pub fn load() -> Result<Self, PlatformError> {
        Ok(Self {
            database_url: env_var("DATABASE_URL")?,
            rust_log: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            public_base_url: env_var("PUBLIC_BASE_URL")?,
        })
    }
}

fn env_var(key: &'static str) -> Result<String, PlatformError> {
    std::env::var(key).map_err(|_| PlatformError::MissingEnvVar(key))
}
