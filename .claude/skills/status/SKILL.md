---
name: status
description: Report where the Sintade build stands against the daily plan. Use when the user types /status or asks what day it is, what is next, or how far behind the plan is.
---

# /status

1. Read `docs/plan/PROGRESS.md` and `docs/plan/daily-build-plan.md`.
2. Run `git status`, `git log --oneline -10`, and `just check` (skip if it takes over 2 minutes; say so).
3. Report, briefly:
   - Current day and deliverable; In progress / not started.
   - Current milestone, days done in it, days left, next demo day.
   - Days completed overall out of 335, and whether the log shows slips (a day taking more than one working day).
   - Baseline health: CI/`just check` result, uncommitted changes, open branches.
   - Deferred items and known issues that affect the next 5 days.
4. Do not modify any files.
