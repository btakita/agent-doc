---
agent_doc_session: 143cbb97-6295-45d1-add5-9c19e9c713bb
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

> **[EXTRACT from tasks/dreamable/robert-ross.md]** (2026-04-06T20:37:31Z)
>
### Re: claude-auto (HEAD)

**claude-auto** is a Claude Code plugin that enforces the **Ketchup Technique** — a TDD/TCR discipline for AI-assisted coding. It wraps Claude Code's hook system to automate quality gates, inject context reminders, and protect files.

#### How it works

Four hook integration points (defined in `hooks/hooks.json`):

| Hook | Trigger | What it does |
|------|---------|--------------|
| **SessionStart** | Session opens | Injects context reminders as `<system-reminder>` |
| **PreToolUse** | Before Edit/Write/Bash | Validates commits, enforces deny-list |
| **UserPromptSubmit** | User submits prompt | Injects task-aware reminders |
| **Stop** | Session ends | Auto-continue decision |

#### Commit validation (the core feature)

When Claude runs `git commit`, the PreToolUse hook intercepts it:

1. Gets staged diff + files + commit message
2. Loads validators from `validators/*.md` — each is a markdown file with YAML frontmatter that tells Claude what to check (no comments, tests required, TCR workflow, etc.)
3. **Batches 3 validators per Claude CLI call**: `claude -p --no-session-persistence --agent validator <prompt> --output-format json`
4. Each validator returns `{"decision":"ACK"}` or `{"decision":"NACK","reason":"..."}`
5. Any NACK blocks the commit. Message can include `[appeal: reason]` to trigger a reconsideration pass.

#### Reminders

Similar pattern — `reminders/*.md` files with YAML frontmatter specifying `when: hook: SessionStart|UserPromptSubmit` and a priority. The loader filters and sorts them, then returns content as `additionalContext` which Claude Code wraps in `<system-reminder>`.

#### Configuration

Stored in `.claude-auto/.claude.hooks.json`. Key settings:
- `autoContinue.mode`: `smart|non-stop|off`
- `validateCommit.mode`: `strict|warn|off`
- `denyList`: glob patterns for protected files

Configure via `/claude-auto:config set <key> <value>` inside Claude Code.

#### Install

```bash
/plugin marketplace add BeOnAuto/auto-plugins
/plugin install claude-auto
```

Or local dev: `claude --plugin-dir /path/to/claude-auto`

The Ketchup loop it enforces: write one failing test → write minimal passing code → `test && commit || revert` → repeat.

transfer claude-audio research to tasks/research/claudescore.md
<!-- agent:boundary:919f8620 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
