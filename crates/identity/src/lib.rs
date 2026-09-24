#![deny(clippy::unwrap_used)]

mod domain;

pub use domain::{Email, EmailError, Password, PasswordError};
