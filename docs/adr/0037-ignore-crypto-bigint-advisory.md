# ADR-0037: Ignore RUSTSEC-2026-0387 (crypto-bigint 0.5.5) until the AWS SDK moves off it

## Status

Accepted — 2026-10-25 (Day 90, by the owner's decision). Temporary: to be removed (see Deferred
in `docs/plan/PROGRESS.md`). `cargo audit --deny warnings` stays blocking in CI (ADR-0022).

## Context

RUSTSEC-2026-0387 reports timing variability in `crypto-bigint` operations meant to be constant
time; the fix is `crypto-bigint >= 0.7.6`. Sintade gets `crypto-bigint 0.5.5` only through
`aws-sigv4` (`aws-sdk-s3` → `aws-runtime` → `aws-sigv4`), and even the newest `aws-sigv4` (1.6.3)
still depends on 0.5.5 directly. There is no version to upgrade to.

In `aws-sigv4`, `crypto-bigint` is used only for **SigV4a** (the ECDSA key derived from the
secret, for multi-region access points). `platform::S3ObjectStore` signs MinIO requests with
standard SigV4 (HMAC-SHA256) and never uses SigV4a, so the affected code does not run.

The other eight advisories published the same day (rustls, rustls-webpki, webpki-roots,
webpki-root-certs, ctutils, tokio) were fixed by lockfile updates, not ignored.

## Decision

Ignore exactly `RUSTSEC-2026-0387` in `.cargo/audit.toml`, with this ADR as the reason. No other
advisory is ignored, and the audit job still fails on any new one.

## Consequences

- The ignore must be removed when an `aws-sigv4` release depends on `crypto-bigint >= 0.7.6`
  (`cargo update -p aws-sigv4`, then `cargo tree -i crypto-bigint@0.5.5` shows nothing).
- If Sintade ever uses SigV4a (e.g. S3 multi-region access points), this ADR no longer holds and
  the dependency must be fixed first.
