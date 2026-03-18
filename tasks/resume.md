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

### Re: Experience + Skills Database for Custom Resumes

Great direction. Your current system already has the right bones — plain template + profile-driven emphasis/filtering. The missing layer is **structured experience data** that lets you recombine content per job description instead of maintaining static prose.

**Proposed architecture:**

```
resume/
├── BrianTakita.md          # still the master template (generated from DB)
├── build.py                 # extended to assemble from DB
├── profiles/                # existing — controls emphasis + filtering
└── data/                    # NEW — the "database"
    ├── skills.toml          # skill taxonomy with tags
    ├── experiences/          # one file per role
    │   ├── presence-ai.toml
    │   ├── censible.toml
    │   ├── neo-innovation.toml
    │   └── ...
    └── projects/             # one file per shipped product
        ├── corky.toml
        ├── agent-doc.toml
        └── ...
```

**Experience entry structure** (`data/experiences/presence-ai.toml`):

Looks good...I'm not actually a Co-Founder though. I was a Director Level.
```toml
[meta]
company = "Presence AI"
title = "Co-Founder & Principal Engineer"
start = "2022-09"
end = "2025-12"
duration = "3 yrs 3 mos"
location = "Remote"
type = "startup"  # startup | enterprise | consulting | freelance

[tags]
domains = ["AI/ML", "real-time systems", "video", "WebRTC"]
skills = ["Python", "Rust", "TypeScript", "React", "FastAPI", "Docker", "AWS"]
patterns = ["reactive programming", "event-driven", "context engineering", "agentic workflows"]
level = ["architecture", "full-stack", "infrastructure", "team-lead"]

# Atomic accomplishments — each is a recombinable unit
[[accomplishments]]
text = "Built real-time AI avatar pipeline processing video/audio streams"
tags = ["AI/ML", "real-time", "video", "pipeline"]
impact = "core-product"

[[accomplishments]]
text = "Designed reactive signal graph for sub-100ms avatar response latency"
tags = ["reactive programming", "performance", "architecture"]
impact = "technical-innovation"

[[accomplishments]]
text = "Implemented WebRTC streaming infrastructure for live avatar sessions"
tags = ["WebRTC", "infrastructure", "real-time"]
impact = "infrastructure"

# Phrases — reusable fragments for prose assembly
[phrases]
headline = "Co-founded AI avatar startup; built real-time video pipeline"
one_liner = "Real-time AI avatar pipeline with reactive signal architecture"
technical = "Reactive signal graph, WebRTC streaming, sub-100ms latency"
```

**Skills taxonomy** (`data/skills.toml`):

```toml
[skills.rust]
display = "Rust"
category = "language"
tags = ["systems", "performance", "safety"]
years = 5

[skills.context_engineering]
display = "Context Engineering"
category = "practice"
tags = ["AI/ML", "agentic", "architecture"]
related = ["agentic workflows", "agent tooling"]
```

**How profiles would use it:**

```toml
# profiles/django-fullstack.toml
[target]
match_tags = ["Python", "Django", "React", "API", "full-stack"]
prioritize = ["backend", "infrastructure", "team-lead"]
max_bullets_per_experience = 4

[highlight]
# auto-generated from matched tags + manual overrides
auto_from_tags = true
extra_phrases = ["distributed systems"]
```

**Build flow changes:**
1. Load profile with `match_tags`
2. Score each experience/accomplishment by tag overlap with target
3. Select top-N accomplishments per role (sorted by relevance)
4. Assemble prose from accomplishment text + phrases
5. Apply existing emphasis/filtering pipeline

**Your message was truncated** at "All of which be recombined into effective prose for su..." — finish that thought, or proceed with this structure?

Yes.
Would it make sense to have a Rust Crate to support this tool?
<!-- /agent:exchange -->

<!-- agent:pending -->
<!-- /agent:pending -->
