---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
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

<!-- agent:exchange patch=append -->

### Re: Are we complete?

Yes — the `/dev` skill is complete as a v1. Here's what we built this session:

**Deliverables:**
| File | Lines | Purpose |
|------|-------|---------|
| `.claude/skills/dev/SKILL.md` | 270 | Workspace-specific skill (agent-loop) |
| `.claude/skills/dev/SPEC.md` | 403 | Portable behavioral contract |

**SKILL.md covers:**
- 5 commands: `check`, `release`, `status`, `changelog` + workspace-wide `status`
- 4 project types: Rust+Maturin, Rust lib, Python, JS/TS apps
- 9-point pre-release verification checklist (tests, audit-docs, README, VERSIONS, SPEC, test coverage, architecture drift, version bump, clean tree)
- Release workflows per project type with secret handling via `pass`
- Dependency ordering (instruction-files → agent-doc → corky)
- Composability hierarchy (SKILL.md → DEV.md → DEV.local.md)
- Meta-cognition self-update protocol

**SPEC.md covers:**
- 11 sections (all modular — adopt individually)
- Project type detection via marker files
- Generic release workflow + publication target table
- Secret handling protocol
- Composability with merge semantics
- Human-driven SPEC evolution + borrowing from other repos

**Ready to use:** `/dev agent-doc check` or `/dev corky release` will work now. The skill is live in the skills list.

**Future iterations** (when needed, not now):
- `DEV.overrides.md` for cross-cutting overrides
- `DEV.md` files for individual submodules as release patterns emerge
- Integration testing the full release workflow end-to-end

go
<!-- /agent:exchange -->

<!-- agent:input -->

<!-- /agent:input -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
