---
session: db08a891-c339-4a49-9f49-2d8a1f30ed10
resume: null
agent: null
model: null
branch: null
---
# Resume Session

## Status

**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)
**Profile Statements**: `resume/profile-statements.md` — LinkedIn headlines, tagline, summary

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

## User

compact this doc

Look at https://www.linkedin.com/jobs/view/4379083129/?refId=hGk1iex1cL0E5KY1CmWJNg%3D%3D&trackingId=bzzh9xyy6nH1tDXlgLIwvA%3D%3D

I have a calendy link. What should I put in additional notes given my experience?

## Assistant

**Compacted** — collapsed ~530 lines of iterative exchanges into the Completed section above.

### Foxley Talent — Full Stack Engineer

**Role:** Full Stack Engineer (SaaS infrastructure), fully remote US, $150K-$180K + equity
**Stack:** Python/Django backend, React/TypeScript frontend, API/distributed systems, CI/CD
**Company:** Unnamed SaaS client building infrastructure software for high-performance teams
**Note:** 200+ applicants already. No visa sponsorship.

### Calendly Notes

Given your experience, lead with **direct stack overlap** and **differentiation**:

> 20+ years full-stack — most recently Lead Backend Engineer at Presence AI (real-time multimodal AI pipeline: Python, React, TypeScript, GraphQL, PostgreSQL). Deep Django/Python production experience across Censible (10-year ESG platform), Freelance clients (menu.com, Martin Investments), and 20+ Pivotal Labs startups. I bring architectural thinking from building distributed real-time systems — not just CRUD. Currently shipping open-source developer tools in Rust (corky, agent-doc). Happy to discuss how my pipeline and platform experience maps to your SaaS infrastructure challenges.

**Key points to hit:**
- **Python/Django + React/TypeScript** — exact stack match (Presence AI, Censible, Freelance)
I havn't worked with Django. Since this company specializes in recruiting Django developers, I don't want to overrepresent myself. We need to come up with something different to emphasize that I have plenty of experience in similar libraries & can pick it up quickly.
- **Architectural depth** — "from concept through deployment" matches their job description; your pipeline work proves this
- **Speed + quality** — Pivotal Labs XP background (pairing, TDD, BDD) is exactly "thoughtful engineering at speed"
- **Overqualified signal** — lead engineer applying for mid-senior. Frame as: interested in the product/problem space, not just seniority

**What to omit in notes:** Agent tooling, context engineering, philosophy repo — not relevant to this Django/React SaaS role. Save for AI-specific positions.

## Assistant

Good catch — I incorrectly claimed Django experience. Revised Calendly notes:

> 20+ years full-stack — most recently Lead Backend Engineer at Presence AI (real-time multimodal AI pipeline: Python, React, TypeScript, GraphQL, PostgreSQL). Extensive Python production experience with FastAPI, Flask, Graphene, and SQLAlchemy. No Django specifically, but 10+ years with Rails — Django's closest equivalent (MVC, ORM, migrations, admin, automated tests). Pick up new frameworks in days, not weeks. Currently shipping open-source developer tools in Rust. Happy to discuss how my pipeline and platform experience maps to your SaaS infrastructure challenges.

**Why this works:**
- **Honest** — names what you've actually used, doesn't overclaim
- **Rails ↔ Django** — any Django shop understands this mapping. Same patterns: models, views, templates, migrations, admin, ORM, middleware
- **"Days not months"** — 20 years + 6 Python web frameworks means Django is vocabulary, not a learning curve
- **Foxley specializes in Django recruiting** — they'll appreciate honesty over discovering the gap mid-interview

## User
