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

### Re: Title correction & Rust crate question

**Title fix noted** — correcting the Presence AI example. Your actual title is "Director of Engineering" at Presence AI, not Co-Founder. The example TOML and phrases will reflect that.

**Should this be a Rust crate?** Honest tradeoff:

**Arguments for Rust:**
- Fits your ecosystem — agent-doc, tagpath, existence-lang are all Rust
- Could integrate as an `agent-doc` subcommand or standalone CLI
- TOML parsing is native (serde + toml crate)
- Tag matching / scoring is a good fit for Rust's type system
- Dog-food it the same way you dog-food agent-doc

**Arguments for keeping Python:**
- `build.py` already works — weasyprint and pandoc are Python-native
- The data layer (load TOML → score → select → assemble markdown) is ~200 lines of Python
- Adding Rust means maintaining two build paths or rewriting PDF generation
- Not performance-critical — runs once per resume build

**Pragmatic recommendation: Hybrid.** Keep weasyprint/pandoc in Python for PDF generation (no Rust equivalent is as good). Build the **data layer** as a Rust CLI that:
1. Loads experience TOML files + profile
2. Scores/selects accomplishments by tag overlap
3. Assembles the markdown template → stdout

Then `build.py` calls the Rust CLI for content assembly and handles PDF rendering.

**My vote:** Start with pure Python extension to `build.py` (fastest to ship), extract to Rust later if the scoring/assembly logic gets complex enough to warrant it.

**Corrected Presence AI example:**

```toml
[meta]
company = "Presence AI"
title = "Director of Engineering"
start = "2022-09"
end = "2025-12"
duration = "3 yrs 3 mos"
location = "Remote"
type = "startup"

[phrases]
headline = "Led engineering at AI avatar startup; built real-time video pipeline"
one_liner = "Real-time AI avatar pipeline with reactive signal architecture"
technical = "Reactive signal graph, WebRTC streaming, sub-100ms latency"
```

Ready to proceed with implementation. Which direction: Python-first or Rust crate?
<!-- /agent:exchange -->

<!-- agent:pending -->
- [x] Title correction: Presence AI = Director of Engineering, not Co-Founder
- [x] Rust crate vs Python decision for resume data layer
- [ ] Implement experience + skills database (awaiting direction: Python or Rust)
<!-- /agent:pending -->
