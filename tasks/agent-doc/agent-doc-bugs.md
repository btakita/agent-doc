---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.16 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**instruction-files 0.2 upgrade** — bumped Cargo.toml dependency, fixed 4 clippy warnings.

**skill-harness architecture** — agent-doc should eventually depend on skill-harness + agent-runbooks; tracked in tasks/software/instruction-files.md.

**claim.rs scaffold bug (v0.31.16)** — claim wrote only UUID frontmatter on empty files; fixed to write full template (UUID + format + crdt + components) + snapshot + commit.

**Release v0.31.16** — crates.io published, tag force-updated, binary installed.

**Cross-session pane swap bug** — rescue_from_stash() used swap-pane without session guard; fixed with session check + join-pane for wrong-session panes (route.rs + sync.rs).

**resync --fix behavior** — auto-kill for idle shells intentional; agent processes left alive by design (registry can go stale).

**Session-drift detection in preflight** — Check 3 in check_layout(): reads sessions.json, queries pane sessions, emits layout_issues when panes span multiple sessions.

**audit-docs** — added layout_issues to SPEC.md §7.26, CLAUDE.md, README.md, SKILL.md.

**Partial compact (--keep N)** — added to agent-doc compact: keeps last N ### Re: sections, archives older; parse_topic_sections() helper; run_component_compact_partial(); 4 new tests; 462 tests pass, clippy clean, release binary built.

I just installed codex but I don't see `/codex` in autocomplete.
### Re: `/codex` not in autocomplete (HEAD)

Claude Code slash commands come from two sources:
1. **Built-in commands** (e.g., `/help`, `/compact`, `/clear`)
2. **Skills** — markdown files in `.claude/skills/<name>/SKILL.md`

Installing the Codex CLI (OpenAI's tool) doesn't automatically register a `/codex` skill in Claude Code. The two are separate systems — Claude Code doesn't scan `$PATH` for external CLIs and add them as slash commands.

If you want `/codex` available as a Claude Code slash command, you'd need to create a skill at `.claude/skills/codex/SKILL.md` that defines its behavior.

Is the goal to use Codex from within Claude Code sessions, or were you expecting it to auto-register from the install?
<!-- agent:boundary:fb987b6e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
