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

## Completed

- Markdown resume created from LinkedIn PDF export + new content
- Resume repo: public at `btakita/resume`, submodule of agent-loop
- PDF pipeline: Makefile (`make pdf`) → pandoc + weasyprint with `resume.css`
- GitHub Action: triggers on .md/.css/Makefile changes, builds PDF, commits, deploys to Pages
- GitHub Pages: landing page at btakita.github.io/resume with PDF download
- README: workflow docs, build instructions, PDF link

### Content Changes

- **Presence AI**: dates updated to April 2024 – February 2026
- **Summary**: "Most recently built the full real-time AI avatar pipeline at Presence AI. Now focused on spec-driven agent tooling and collaborative intelligence workflows."
- **Tagline**: "Building emergent software systems through agent loops — spec-driven development, automated verification, and collaborative intelligence tooling where agents and humans work in parallel with full context. 20+ years full stack experience."
- **Top Skills**: Spec-Driven Development, Agents, Rust, Python, TypeScript, Zig
- **Open Source section**: 2002 – Present
  - **corky** — Multi-Source Correspondence Toolkit for Agent-Augmented Collaboration (Rust)
  - **agent-doc** — Document-as-Interface for Interactive Sessions with AI Agents (Rust)
  - Other contributions (italic, visually lighter): RSpec, SvelteJS, RMemo/RelementJS, rappstack, Access Query Analyzer, lazily-py, lazily-zig
- **Proofread**: 13 fixes (grammar, tense, parallelism, style)
- **SLOC metric removed** from corky heading
- **`| Docs` link removed** from corky GitHub line

### LinkedIn Drafts

**About section** (copy-paste ready):
Over 20 years of software development experience developing full-stack applications. Most recently built the full real-time AI avatar pipeline at Presence AI — facial landmark detection, LatentSync lip-sync inference, and frame sequencing on H100 GPUs. Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Currently dog-fooding two open-source tools: corky (git-native correspondence toolkit) and agent-doc (persistent AI document sessions) — both built through the same spec-driven agent methodology they enable.

**LinkedIn post draft**: `mail/social/20260224-agent-tooling-linkedin.md` (status: draft)
To publish: change `status: draft` → `status: ready`, then `corky social publish mail/social/20260224-agent-tooling-linkedin.md`.

### Social Media Posting (corky)

Implemented `corky social` module — commit `772c71e`, pushed to main.

**What was built:**
- 8 source files in `corky/src/social/` — platform enum, profiles.toml registry, YAML frontmatter drafts, URN-keyed token store, OAuth2 auth flow, LinkedIn API, publish orchestration, CLI dispatch
- 54 edge-case tests across 5 test files (P1-P9, D1-D9, T1-T9, A1-A5, PB1-PB10)
- SPECS.md §12 with full edge case table
- `make check` clean (clippy + 279 tests)

**CLI commands:**
```
corky social auth <platform> [--profile NAME]
corky social draft <platform> [BODY] [--author X] [--visibility public] [--tags X,Y]
corky social publish <file>
corky social check
corky social list [--status X]
corky social rename-author <old> <new>
```

**Next steps to use:**
1. Create LinkedIn app at [linkedin.com/developers](https://www.linkedin.com/developers/)
2. Enable "Share on LinkedIn" + "Sign In with LinkedIn using OpenID Connect"
3. Add `http://127.0.0.1:8484/callback` as redirect URI
4. Set credentials in `.corky.toml` under `[social.linkedin]` or via env vars

### Directory Naming Convention

Explored renaming `mail/` → `mailbox/`, `comms/`, `correspondence/`. Conclusion: **keep `mail/`**.

- `mailbox/` creates `mailbox/mailboxes/` path stutter (literal, not conceptual)
- `comms/` — militaristic tone clashes with "corky"
- `correspondence/` — too long (15 chars)
- Flatten approaches rejected — namespace collisions between mailbox names and system dirs
- "Mailbox" remains the correct internal abstraction (actor model precedent, Hewitt 1973)
- `mailboxes/` subdir is load-bearing architecture — namespace boundary preventing collisions

Documented in **SPECS.md §2.5 Directory Naming Convention**.

## Pending

*(none)*

## User

Is it possible to schedule the linked in post using their api?
