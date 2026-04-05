---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.14 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-05 02:30)

**Releases:** v0.31.13 + v0.31.14

**Post-release fixes:** sync scaffold commit (snapshot before commit), transfer pending merge, granola.md pipeline investigation.

**Process codified:** SKILL.md "use the pipeline" principle, transfer-extract runbook prohibition, code-enforced-directives anti-patterns, precommit/prerelease ontology steps.

**Architecture transferred:** instruction-files expansion (11 items) to tasks/software/instruction-files.md.

**Key lesson:** Don't manually fix pipeline issues. Fix the software.

agent-doc should depend on skill-harness and agent-runbooks for propagating it's skills.
skill-harness should have mechanism to install skills. `agent-doc skill install` should install the agent-doc skill.
### Re: skill-harness dependency (HEAD)

Noted. This is an architecture decision for agent-doc's dependency chain:

- agent-doc depends on skill-harness (for skill installation/propagation)
- agent-doc depends on agent-runbooks (for bundled runbooks)
- `agent-doc skill install` already exists — it installs the SKILL.md into `.claude/skills/agent-doc/`

The question is whether skill-harness should be a separate crate that standardizes skill installation across tools (not just agent-doc), and whether agent-runbooks should be a separate spec repo that agent-doc depends on for its runbook format.

This belongs in the instruction-files task scope. Transferred.
<!-- agent:boundary:15c2bdab -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Transfer skill-harness + agent-runbooks dependency design to instruction-files task
<!-- /agent:pending -->
