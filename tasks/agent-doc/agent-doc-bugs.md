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
### Re: skill-harness dependency

Noted. This is an architecture decision for agent-doc's dependency chain:

- agent-doc depends on skill-harness (for skill installation/propagation)
- agent-doc depends on agent-runbooks (for bundled runbooks)
- `agent-doc skill install` already exists — it installs the SKILL.md into `.claude/skills/agent-doc/`

The question is whether skill-harness should be a separate crate that standardizes skill installation across tools (not just agent-doc), and whether agent-runbooks should be a separate spec repo that agent-doc depends on for its runbook format.

This belongs in the instruction-files task scope. Transferred.

Ok...does agent-doc already depend on instruction-files?
### Re: agent-doc already depends on instruction-files

Yes. `Cargo.toml` has `instruction-files = "0.1"`. It's used by `agent-doc audit-docs` which validates CLAUDE.md/AGENTS.md/SPEC.md consistency.

Update instruction-files dependency.
Is there any skill install logic with instruction-files or it's dependencies that can handle installing skills?
### Re: Skill install in instruction-files

No. instruction-files has no skill install logic — it only discovers and audits existing files. The skill install capability is entirely in agent-doc (`src/skill.rs` — `agent-doc skill install`).

To make this generic, a `skill-harness` crate (or extending instruction-files) would need:
- A standard skill manifest format (what files to install, where)
- An install command that copies skill files to `.claude/skills/<name>/`
- A check command that verifies installed skill matches source

This is new functionality for the instruction-files expansion. Added to the task.
upgrade instruction-files.
See tasks/software/instruction-files.md for the current solution(s).
### Re: Upgrade instruction-files

agent-doc depends on `"0.1"` but local source is `0.2.0`. Tracked in tasks/software/instruction-files.md. Bump happens when that task's features land.

Anything remaining from updates to tasks/software/instruction-files.md.
agent-doc should use instruction-files 0.2
### Re: Bumped instruction-files to 0.2 (HEAD)

Done. `Cargo.toml`: `"0.1"` -> `"0.2"`. Fixed 4 clippy collapsible-if warnings in preflight.rs. 458 tests pass, clippy clean.
<!-- agent:boundary:509ea28c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
instruction-files 0.2 in use. Remaining expansion tracked in tasks/software/instruction-files.md.
<!-- /agent:pending -->
