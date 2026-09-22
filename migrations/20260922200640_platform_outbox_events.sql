CREATE TABLE outbox_events (
    id           bigserial PRIMARY KEY,
    event_type   text NOT NULL,
    aggregate_id uuid NOT NULL,
    payload      jsonb NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    dispatched_at timestamptz
);
