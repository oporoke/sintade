---
name: day
description: Start and complete the next day of the Sintade daily build plan (or a specific day number). Use when the user types /day, "start day", "next day" or "continue the plan".
---

# /day — run one day of the build plan

Arguments: optional day number (`/day 14`). Without it, use the current day from `docs/plan/PROGRESS.md`.

## 1. Orient (no code yet)

1. Read `docs/plan/PROGRESS.md`. Determine the day to run.
   - If the requested day is later than the current day, stop and ask: days are not skipped.
   - If the current day is marked In progress, resume it instead of starting fresh.
2. Read that day's row in `docs/plan/daily-build-plan.md`, plus its milestone heading and the previous day's row.
3. Read every design section the deliverable touches in `docs/design.md` (data model §8, API §9, flows §10, NFRs §11, conventions §17, tests §18 as relevant).
4. Verify the baseline: `git status` clean, on up-to-date `main`, `just check` and `just test` green. If red, report and fix the baseline first, logging it in PROGRESS.md.

## 2. Plan (wait for approval)

Present a short plan and **wait for the user to approve** before writing code:

- Goal: the deliverable in one sentence.
- Check: exactly how the Check column will be proven.
- Files to create/modify, migrations, routes, jobs, events.
- Tests to add (unit, integration, tenant-isolation rows, E2E).
- Risks or ambiguities, and anything you propose to defer.

If the day looks larger than one day, propose a split here.

## 3. Build

1. `git switch -c feat/day-NNN-<slug>`.
2. Mark the day In progress in PROGRESS.md.
3. Implement in small, reviewable commits (Conventional Commits). Write tests alongside or first.
4. Follow every rule in `CLAUDE.md`. Do not start later days' work.

## 4. Verify

1. Run the Check and capture evidence.
2. Run `just check` and `just test`; run `just e2e` if capture, upload or playback changed.
3. Re-read the diff for: tenant scoping, secrets in logs, `unwrap`, missing tests, layering violations.

## 5. Close

1. Update `README.md` status markers (📋 → 🟡/✅) for anything now implemented; update `.env.example`, `justfile`, docs as needed.
2. Write an ADR in `docs/adr/` if a decision was made or changed.
3. Update `docs/plan/PROGRESS.md`: log row (day, date, deliverable, Done, PR/commit, evidence, notes), advance "Current day", add deferred items.
4. Open the PR (or merge per the user's instruction).
5. Report back in this format:

```
Day NNN — <deliverable>: DONE | PARTIAL | BLOCKED
Check: <evidence>
Tests: <added / passing>
Docs updated: <files>
Deferred: <items or none>
Next: Day NNN+1 — <deliverable>
```
