CREATE TABLE jobs (
    id           uuid PRIMARY KEY,
    kind         text NOT NULL,
    payload      jsonb NOT NULL,
    run_at       timestamptz NOT NULL DEFAULT now(),
    attempts     integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL DEFAULT 5,
    locked_by    text,
    locked_until timestamptz,
    last_error   text,
    done_at      timestamptz,
    dead_at      timestamptz
);
CREATE INDEX jobs_ready ON jobs (kind, run_at) WHERE done_at IS NULL AND dead_at IS NULL;
