use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use sqlx::PgPool;

// No real subscriber is registered yet (see main.rs), so the bin target's dead-code lint
// can't see this being used even though tests exercise it.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum SubscriberError {
    #[error("{0}")]
    Failed(String),
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type SubscriberFn =
    Box<dyn Fn(Value) -> BoxFuture<'static, Result<(), SubscriberError>> + Send + Sync>;

#[derive(Default)]
pub struct SubscriberRegistry {
    subscribers: HashMap<String, Vec<SubscriberFn>>,
}

impl SubscriberRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn subscribe<F, Fut>(&mut self, event_type: &str, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), SubscriberError>> + Send + 'static,
    {
        self.subscribers
            .entry(event_type.to_string())
            .or_default()
            .push(Box::new(move |payload| Box::pin(handler(payload))));
    }

    async fn dispatch(&self, event_type: &str, payload: Value) -> Result<(), SubscriberError> {
        if let Some(subscribers) = self.subscribers.get(event_type) {
            for subscriber in subscribers {
                subscriber(payload.clone()).await?;
            }
        }
        Ok(())
    }
}

pub struct OutboxRelay {
    pool: PgPool,
}

impl OutboxRelay {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Claims a batch of undispatched events (`SKIP LOCKED`, so multiple relay instances
    /// don't double-process the same row), dispatches each to its registered subscribers, and
    /// marks it `dispatched_at` only on successful dispatch -- a failed dispatch leaves the row
    /// undispatched so it's retried on the next poll. Returns how many were dispatched.
    pub async fn poll_once(&self, registry: &SubscriberRegistry) -> Result<usize, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query!(
            r#"
            SELECT id, event_type, payload
            FROM outbox_events
            WHERE dispatched_at IS NULL
            ORDER BY id
            FOR UPDATE SKIP LOCKED
            LIMIT 20
            "#
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut dispatched = 0usize;
        for row in rows {
            if registry
                .dispatch(&row.event_type, row.payload)
                .await
                .is_ok()
            {
                sqlx::query!(
                    "UPDATE outbox_events SET dispatched_at = now() WHERE id = $1",
                    row.id
                )
                .execute(&mut *tx)
                .await?;
                dispatched += 1;
            }
        }

        tx.commit().await?;
        Ok(dispatched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::{DomainEvent, WorkspaceId};
    use platform::Outbox;
    use serde::Serialize;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    #[derive(Serialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        const EVENT_TYPE: &'static str = "test.relay-event";

        fn aggregate_id(&self) -> Uuid {
            Uuid::nil()
        }

        fn workspace_id(&self) -> WorkspaceId {
            WorkspaceId::new_v7()
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn event_reaches_subscriber_exactly_once(pool: PgPool) {
        let outbox = Outbox::new(Arc::new(platform::SystemClock));
        let mut tx = pool.begin().await.expect("begin tx");
        outbox
            .push(
                &mut tx,
                &TestEvent {
                    message: "hi".to_string(),
                },
            )
            .await
            .expect("push succeeds");
        tx.commit().await.expect("commit succeeds");

        let call_count = Arc::new(AtomicUsize::new(0));
        let mut registry = SubscriberRegistry::new();
        let counter = call_count.clone();
        registry.subscribe(TestEvent::EVENT_TYPE, move |_payload| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        let relay = OutboxRelay::new(pool.clone());
        let dispatched_first = relay.poll_once(&registry).await.expect("poll succeeds");
        assert_eq!(dispatched_first, 1);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let dispatched_second = relay.poll_once(&registry).await.expect("poll succeeds");
        assert_eq!(
            dispatched_second, 0,
            "an already-dispatched event is not reclaimed"
        );
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "subscriber must not be called again"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn failed_dispatch_leaves_event_undispatched_for_retry(pool: PgPool) {
        let outbox = Outbox::new(Arc::new(platform::SystemClock));
        let mut tx = pool.begin().await.expect("begin tx");
        outbox
            .push(
                &mut tx,
                &TestEvent {
                    message: "will fail once".to_string(),
                },
            )
            .await
            .expect("push succeeds");
        tx.commit().await.expect("commit succeeds");

        let call_count = Arc::new(AtomicUsize::new(0));
        let mut registry = SubscriberRegistry::new();
        let counter = call_count.clone();
        registry.subscribe(TestEvent::EVENT_TYPE, move |_payload| {
            let counter = counter.clone();
            async move {
                let attempt = counter.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(SubscriberError::Failed("boom".to_string()))
                } else {
                    Ok(())
                }
            }
        });

        let relay = OutboxRelay::new(pool.clone());
        let dispatched_first = relay.poll_once(&registry).await.expect("poll succeeds");
        assert_eq!(dispatched_first, 0, "failed dispatch is not marked done");

        let dispatched_second = relay.poll_once(&registry).await.expect("poll succeeds");
        assert_eq!(dispatched_second, 1, "the event is retried and succeeds");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
