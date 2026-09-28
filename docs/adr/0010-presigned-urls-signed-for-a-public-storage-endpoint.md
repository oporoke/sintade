# ADR-0010: Presigned URLs are signed for a public storage endpoint; dev proxies it same-origin

## Status

Accepted — 2026-09-28 (Day 37). Resolves the storage half of
[ADR-0006](0006-https-dev-server-with-same-origin-api-proxy.md)'s open question.

## Context

The browser `PUT`s chunks straight to object storage (CLAUDE.md rule 4). In dev the SPA is
served over HTTPS (`https://localhost:4200`, ADR-0006) and MinIO over plain HTTP
(`http://localhost:9010`). The Day 37 uploader e2e showed:

- Chromium and Firefox treat `http://localhost` as potentially trustworthy and allow the `PUT`.
- WebKit blocks it as mixed content ("[blocked] The page at https://localhost:4200/… requested
  insecure content from http://localhost:9010/…"), and so would Safari.

In production, storage is reached over HTTPS at a public host (a CDN or storage domain) that is
not the address the API uses to reach MinIO on the private network. SigV4 presigned URLs sign
the host, so a URL must be signed for the host the browser will send.

## Decision

1. `platform::S3ObjectStore` keeps two clients. The internal one (`S3_ENDPOINT`) does `HEAD`,
   list and delete. The presign client signs URLs for `S3_PUBLIC_ENDPOINT` when it is set
   (`with_public_endpoint`), and for `S3_ENDPOINT` otherwise.
2. In dev, `S3_PUBLIC_ENDPOINT=https://localhost:4200`, and `web/proxy.conf.json` proxies
   `/<bucket>` (`/sintade-dev`) to MinIO with `changeOrigin: false`. The `Host` header stays
   `localhost:4200`, which is what the URL was signed for, and the path is unchanged
   (path-style), so MinIO accepts the signature. The upload is same-origin: no mixed
   content, no CORS.
3. CI's e2e job starts MinIO and sets `S3_PUBLIC_ENDPOINT` the same way. Rust tests don't set
   it; they `PUT` straight to MinIO.

## Consequences

- The dev proxy path is the bucket name. If `S3_BUCKET` changes, `proxy.conf.json` must too.
- Production must set `S3_PUBLIC_ENDPOINT` to the HTTPS storage host (`TODO: Verify` with the
  deploy days: CDN in front of MinIO vs a storage subdomain) and allow its CORS origin.
- `MINIO_API_CORS_ALLOW_ORIGIN` stays configured, for tools and for a future cross-origin
  production host.
