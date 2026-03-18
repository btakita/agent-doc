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

<!-- agent:exchange patch= -->

### Re: SPEC.md + pending component

**Created `.claude/skills/dev/SPEC.md`** — a project-agnostic specification for the dev skill. Another project can use this SPEC.md as a behavioral contract to build their own `/dev` skill.

**SPEC.md structure (10 sections):**
1. **Overview** — thin orchestrator architecture, no CLI binary needed
2. **Architecture** — invocation → skill → agent → toolchain → targets
3. **Commands** — check, release, status, changelog (behavioral contracts)
4. **Project Type Detection** — marker file → project type mapping (Cargo.toml, package.json, etc.)
5. **Pre-Release Verification Checklist** — 9 mandatory checks including SPEC.md and drift detection
6. **Release Workflows** — generic workflow + publication target table with verify commands
7. **Secret Handling** — resolution protocol + path registry pattern
8. **Meta-Cognition** — trigger → action table + update protocol
9. **Workspace Integration** — cross-project updates + conventions
10. **Customization** — three-layer model: SPEC.md (invariant) → SKILL.md (workspace config) → project CLAUDE.md (overrides)

**Key design insight:** SPEC.md is the **invariant layer** (what the skill MUST do), SKILL.md is the **configuration layer** (workspace-specific tables, secret paths, dependency graphs). Any project can take the SPEC.md and write their own SKILL.md to match their workspace.

**Pending component** — now active. Using it going forward.

**Truncated message:** Your message "I want " appears cut off again. Could you complete it?
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] Add SPEC.md for the dev skill (portable spec for other projects)
- [ ] Respond to truncated message: "I want " (awaiting completion)
<!-- /agent:pending -->
