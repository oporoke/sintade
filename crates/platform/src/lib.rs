#![deny(clippy::unwrap_used)]

mod clock;
mod config;
mod db;
mod error;
mod job_queue;
mod object_store;
mod telemetry;

pub use clock::{Clock, FixedClock, SystemClock};
pub use config::Config;
pub use db::connect;
pub use error::PlatformError;
pub use job_queue::{ClaimedJob, FailOutcome, JobId, JobQueue, JobQueueError};
pub use object_store::{ObjectMeta, ObjectStore, S3ObjectStore, StorageError};
pub use telemetry::init as init_telemetry;
