#![deny(clippy::unwrap_used)]

mod app;

pub use app::{
    MessagingError, MessagingService, Recipient, RecipientDirectory, RecordingReadyMessage,
};
