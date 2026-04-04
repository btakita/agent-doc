---
agent_doc_session: 0f91fe16-548b-403c-9a53-79ebe1a9ed79
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Session Summary

**Runbook extraction session** (2026-04-04)

### Completed
- Extracted 14 runbooks from 4 SKILL.md files (dev, email, livestream, module-harness)
- Token savings: ~2,235t (34%) per skill invocation
- Created 3 corky/email runbooks: review-inbox, draft-reply, enrich-contact
- Added btakita/agent-runbooks submodule at src/agent-runbooks/
- Implemented agent-doc notify subcommand (src/notify.rs, 8 tests passing, uncommitted)
- Saved feedback_distribution_aware_reorg.md to auto memory

### Key Decisions
- Runbooks co-locate with skills (distribution-aware, not centralized)
- Auto memory = guardrails (always-loaded); runbooks = procedures (on-demand)
- Notify format: blockquote with --source and --affects flags
- No contexts/ directory; scoped domains in .agent directory instead

### Transferred to agent-doc-bugs
- Review, commit, and release agent-doc with notify subcommand

Is there any deterministic cli functionality that should be added to agent-runbooks?
### Re: Deterministic CLI for agent-runbooks

Yes -- four commands that belong in the binary:

| Command | Purpose | Priority |
|---------|---------|----------|
| `audit <skill-dir>` | Validate SKILL.md runbook refs exist, find orphans | High |
| `list <skill-dir>` | List runbooks with metadata (name, trigger, lines) | Medium |
| `validate <file>` | Check runbook format (title, steps, refs) | Medium |
| `install <skill-dir> --from <template>` | Copy template into skill's runbooks/ | Low |

`audit` is highest priority -- it catches drift between SKILL.md references and actual runbook files. Could integrate with `make precommit` to prevent broken references.

All are deterministic (fixed inputs -> fixed outputs) and CI-runnable. Could start as `agent-doc runbook audit/list/validate` subcommands, then extract to standalone `agent-runbooks` crate if scope grows.

Should we make the project directory configurable? It will default to `.agent/runbooks`. We can't guarantee that `.agent/` is present.
### Re: Configurable project directory (HEAD)

Yes, configurable. Resolution chain:

1. `--dir <path>` CLI flag (explicit)
2. Walk up from CWD for `.agent/runbooks/` (project-local)
3. Walk up for `.claude/skills/*/runbooks/` (skill-local, current pattern)
4. Fallback: error

This way projects without `.agent/` still work via the `.claude/skills/` path. A `--dir` flag handles edge cases. Config file (`[runbooks] dir = "..."` in `.agent/config.toml`) can come later if needed.

For v1: walk-up search + `--dir` override. No config file needed yet.
<!-- agent:boundary:d1321bb2 -->
<!-- /agent:exchange -->a

## Pending / Not Built

<!-- agent:pending patch=replace -->
All runbooks session items complete. Remaining work transferred to agent-doc-bugs.
<!-- /agent:pending -->
