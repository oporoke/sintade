# Setting up Claude Code for the Sintade daily plan

Copy these into the repo root on Day 1:

```text
sintade/
├── CLAUDE.md                        # loaded automatically by Claude Code every session
├── .claude/skills/
│   ├── day/SKILL.md                 # /day [N]  — run one day of the plan
│   ├── status/SKILL.md              # /status   — where are we
│   └── demo/SKILL.md                # /demo     — close a milestone
├── docs/
│   ├── design.md                    # export of the design doc main tab (Markdown)
│   ├── plan/daily-build-plan.md     # export of the "Daily build plan" tab (Markdown)
│   ├── plan/PROGRESS.md             # state file the skills read and update
│   └── adr/                         # created as decisions are made
└── README.md                        # the master README
```

Daily loop:

1. `cd sintade && claude`
2. `/status` — confirm the current day and baseline.
3. `/day` — review and approve the plan it proposes, let it build, review the diff.
4. On a milestone's last day: `/demo`.
5. `/clear` between days so each day starts with fresh context (CLAUDE.md reloads automatically).
