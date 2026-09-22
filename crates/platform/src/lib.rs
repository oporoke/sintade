#![deny(clippy::unwrap_used)]

mod clock;
mod config;
mod db;
mod error;
mod telemetry;

pub use clock::{Clock, FixedClock, SystemClock};
pub use config::Config;
pub use db::connect;
pub use error::PlatformError;
pub use telemetry::init as init_telemetry;
