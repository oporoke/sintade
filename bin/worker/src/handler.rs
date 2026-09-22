use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("invalid payload: {0}")]
    InvalidPayload(#[from] serde_json::Error),

    #[error("{0}")]
    Failed(String),
}

pub struct JobCtx {
    pub job_id: platform::JobId,
    pub attempt: i32,
}

pub trait JobHandler: Send + Sync + 'static {
    const KIND: &'static str;
    type Payload: DeserializeOwned + Send;

    fn run(
        &self,
        ctx: &JobCtx,
        payload: Self::Payload,
    ) -> impl Future<Output = Result<(), JobError>> + Send;
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type DispatchFn = Box<
    dyn Fn(JobCtx, serde_json::Value) -> BoxFuture<'static, Result<(), JobError>> + Send + Sync,
>;

#[derive(Default)]
pub struct HandlerRegistry {
    handlers: HashMap<&'static str, DispatchFn>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<H>(&mut self, handler: H)
    where
        H: JobHandler + Clone + 'static,
    {
        self.handlers.insert(
            H::KIND,
            Box::new(move |ctx, raw_payload| {
                let handler = handler.clone();
                Box::pin(async move {
                    let payload: H::Payload = serde_json::from_value(raw_payload)?;
                    handler.run(&ctx, payload).await
                })
            }),
        );
    }

    pub async fn dispatch(
        &self,
        kind: &str,
        ctx: JobCtx,
        payload: serde_json::Value,
    ) -> Result<(), JobError> {
        match self.handlers.get(kind) {
            Some(dispatch) => dispatch(ctx, payload).await,
            None => Err(JobError::Failed(format!(
                "no handler registered for kind {kind:?}"
            ))),
        }
    }
}
