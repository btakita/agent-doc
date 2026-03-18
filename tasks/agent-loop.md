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

<!-- agent:exchange -->

### Re: SPEC.md verification + Meta-cognition

**SPEC.md added to pre-release checklist.** Two new rows in the verification table:
- **SPEC.md is current** — if one exists, verify it reflects current architecture. Flag drift.
- **No architecture-docs drift** — cross-check CLAUDE.md, README.md, and SPEC.md against actual code structure. Flag inconsistencies.

**Meta-cognition section added.** New `## Meta-Cognition — Self-Update` section in SKILL.md defines:

**When the skill self-updates:**
- Release fails due to missing step → add it
- New submodule added → update project tables + dependency ordering
- New publication target → add workflow section
- Pre-release check catches something new → add to checklist
- Secret path changes → update secret handling
- User corrects behavior → encode as rule

**Architecture-documentation drift detection** is now baked into both `/dev check` and `/dev release` — it compares CLAUDE.md, SPEC.md, and README.md against actual implementation and flags mismatches before proceeding.

The key insight: the skill accumulates operational knowledge over time. Each release teaches it something, and those lessons persist in the SKILL.md for the next session.

Updated SKILL.md is at `.claude/skills/dev/SKILL.md` — ready for a test drive or further iteration

Please use the agent:pending component.

Add a SPEC.md for this skill. So another project can use the SPEC.md to build up their own custom dev skill.

<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
