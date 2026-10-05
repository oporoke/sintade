# Launch checklist and go / no-go (MVP, "10 developer users")

The decision is the product owner's. This lists what is proven, what is not, and what only a
person can do. Evidence lives in `docs/demos/M08-hardening-launch.md` and the PROGRESS log.

## Exit criteria (docs/design.md §12, MVP)

| Criterion | State | Evidence / what is missing |
| --- | --- | --- |
| 30-minute 1080p recording on Chrome, Edge, Firefox, Safari uploads and plays everywhere | **Partly** | Processing a 30-min 1080p take: 10 min on 4 cores (ADR-0013). 10-minute recordings upload and play on Chromium and Firefox (M4–M6 demos). **Not shown:** a 30-minute browser recording (the free plan stops at 10 min; a paid entitlement is needed), Edge as a recording browser (extension only), Safari (no `MediaRecorder` in the Linux test build: macOS check) |
| Killing the tab at minute 10 and reopening recovers the recording with no gaps | **Shown at minute 9** (see M8 demo) | `m8-rehearsal.spec.ts`: whole recording back, no frame gap over 2 s; "minute 10" is the plan's limit, so the kill is at 9:00 |
| Dropping network for 60 s mid-recording loses nothing | **Shown** | M4 demo (Chromium, Firefox), `upload-loss.spec.ts` on every PR |
| Stop-to-link ≤ 5 s p95 on the reference setup | **Shown locally** | `m8-rehearsal.spec.ts`, 20 recordings, production rate limits. **Reference setup does not exist** (no staging/production host): re-run there |
| Cross-tenant access tests pass; no public bucket paths | **Shown** | Generated tenant-isolation harness on every PR (Days 18, 60, 67); `platform` test `the_bucket_is_private_and_a_signature_opens_one_object`; production smoke check |
| Extension installed from the Chrome Web Store starts a recording from any tab with click highlights visible | **Not met** | Works installed unpacked in Chrome and Edge (M7). Not in any store: needs the owner's store accounts (`docs/store/SUBMISSION.md`) |

## Before inviting anyone: things only a person can do

1. **Provision production** (`docs/runbooks/provision-host.md`): domain and DNS, VPS, off-site
   backup provider, SMTP provider, Sentry project, GitHub `production` environment with required
   reviewers and secrets. Then tag and deploy (`docs/runbooks/deploy.md`) and confirm
   `https://<domain>/readyz`.
2. **First real restore** from the off-site copy on that host (`docs/runbooks/restore.md`, drill log).
3. **Legal:** terms of service and privacy policy reviewed by a Tanzanian advocate and published
   (`docs/tos.md` is a draft; the privacy page is a draft; both have `[bracketed]` items).
4. **Store submission** for the extension (optional for a developer-user launch: they can load it
   unpacked).
5. **Real-device checks automation can't make:** Safari on macOS (record, upload, watch), the
   toolbar click and a real microphone with the extension, a phone watching a link.
6. **Payments are not part of the MVP:** all users are on the free plan (50 recordings, 10 min).
   Say so in the invite.

## The ten users

- Pick them from the target segment (developers and tech teams in Tanzania, §1): five who record
  bug reports and walkthroughs alone, five in a team that shares recordings.
- Send `invite.md`'s text by hand, one by one; create their accounts only through sign-up (no admin
  backdoor exists). Ask them to use it for real work for a week.
- A channel for questions (WhatsApp group or email alias): `[support contact]` — **TODO: Verify**.
- Watch: Sentry issues, `OpsWatchdog` alerts, the queue (`worker-stuck.md`), disk (`disk-full.md`),
  and each user's first recording end to end (did a link come out?).
- Success looks like (from §1): every user got a link within seconds of stopping, no recording
  lost, nobody blocked by a limit they didn't understand, at least eight would use it again.

## Known limits to tell them (also `invite.md`)

- Free plan: 50 recordings of up to 10 minutes, up to 1080p. Longer takes stop themselves at 10:00.
- Safari can't record yet (use Chrome, Edge or Firefox); Safari can watch.
- No restore from trash yet: deleting is final after the confirmation.
- Public links are not listed or indexed; "anyone with the link" works.
- No comments, transcripts, webcam bubble or team workspaces yet (V1).
- The extension is installed by loading a folder until it is in the stores.

## Rollback of the launch

Nothing to roll back for users: they can stop using it. For the system: `docs/runbooks/deploy.md`
(previous SHA), `docs/runbooks/restore.md` (data).
