#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    Forbidden(String),

    /// The request conflicts with the resource's current state (e.g. a finalized take).
    #[error("{0}")]
    Conflict(String),

    #[error("too many requests")]
    RateLimited,

    #[error("internal error: {0}")]
    Internal(String),
}
