---
agent_doc_session: 68563886-d4b4-4194-bbdf-a3d53239ab4d
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Make a profile for laura-s https://www.linkedin.com/in/laura-s-3356993/

❯ Add a profile for law-school-admission-council.

❯ Laura is a recruiter who send me a message.
```
Engineer Lead roles at the Law School Admission Council (LSAC); full-time, perm on W2; remote
Hi Brian,

I hope you’re doing well! 
Your impressive background in software development really caught my attention, as I am currently looking to hire two Engineer Leads at LSAC - the Law School Admission Council in full-time, permanent, remote positions.
I believe your experience would be a great addition to our teams.

Here is the link to the job description and compensation details, if you'd like to apply (the job description is the same for both positions): 
https://careers-lsac.icims.com/jobs/1736/engineer-lead---assessments-applications/job

If you’re open to new opportunities, I’d love to chat more about these roles and how they might align with your goals. Let me know if you’re interested!

Best regards,
Laura

Laura S. Dancovici 
Senior Recruiter 
office: 770.648.2143
email: ldancovici@lsac.org
```

### Re: Profiles created — Laura S. Dancovici + LSAC — opus-4.8

Both profiles created and committed in the `mail` submodule (`183ae41`, local only — not pushed).

**Contact — `mail/contacts/laura-s/`**
- `AGENTS.md` (+ `CLAUDE.md` symlink) — role, LinkedIn, email/phone, cross-link to the LSAC topic
- `20260701-linkedin-laura-s-dancovici.txt` — raw outreach message

**Company — `mail/topics/law-school-admission-council/`**
- `AGENTS.md` (+ `CLAUDE.md` symlink) — org overview, the Engineer Lead role, fit notes, cross-link to Laura

**Read on the opportunity:**

- **Laura is an *internal* LSAC recruiter** (`ldancovici@lsac.org`), not an agency — so unlike the Arron Fray outreach, there's no hidden client and no client-name to chase. She represents the hiring org directly.
- **LSAC** is the not-for-profit that runs the **LSAT** (plus LawHub and the Credential Assembly Service). Newtown, PA; the two openings are remote, full-time, permanent W2.
- **Role — Engineer Lead, Assessments Applications** (two identical seats). It's a lead/architecture role on an autonomous agile team in Technology Solutions & Platforms. The AI angle is real: expanding past GitHub Copilot into **LLMs, RAG, agentic workflows, responsible AI** — which lines up well with your agent-doc / tsift / Presence AI background. The data stack skews **Azure — Cosmos DB, SQL, Oracle, stored procs**, which is a notable difference from your usual Rust/TS/Python + SQLite/FalkorDB world.
- **Comp:** she points to the posting for details; a public listing showed roughly **$126K–$135K/yr** for a Newtown Engineer Lead — verify against the actual JD, since a lead + AI-scope role sometimes lands higher.

Note: the iCIMS posting (`jobs/1736`) is JavaScript-gated, so role detail was assembled from LSAC's site + public listings rather than the live posting — treat comp/stack specifics as "verify before relying."

Want me to draft a LinkedIn reply to Laura, or spin up a tailored resume for the Engineer Lead JD?
<!-- agent:boundary:e103e52d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
