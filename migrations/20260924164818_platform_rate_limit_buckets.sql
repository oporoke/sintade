-- Fixed-window rate-limit counter, shared by any caller that needs one (login, signup, ...).
-- One row per (bucket key, current window); `check_and_increment` resets count on a new window.
CREATE TABLE rate_limit_buckets (
    key          text PRIMARY KEY,
    window_start timestamptz NOT NULL,
    count        integer NOT NULL DEFAULT 1
);
