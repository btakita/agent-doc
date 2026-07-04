---
name: feedback-tsift-skill-primary-source
description: tsift instruction architecture — SKILL.md is the primary source; AGENTS.md lean canonical + CLAUDE.md symlink; dispatch to runbooks
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 8786e0ec-df49-4d42-9d2f-f4408fbc6afa
---

For tsift's agent instruction files, **SKILL.md is the primary source of truth** (full tsift API access, dispatching to runbooks for token-efficient on-demand loading). `AGENTS.md` is the lean canonical entrypoint that defers to SKILL + runbooks (no duplicated command reference) and keeps the `tsift init`-managed `<!-- tsift:code-navigation -->` block; `CLAUDE.md` is a symlink → `AGENTS.md` (per [[feedback_agents_md_canonical]], AGENTS.md is canonical). Extract detailed content (command reference, conventions, hook integration, graph module) into runbooks rather than inlining.

**Why:** user directive 2026-06-05 — "SKILL.md should be the primary source." Historically the skill at `.claude/skills/tsift/SKILL.md` was a thin copy derived FROM `src/tsift/CLAUDE.md`; the user wants that inverted so SKILL.md owns the full surface and other files defer to it.

**How to apply:** when restructuring tsift instruction files, make SKILL.md comprehensive + runbook-dispatching, keep AGENTS.md/CLAUDE.md thin pointers, and dedup redundant content into the SKILL + runbooks. Keep `tsift-version` frontmatter synced to `src/tsift/Cargo.toml`.
