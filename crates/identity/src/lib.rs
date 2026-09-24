#![deny(clippy::unwrap_used)]

mod app;
mod domain;
mod infra;

pub use app::{IdentityService, RegisterError, RegisterRequest};
pub use domain::{Email, EmailError, Password, PasswordError};
