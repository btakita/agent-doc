---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_mode: template
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange -->
Converted component from `output` to `exchange`. Write prompts here — both user and agent share this surface.

commit submodule. release any pending features + bug fixes. commit & push agent-loop

**Completed** — submodules committed, pointers updated, agent-loop pushed.

| Submodule | Version | Action |
|-----------|---------|--------|
| mail | main@a153669 | Committed 39 files (conversations, contacts, drafts) + pushed |
| agent-doc | v0.14.5+2 | Pointer updated (JetBrains plugin fixes, no crate release needed) |
| corky | v0.14.0 | Pointer updated |
| tmux-router | v0.2.4 | Pointer updated |

Parent commit: `cca8d88` — includes all submodule pointers + 22 task doc updates. Pushed to `main`.

Only untracked item remaining: `src/agent-kit` (new, not yet added to parent).

audit docs

**Audit docs completed.**

| Project | Issues | Action |
|---------|--------|--------|
| agent-doc | 5 → 1 (warning only) | Updated README.md + SPEC.md for route cascade & sync_after_claim. Touched SKILL.md + CLAUDE.md (reviewed, not stale). Domain Ontology table stays in CLAUDE.md per convention. |
| corky | 3 → 0 | Updated README.md + SPEC.md — transcribe now enabled by default (CPU). Touched AGENTS.md (reviewed, not stale). |

Submodule commits pushed, parent pointers updated → `234d137`. Pushed to `main`.
<!-- /agent:exchange -->
