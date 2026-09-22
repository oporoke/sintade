---
name: demo
description: Run the end-of-milestone demo and close the milestone in the Sintade build plan. Use on a milestone's last day or when the user types /demo.
---

# /demo — close a milestone

1. Identify the milestone from `docs/plan/PROGRESS.md` and confirm today is its last day (or the user asked explicitly).
2. List every day in the milestone and confirm each is logged Done. Anything not done blocks the demo; report it.
3. Write a demo script in `docs/demos/M<NN>-<slug>.md`: the end-to-end steps a person follows on staging to see everything built so far, with expected results.
4. Execute what can be automated (E2E, smoke tests against staging, health checks) and record results in the demo file.
5. Check the milestone against `docs/design.md` exit criteria (§12) and acceptance criteria (§15) it covers; tick what passes.
6. Update the progress tracker in `docs/design.md` §14 statuses for the milestone's items, `README.md` roadmap (§50) and status markers.
7. Update `docs/plan/PROGRESS.md`: milestone closed, date, demo file link, carry-over items.
8. Report: what was demonstrated, what failed, carry-over, and the next milestone's first day.
