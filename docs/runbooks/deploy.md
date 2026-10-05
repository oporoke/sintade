# Runbook: deploy and roll back

## Routine (production)

1. Staging is green for the commit (its smoke test passed), the CI run of the commit is green.
2. Tag the commit on `main`: `git tag v0.1.0 <sha> && git push origin v0.1.0`.
3. The `deploy-production` job starts and **waits for approval** (a required reviewer). Check the
   commit one last time, approve.
4. The job: confirms the three images exist for that SHA → copies `deploy/` to `/opt/sintade` and
   writes the env files → runs `deploy.sh <sha>` on the host:
   pull all images → data services up → **migrate** (`api migrate`, one run) → worker restart
   (SIGTERM; the job in flight finishes or is re-queued) → API restart, wait healthy → web and
   Caddy → smoke test through TLS (`/readyz`, `/healthz`).
5. Watch for 15 minutes (§19): 5xx rate, p95, queue age (`worker-stuck.md` queries), Sentry.

Migrations must be backward-compatible with the version still running (add nullable → backfill
in a job → add constraint `NOT VALID` → `VALIDATE`; `CREATE INDEX CONCURRENTLY` in its own file).
That is what makes the order above and rollback safe.

## Rollback

- **App:** run `./deploy.sh <previous sha>` on the host (the failed script prints it), or re-run
  the deploy job for the previous tag. Images are immutable and pulled by SHA, never `latest`.
  `cat /opt/sintade/.env.deployed` is what is running now.
- **Database:** never "down-migrate". The previous image runs on the new schema by rule; fix
  forward. Restore from backup only for corruption: `restore.md`.
- **Web only:** the web image is one of the three; `deploy.sh` rolls them together.

## When the deploy job fails

| Where | Meaning | Do |
| --- | --- | --- |
| "images for this commit exist" | The commit never reached `main`, or `build-images` failed | Tag a commit that has images |
| pull | Registry/credentials | Nothing changed on the host; retry |
| migrate | A migration failed (the database is unchanged unless it was a no-transaction one) | Read the output; fix forward in a new migration; the old app is still running |
| API not healthy | New image can't start (config, DB) | `docker compose logs api`; roll back |
| smoke test | TLS/DNS/Caddy | `docker compose logs caddy`; certificates need ports 80/443 and DNS |

## Staging

Same job on every push to `main` (`deploy-staging`, needs the `STAGING_*` settings). A tag is a
staging deploy that someone approved.
