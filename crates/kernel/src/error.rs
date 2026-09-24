#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("{0}")]
    Validation(String),

    #[error("internal error: {0}")]
    Internal(String),
}
