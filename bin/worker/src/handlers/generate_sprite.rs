use std::sync::Arc;

use media::{GenerateSprite, MediaService, SpriteOutcome};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `GenerateSprite` (docs/design.md §10 Process step 6): the scrub sprite, its `sprite.vtt` and
/// the animated preview, made from the stored MP4. Enqueued when `ProcessTake` finishes; a take
/// whose MP4 isn't stored yet fails and is retried with backoff.
#[derive(Clone)]
pub struct GenerateSpriteHandler {
    media: Arc<MediaService>,
}

impl GenerateSpriteHandler {
    pub fn new(media: Arc<MediaService>) -> Self {
        Self { media }
    }
}

impl JobHandler for GenerateSpriteHandler {
    const KIND: &'static str = media::GENERATE_SPRITE;
    type Payload = GenerateSprite;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        match self.media.generate_sprite(payload).await {
            Ok(SpriteOutcome::Built { tiles, sheets }) => {
                tracing::info!(job_id = %ctx.job_id, tiles, sheets, "GenerateSprite done");
                Ok(())
            }
            Ok(SpriteOutcome::NotFound) => {
                tracing::info!(job_id = %ctx.job_id, "GenerateSprite had nothing to do");
                Ok(())
            }
            Err(error) => Err(JobError::Failed(error.to_string())),
        }
    }
}
