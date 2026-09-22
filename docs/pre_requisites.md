Start building. The design is done, and more documentation now has diminishing returns. Before Day 1, spend a few days on things with long lead times or that block the plan.

**Day 0 (1–3 days, before any code)**

| Priority | Action | Why now |
| --- | --- | --- |
| 1 | Register the domain (e.g. `sintade.dev` / `sintade.co.tz`), GitHub org, Chrome Web Store developer account | Name could be taken; extension submission is Day 66 |
| 2 | Start merchant onboarding with M-Pesa, Airtel Money, Mixx by Yas, HaloPesa | Approvals take weeks to months; you need them by Day 138 |
| 3 | Talk to 5–10 developers or tech teams who'd use it; line up the 10 launch users | Validates the first segment before 70 days of work |
| 4 | Choose a license and create the repo with the README | Removes the first TODOs; everything else hangs off the repo |
| 5 | Rent the staging VPS and estimate monthly infra cost (API, worker, MinIO, backups) | Staging is needed by Day 10; cost informs TZS pricing |

**Then from Day 1**

- Follow the daily plan exactly. One deliverable a day, and update the tracker at each milestone demo.
- Write ADRs 0001–0010 as each decision is exercised in code, not all up front.
- Re-verify the README against real code at the end of every milestone and flip its status markers.
- Start legal documents (terms, privacy policy) in parallel around Day 50. The extension store needs a privacy policy by Day 66.

**What not to do:** don't expand the docs further or start V1/V2 items early. The biggest risk now is time before real users touch it.