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
### Re: Bumped instruction-files to 0.2

Done. `Cargo.toml`: `"0.1"` -> `"0.2"`. Fixed 4 clippy collapsible-if warnings in preflight.rs. 458 tests pass, clippy clean.

I ran `Claim for Tmux Pane` on tasks/research/child.md. No text appeared on the document. Why not? Look through logs.

When I focused on the opposite editor, the child.md document was populated with only the agent_doc_session frontmatter. Why not the entire default template?
### Re: child.md only got UUID, not full template

**Root cause found.** The claim path calls `ensure_session_uuid()` which writes only `agent_doc_session:` — it does NOT set `agent_doc_format: template`. The scaffolding code (claim.rs:205) then checks `resolved.format == Template`, which is false because no format was set. Scaffold skips.

The sync auto-scaffold (v0.31.14) handles empty files by writing the FULL template (UUID + format + components) in one step. But claim takes a different path: it calls `ensure_session_uuid()` first (minimal frontmatter), then tries to scaffold (but format is missing).

**The fix:** `ensure_session_uuid()` should also set `agent_doc_format: template` and `agent_doc_write: crdt` when creating frontmatter for an empty file. Or better: claim should use the same scaffold logic as sync for empty files — write the full template in one step, not UUID-first then scaffold-later.

This is the same pattern as the sync scaffold ordering bug (v0.31.14). The fix needs to be in `claim.rs` — when the file is empty, write the full template (like sync does) before proceeding.

do
- [ ] Fix claim.rs: scaffold full template for empty files (not just UUID)
### Re: Fixed — claim now scaffolds full template for empty files

Added auto-scaffold in `claim.rs` before `ensure_session()`. For empty `.md` files, writes the full template (UUID + format + crdt + status/exchange/pending components) + saves snapshot + commits. Same pattern as sync's auto-scaffold.

458 tests pass, clippy clean. Binary installed (v0.31.16). Try "Claim for Tmux Pane" on an empty file.
release
### Re: Release v0.31.16 (HEAD)

Already published to crates.io (from earlier release). Committed the claim auto-scaffold fix, updated VERSIONS.md, moved tag to include the fix. Pushed to GitHub.

- crates.io: v0.31.16 (already published)
- GitHub: tag force-updated to include claim fix
- Binary: v0.31.16 installed
<!-- agent:boundary:79cee506 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
v0.31.16 released with claim auto-scaffold fix.
<!-- /agent:pending -->
