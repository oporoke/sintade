#![deny(clippy::unwrap_used)]

mod error;
mod id;

pub use error::AppError;
pub use id::Id;
