#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("missing environment variable {0}")]
    MissingEnvVar(&'static str),

    #[error("database connection failed: {0}")]
    Database(#[from] sqlx::Error),
}
