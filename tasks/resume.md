---
agent_doc_session: db08a891-c339-4a49-9f49-2d8a1f30ed10
tmux_session: '23'
agent_doc_mode: template
---


# Resume Session

<!-- agent:status -->
**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)
**Profile Statements**: `resume/profile-statements.md` — LinkedIn headlines, tagline, summary
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

<!-- agent:exchange -->
Switched to template mode. All prior conversation history compacted into the Completed section above (including Foxley Talent Calendly notes). Your edits incorporated: "automated tests" added to Rails-Django comparison, "months" → "weeks".

Write here for new topics.

Is my resume at risk of being flagged for fraud?
See https://www.linkedin.com/posts/amberio_ashbyhiddengems-activity-7437501367537229825-IgB8?utm_source=share&utm_medium=member_desktop&rcm=ACoAAAC0KQ4BlxIAK5Vbm7Gssr6SzIaiyNG0e68

### Resume Fraud Risk Assessment

Re: [Ashby fraud detection post](https://www.linkedin.com/posts/amberio_ashbyhiddengems-activity-7437501367537229825-IgB8) — Ashby's ATS flags suspicious candidate profiles (identity/pattern-based, not AI writing detection).

**Your main risk: overlapping dates.** Freelance (2019–2026) overlaps Presence AI and Censible. Kevin already flagged this. ATS systems with date-overlap checks would flag it too. The `copilotkit.toml` profile already removes Freelance — use filtered profiles for ATS submissions.

**Not a risk:** AI-assisted writing style. ATS fraud detection focuses on identity/profile patterns, not prose. All your claims are verifiable (GitHub repos, crates.io, npm, named employers, public commit history since 2006).

**Recommendation:** Always use a profile that removes overlapping Freelance dates for ATS submissions. For direct recruiter sends, explain overlap verbally.
<!-- /agent:exchange -->
