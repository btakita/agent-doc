---
name: AGENTS.md is canonical, CLAUDE.md is symlink
description: When creating contact files, AGENTS.md is the canonical file and CLAUDE.md symlinks to it — not the reverse
type: feedback
---

AGENTS.md is the canonical file (committed). CLAUDE.md is a symlink to AGENTS.md.

**Why:** This matches the dual-name convention documented in corky's CLAUDE.md and Presence AI's CLAUDE.md. AGENTS.md is portable across agent tools; CLAUDE.md is read natively by Claude Code.

**How to apply:** When creating any new `AGENTS.md`/`CLAUDE.md` pair (contacts, packages, etc.), write content to `AGENTS.md` first, then `ln -s AGENTS.md CLAUDE.md`. Never create `CLAUDE.md` as the canonical file with `AGENTS.md` as symlink.
