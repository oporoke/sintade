# ADR-0005: MinIO Docker image mirrored to GHCR, not pulled from quay.io

## Status

Accepted — 2026-09-24

## Context

`docs/design.md` §2 specifies MinIO (self-hosted, S3 API) for object storage. Day 2 already
recorded one retreat by MinIO from free distribution: its Docker Hub images were archived
(worked around via [ADR from Day 2's PROGRESS.md log](../plan/PROGRESS.md), repointing
`compose.yml` and CI at `quay.io/minio/minio`) and per-bucket CORS was found to be
AIStor-only (paid).

On 2026-09-24, while building Day 13, CI's `backend` job failed pulling
`quay.io/minio/minio` with `401 unauthorized`. Direct verification (both from this machine and
via `curl https://quay.io/api/v1/repository/minio/minio`, which itself returned
`invalid_token`) confirmed this isn't a transient rate limit: MinIO has now also pulled its
images from quay.io, its other registry. Every prior day's MinIO usage (Day 2's `compose.yml`,
Day 5's `ObjectStore` tests, Day 10's CI) depended on one of these two now-dead registries.

This is a tech-stack-level break (§2), not a Day 13 concern, so it was raised with the user
before acting rather than silently worked around.

## Options considered

1. **Mirror the still-locally-cached image to a registry we control (GHCR).** The exact image
   (`quay.io/minio/minio:latest`, MinIO `RELEASE.2025-09-07T16-13-09Z`, digest
   `sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e`) was still present
   in this machine's local Docker cache from before the lockout. Pushing it to
   `ghcr.io/oporoke/sintade/minio` keeps MinIO itself unchanged, fixes CI immediately, and needs
   no re-verification of presigned URL or webhook behavior already built on Days 5–8.
2. **Replace MinIO with an actively-maintained S3-compatible alternative** (SeaweedFS, Garage,
   LocalStack). Addresses the deeper risk (MinIO's community edition is being wound down in
   favor of paid AIStor) but is a real architecture change: needs its own ADR revising §2, and
   re-verification of every `ObjectStore` behavior (presigned PUT/GET, `head`, `delete_prefix`,
   and the bucket-notification webhook the media pipeline will need from V1 onward).
3. **Do nothing / block on this.** Rejected — blocks every future day touching storage, and
   Day 13's actual deliverable (login) has nothing to do with it.

The user chose option 1.

## Decision

The cached MinIO image is retagged and pushed to `ghcr.io/oporoke/sintade/minio`, at both
`:latest` and the pinned, immutable tag `:RELEASE.2025-09-07T16-13-09Z`. `compose.yml`'s
`minio`/`minio-init` services and `.github/workflows/ci.yml`'s MinIO steps are repointed at the
pinned tag (not `:latest`, so a future re-push under the same "latest" pointer can't silently
change what CI and local dev run). The GHCR package (`sintade/minio`) is set to public so
CI can pull it anonymously, matching how `quay.io/minio/minio` worked before the lockout --
this is a one-way flip (GitHub does not allow a public container package to be made private
again), accepted since the mirrored content (an unmodified MinIO binary) is not sensitive.

The `api`/`worker`/`web` application images stay private on GHCR as already set up in Day 10 --
CI already authenticates for those via `GITHUB_TOKEN`, so no visibility change was needed there.

## Consequences

- `compose.yml` and CI no longer depend on MinIO's own registries at all; the only external
  dependency for object storage is now a registry this project's own GitHub account controls.
- The pinned tag means MinIO will not silently upgrade underneath local dev or CI; bumping it is
  a deliberate future action (re-mirror a newer release, retag, update the two references).
- This mirror is a stopgap for the immediate breakage, not a resolution of the deeper risk noted
  in option 2 above -- MinIO's continued retreat from open source (Docker Hub delisting, then
  quay.io delisting, paid-only bucket CORS) means self-hosted MinIO's long-term viability for
  this project is now genuinely in question. Revisit if MinIO breaks a third time, or before any
  day that would deepen the dependency (e.g. building on MinIO-specific bucket-notification
  webhooks for the V1 media pipeline).
