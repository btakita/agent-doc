---
session: cb5672cc-5d1d-4228-af30-640a336f692e
resume: null
agent: null
model: null
branch: null
---

## Session Summary

Philosophy integration for cross-project AI agent context. All work complete and pushed.

**What was built:**
- **`philosophy/install.sh`** — Creates `~/.claude/philosophy` symlink to git repo + writes 14-term kernel to `~/.claude/CLAUDE.md`. Idempotent. Accepts optional `TARGET_DIR`.
- **14-term kernel** in `~/.claude/CLAUDE.md` — Existence, Entity, Abstraction, Scope, Context, Resolution, Pattern, System, Domain, Focus, Perspective, Consciousness, Evolution, Story
- **README.md** — Three-layer architecture docs, install instructions, template system
- **AGENTS.md** — Full 14-term table for agent context

**Architecture (three layers):**
- **Layer 0: Kernel** — 14 terms in `~/.claude/CLAUDE.md`, always loaded in every session
- **Layer 1: Full ontology** — 152 nodes in `philosophy/src/`, referenced on demand via symlink
- **Layer 2: Domain overlays** — Each repo adds `## Domain Ontology` in its CLAUDE.md, extending base terms

**Key decisions:**
- `~/.claude/CLAUDE.md` is the right place (user-level, auto-loaded everywhere)
- Symlink `~/.claude/philosophy` → git repo for transparent read/write
- Domain overlays reference upstream nodes (inheritance chain, not duplication)
- Expanded from 12 to 14 terms (added Evolution, Story)
- Philosophy is a template system — others fork and customize

**Git state:** All committed and pushed. Session history squashed (`ac44158`).

## User


