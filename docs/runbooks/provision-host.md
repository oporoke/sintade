# Runbook: provision a host (staging or production)

Everything in the repo is ready to be deployed; what is below is what a person with accounts has
to do once. Nothing here can be done from CI. Order matters: DNS and the host first, then GitHub,
then the first deploy.

## 0. Decisions that are still open (`TODO: Verify`)

| Decision | Needed for |
| --- | --- |
| Domain, e.g. `app.<yours>` (and `storage.app.<yours>`) | DNS, TLS, `PUBLIC_BASE_URL`, the extension's host permission |
| VPS provider and region | The hosts; latency to Tanzania users (§2) |
| Off-site backup provider (a different provider from the VPS) | `deploy/backup/README.md` |
| SMTP provider | `SMTP_URL`: verification and "ready" emails |
| CDN in front (Cloudflare / BunnyCDN, §2) | If used: restore real client IPs (`ngx_http_realip_module` in `deploy/nginx.conf.template`), or rate limits see the CDN |

## 1. The host

1. Ubuntu 24.04 LTS, 4 vCPU / 8 GB / 160 GB for a single-host start (the worker wants the cores:
   a 30-minute 1080p take is ~10 min on 4 vCPU, ADR-0013). Separate worker and database hosts
   when load says so: start only some compose profiles on each (`deploy/compose.prod.yml`).
2. Firewall: allow 22 (your IPs only), 80 and 443. Nothing else: Postgres and MinIO are not
   published, the MinIO console listens on localhost.
3. Install Docker Engine + the compose plugin. Create a deploy user in the `docker` group with the
   CI public key in `authorized_keys`, and passwordless `sudo` for exactly `mkdir -p /etc/sintade`
   and `tee /etc/sintade/env`.
4. `sudo mkdir -p /opt/sintade /var/backups/sintade && sudo chown deploy /opt/sintade && sudo chown 999 /var/backups/sintade && sudo chmod 700 /var/backups/sintade`
   (999 is the `postgres` user in the image).
5. Time sync on (`timedatectl`): signed URLs and WAL timestamps depend on it.

## 2. DNS and TLS

Create `A`/`AAAA` records for `DOMAIN` and `storage.DOMAIN` to the host. Caddy requests the
certificates by itself on first start (ports 80/443 must be reachable); first start before DNS
propagates only delays it.

## 3. Secrets (GitHub → Settings → Environments → `production`, and `staging`)

Required reviewers on `production` = the manual approval of §19. Variables: `PRODUCTION_HOST`,
`PRODUCTION_SSH_USER`, `PRODUCTION_DOMAIN`. Secrets: `PRODUCTION_SSH_KEY`, `PRODUCTION_APP_ENV`
(contents of `/etc/sintade/env`) and `PRODUCTION_COMPOSE_ENV` (contents of `/opt/sintade/.env`),
both from `deploy/env.example`. Generate `SESSION_SECRET` with `openssl rand -base64 64 | tr -d '\n'`.
`RATE_LIMIT_SCALE` must not be set. Staging is the same with `STAGING_*` names.

Inventory of secrets (where each is used, who can read it) is kept in the handoff checklist (§22),
never the values. Rotate storage keys every 90 days (§19): create a new MinIO service account,
update `PRODUCTION_APP_ENV`, redeploy, delete the old account.

## 4. Backups and monitoring on the host

- Timers from `deploy/backup/README.md` (nightly base backup, 5-minute off-site mirror) and the
  `mc` alias for the off-site store for the `postgres`/deploy user.
- Sentry: create a project (Rust); put its DSN in `SENTRY_DSN`. Alert rules, all "notify by email":
  *any new issue*, *an issue seen > 20 times in 10 minutes*. The worker's `OpsWatchdog` reports
  as `ERROR` events named `ALERT <kind>`, so they arrive as Sentry issues too.
- `ALERT_EMAIL` gets the watchdog's own mail (stuck queue, dead jobs, failing archiving), at most
  one per kind per hour. It needs working SMTP: also set up an external uptime check on
  `https://DOMAIN/readyz` (any free monitor; alert after 2 failures), because a dead API or SMTP
  can't announce itself.
- Host metrics (disk, CPU) from the provider's panel or `node_exporter`: alert at 80 % disk
  (`disk-full.md`).

## 5. First deploy

Merge to `main` (images are built per commit), tag a commit `vX.Y.Z`, push the tag, approve the
`deploy-production` job. Or by hand on the host, in an emergency, with the files already in place:
`/opt/sintade/deploy.sh <git sha>`. `docs/runbooks/deploy.md` has the routine and the rollback.
