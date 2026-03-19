---
agent_doc_session: db08a891-c339-4a49-9f49-2d8a1f30ed10
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


# Resume Session

<!-- agent:status -->
**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)
**Profile Statements**: `resume/profile-statements.md` — LinkedIn headlines, tagline, summary
**agent-resume**: [btakita/agent-resume](https://github.com/btakita/agent-resume) — complete: build, render, validate, search-index commands; search.html for GitHub Pages
**Tests**: `make test` — 12 tests (search index validation + LightPanda DOM tests)
**Apex CVS**: `profiles/apex-cvs.toml` — custom profile for CVS Health via Deepak Rana (Apex Systems). PDF: `BrianTakita-apex-cvs.pdf`
<!-- /agent:status -->

## Completed

### Resume Content
- Markdown resume from LinkedIn PDF + briantakita.me portfolio/open-source mining
- Career Highlights section (pages 1-2): 8 one-liners spanning 20-year career
- 5 rounds proofread (casing, tense, corporate names, punctuation)
- 83+ highlight phrases tuned for: agentic loops, reactive programming, event-driven, ML, real-time systems, context engineering

### Build System
- `build.py` + Makefile → pandoc (fragment mode) + weasyprint with `resume.css`
- `profiles/default.toml`: phrase highlighting, skill bolding, section filtering
- GitHub Action: builds PDF, commits, deploys to GitHub Pages

### Key Decisions
- **Career Highlights** always shown (Option A) — benefits every reader, front-loads signal
- **Single profile** (default) — startup-founder profile removed; Career Highlights makes it redundant
- **LinkedIn headline**: Option 3 hybrid (212 chars) for recruiter/founder scanning
- **"multimodal"** not "multi-modal" (standard ML/AI literature)
- **BDD Pioneer** added — connects RSpec/rr to the BDD movement narrative
- **RR proxy innovation** — pioneered proxy-based test doubles, now standard as `patch` in pytest

### Foxley Talent — Full Stack Engineer
- **Role:** Full Stack Engineer (SaaS infrastructure), fully remote US, $150K-$180K + equity
- **Stack:** Python/Django backend, React/TypeScript frontend, API/distributed systems, CI/CD
- **Calendly notes:** Honest framing — Rails ↔ Django mapping, no Django overclaim, "days not weeks" framework pickup
- **Revised notes sent:** Presence AI pipeline, FastAPI/Flask/Graphene/SQLAlchemy, 10+ years Rails as Django equivalent

<!-- agent:exchange patch=append -->
### Session Summary

**agent-resume Rust crate** ([btakita/agent-resume](https://github.com/btakita/agent-resume))
- Hybrid architecture: Rust CLI (`build`, `render`, `validate`, `search-index`) assembles content from `data.toml`; Python handles pandoc+weasyprint PDF
- `build.py` refactored from 280→125 lines
- Search: `search.html` + `search-index.json` for GitHub Pages
- 12 pytest tests (search index validation + LightPanda DOM)
- data.toml: 14 experiences, 37+ skills, 45+ accomplishments

**Contacts created:**
- `mail/contacts/deepak-rana/CLAUDE.md` — Apex Systems recruiter, CVS Health role. 6 calls transcribed.
- `mail/contacts/diksha-jaiswal/CLAUDE.md` — Bitsoft International recruiter, Deloitte role. 4 calls transcribed.

**Resume profiles:**
- `profiles/apex-cvs.toml` — CVS Health Full Stack Developer (Python + React + GCP), $70/hr W2
- `profiles/deloitte-nodejs.toml` — Deloitte NodeJS Backend Engineer, $60/hr W2

**Emails sent via Gmail API:**
- Deepak Rana: RTR + resume for CVS Health (pushed to Gmail Drafts)
- Diksha Jaiswal: Rate confirmation + RTR + resume for Deloitte (sent directly)

**Corky v0.18.1** — Gmail API email sending (`gmail.compose` scope). Published to crates.io + GitHub Release.

**Resume repo** — committed and pushed to [btakita/resume](https://github.com/btakita/resume). README updated with agent-resume install instructions.

Create a contact for arun.s@objectwin.com with the email correspondence

### Re: Arunprakash S contact created

**Contact created:** `mail/contacts/arunprakash-s/CLAUDE.md`

| Field | Value |
|-------|-------|
| **Name** | Arunprakash S |
| **Company** | ObjectWin Technology Inc (Senior Recruiter) |
| **Email** | arun.s@objectwin.com |
| **Phone** | (838) 280-7886 |
| **Client** | Warner Bros Discovery (WBD) |
| **Role** | Back-End Developer (Rust) #30365-1 |
| **Contract** | W2, 12+ months (extension/conversion possible) |
| **Location** | Remote (EST preference) |
| **Rate** | $75/hr W2 all inclusive |
| **Skills** | 5+ yrs Rust, Node.js/Python/C#, cloud, SQL + document DBs, containers |
| **Team** | Media Technology & Operations (Live Sports, CNN, Cartoon Network, Adult Swim) |
| **Status** | RTR agreed (2026-03-18), resume submitted |

Strong fit — Rust-primary role with event-driven architecture (preferred), 12+ month contract at WBD.

3 email threads found in `mail/conversations/`.

Transcribe and diarize the recordings that I just added to arunprakash-s/recordings

Analyze correspondence and recordings.

Create a resume.

Discuss any gaps in the resume.

### Re: WBD Rust Backend Developer — transcription, analysis, resume

**1. Recordings transcribed** (2 calls):

| Call | Date | Duration | Summary |
|------|------|----------|---------|
| 1 | Mar 18 2:23 PM | 13 min | Role details, rate agreed $75/hr, resume requested urgently |
| 2 | Mar 18 5:02 PM | 1.5 min | Follow-up — needs resume with Rust highlighted ASAP |

**2. Analysis — 9/10 fit:**

**Strongest signals:**
- **Rust primary** — 5 years production Rust (corky, agent-doc, agent-kit, tmux-router, tagpath): Add Holochain experience. Developing the Holochain prototype for protocol.love. Which is a collaborative governance platform. This will be under my Freelance consulting job. See cogov-dev for more context. It is at https://github.com/cogov/protocol.love. Please evaluate as to whether or not this should be included in the resume. It is very basic but it is my first experience with Rust. And Holochain is a sophisticated crypto platform.
- **Milyoni/Warner Bros** — direct WBD domain experience (social entertainment platform for WB, Lions Gate, Universal  Music)
- **Event-driven architecture** — 15-year lineage (ZeroMQ at Presence AI, reactive frameworks, event-driven at Milyoni/TrueCar)
- **AWS** — 10 years (ECS, IAM, EC2, cloud infra)
- **Media/content systems** — Presence AI real-time video pipeline, Milyoni streaming platform

**3. Resume profile created:** `profiles/wbd-rust.toml`
**PDF built:** file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-wbd-rust.pdf

Experiences: Presence AI, Open Source (Rust), Freelance (AWS ECS), Milyoni (WB), TrueCar (MongoDB), Censible (reactive), Pivotal Labs (architecture)

**4. Gaps to be aware of (conversational, not resume-blockers):**

| Gap | Severity | Mitigation |
|-----|----------|-----------|
| **Temporal** (workflow engine) | Low — "preferred" not required | Agent-doc/tmux-router are workflow orchestration; event-driven patterns transfer |
| **Step Functions/Argo/Jenkins** | Low — preferred | 15-year CI/CD history, infrastructure automation |
I have extensive CI/CD experience. At:
- Pivotal Labs
- Honk/TrueCar
- Freelancing
  - Rundavoo
- Censible/Martin Investments
- PresenceAI
| **Container pipeline deployment** | Low | Docker 8 yrs, AWS ECS admin, Heroku deployments |
Experience covered by:
- Pivotal Labs (Heroku)
- Honk/TrueCar (Heroku)
- Freelancing (Docker)
  - Rundavoo
  - Menu.com
- Censible/Martin Investments (Heroku, Docker)
- PresenceAI (Docker)
| **Observability** | Low | Sub-100ms latency tuning at Presence AI implies monitoring |
- PresenceAI
- Censible/Martin Investments
- Honk/TrueCar
- Menu.com
- Rundavoo
- Pivotal Labs
| **Jira** | Trivial | Standard tool, not a skill gap |
- Pivotal Labs
- TrueCar

**No critical gaps.** The role requires Rust + cloud + event-driven — all Brian's strongest areas. The Milyoni/Warner Bros connection is a unique differentiator no other candidate likely has.

**Resume needs to go out ASAP** — Arunprakash followed up twice. Want me to draft and send the email with the resume attached?

Draft Response + Update Resume
<!-- /agent:exchange -->

<!-- agent:pending -->
- [x] Transcribe arunprakash-s recordings
- [x] Analyze correspondence and recordings
- [x] Create WBD Rust resume profile
- [x] Discuss resume gaps
- [ ] Send resume to Arunprakash (pending approval)
<!-- /agent:pending -->
