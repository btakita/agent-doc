# agent-doc Functional Specification

> Language-independent specification for the agent-doc interactive document session tool.
> This document captures the exact behavior a port must reproduce.

## 1. Overview

agent-doc manages interactive document sessions between a human and an AI agent.
The human edits a markdown document, sends diffs to the agent, and the agent's
response is appended. Session state is tracked via YAML frontmatter, snapshots,
and git commits.

## 2. Document Format

### 2.1 Session Document

Frontmatter fields:
- `session`: Agent session ID (set after first run, used for `--resume`)
- `agent`: Agent backend name (overrides config default)
- `model`: Model override (passed to agent backend)
- `branch`: Reserved for branch tracking

All fields are optional and default to null. The body alternates `## User` and `## Assistant` blocks.

### 2.2 Frontmatter Parsing

Delimited by `---\n` at file start and closing `\n---\n`. If absent, all fields default to null and entire content is the body.

## 3. Snapshot System

### 3.1 Storage

Snapshots live in `.agent-doc/snapshots/` relative to CWD. Path: `sha256(canonical_path) + ".md"`.

### 3.2 Lifecycle

- **Save**: After successful run, full content saved as snapshot
- **Load**: On next run, loaded as "previous" state for diff
- **Delete**: On `reset`, snapshot removed
- **Missing**: Diff treats previous as empty (entire doc is the diff)

## 4. Diff Computation

Line-level unified diff via `similar` crate. Returns `+`/`-`/` ` prefixed lines, or None if unchanged.

## 5. Agent Backend

### 5.1 Trait

`fn send(prompt, session_id, fork, model) -> (text, session_id)`

### 5.2 Resolution Order

1. CLI `--agent` flag
2. Frontmatter `agent` field
3. Config `default_agent`
4. Fallback: `"claude"`

### 5.3 Claude Backend

Default: `claude -p --output-format json --permission-mode acceptEdits`. Session handling: `--resume {id}` or `--continue --fork-session`. Appends `--append-system-prompt` with document-mode instructions. Removes `CLAUDECODE` env var. Parses JSON: `result`, `session_id`, `is_error`.

### 5.4 Custom Backends

Config overrides `command` and `args` for any agent name.

## 6. Config

Location: `{XDG_CONFIG_HOME}/agent-doc/config.toml` (default `~/.config/agent-doc/config.toml`).

Fields: `default_agent`, `[agents.{name}]` with `command`, `args`, `result_path` (reserved), `session_path` (reserved).

## 7. Commands

### 7.1 run

`agent-doc run <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

1. Compute diff → 2. Build prompt (diff + full doc) → 3. Branch if `-b` → 4. Send to agent → 5. Update session ID → 6. Append response → 7. Save snapshot → 8. `git add -f` + commit

First run prompt wraps full doc in `<document>` tags. Subsequent wraps diff in `<diff>` tags + full doc in `<document>`.

### 7.2 init

`agent-doc init <FILE> [TITLE] [--agent NAME]` — scaffolds frontmatter + `## User` block. Fails if exists.

### 7.3 diff

`agent-doc diff <FILE>` — prints unified diff to stdout.

### 7.4 reset

`agent-doc reset <FILE>` — clears session ID, deletes snapshot.

### 7.5 clean

`agent-doc clean <FILE>` — squashes all `agent-doc:` commits for file into one via `git reset --soft`.

### 7.6 audit-docs

`agent-doc audit-docs` — checks CLAUDE.md/AGENTS.md/README.md/SKILL.md for tree path accuracy, line budget (1000), staleness, and actionable content. Exit 1 on issues.

### 7.7 start

`agent-doc start <FILE>` — start Claude in a new tmux pane and register the session.

1. Ensure session UUID in frontmatter (generate if missing)
2. Read `$TMUX_PANE` (must be inside tmux)
3. Register session → pane in `sessions.json`
4. Exec `claude` (replaces process)

### 7.8 route

`agent-doc route <FILE>` — route a `/agent-doc` command to the correct tmux pane.

1. Read session UUID from frontmatter
2. Look up pane in `sessions.json`
3. If pane alive → `tmux send-keys` `/agent-doc <FILE>` + Enter
4. If pane dead → auto-start cascade (create session/window, start Claude, register)

### 7.9 claim

`agent-doc claim <FILE>` — claim a document for the current tmux pane.

1. Ensure session UUID in frontmatter (generate if missing)
2. Read `$TMUX_PANE` (must be inside tmux)
3. Register session → pane in `sessions.json`

Unlike `start`, does not launch Claude — the caller is already inside a Claude session. Last-call-wins: a subsequent `claim` for the same file overrides the previous pane mapping.

### 7.10 prompt

`agent-doc prompt <FILE>` — detect permission prompts from a Claude Code session.

- Captures tmux pane content, strips ANSI, searches for numbered-option patterns
- Returns JSON: `{ "active": bool, "question": str, "options": [...] }`
- `--answer N` navigates to option N and confirms
- `--all` polls all live sessions, returns JSON array

### 7.11 commit

`agent-doc commit <FILE>` — git add + commit with auto-generated timestamp.

### 7.12 skill

`agent-doc skill install` — write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. Idempotent (skips if content matches).

`agent-doc skill check` — compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.

### 7.13 upgrade

`agent-doc upgrade` — check crates.io for latest version, upgrade via GitHub Releases binary download → cargo install → pip install (cascade).

## 8. Session Routing

### 8.1 Registry

`sessions.json` maps document session UUIDs to tmux panes:

```json
{
  "cf853a21-...": {
    "pane": "%4",
    "pid": 12345,
    "cwd": "/path/to/project",
    "started": "2026-02-25T21:24:46Z",
    "file": "tasks/plan.md"
  }
}
```

Multiple documents can map to the same pane (one Claude session, multiple files).

### 8.2 Use Cases

| # | Scenario | Command | What Happens |
|---|---|---|---|
| U1 | First session for a document | `agent-doc start plan.md` | Creates tmux pane, launches Claude, registers pane |
| U2 | Submit from JetBrains plugin | Plugin `Ctrl+Shift+Alt+A` | Calls `agent-doc route <file>` → sends to registered pane |
| U3 | Submit from Claude Code | `/agent-doc plan.md` | Skill invocation — diff, respond, write back |
| U4 | Claim file for current session | `/agent-doc claim plan.md` | Skill delegates to `agent-doc claim` → updates sessions.json |
| U5 | Claim after manual Claude start | `/agent-doc claim plan.md` | Fixes stale pane mapping without restarting |
| U6 | Claim multiple files | `/agent-doc claim a.md` then `/agent-doc claim b.md` | Both files route to same pane |
| U7 | Re-claim after reboot | `/agent-doc claim plan.md` | Overrides old pane mapping (last-call-wins) |
| U8 | Pane dies, plugin submits | Plugin `Ctrl+Shift+Alt+A` | `route` detects dead pane → auto-start cascade |
| U9 | Install skill in new project | `agent-doc skill install` | Writes bundled SKILL.md to `.claude/skills/agent-doc/` |
| U10 | Check skill version after upgrade | `agent-doc skill check` | Reports "up to date" or "outdated" |
| U11 | Permission prompt from plugin | PromptPoller polls `prompt --all` | Shows bottom bar with numbered hotkeys in IDE |

### 8.3 Claim Semantics

`claim` binds a document to a **tmux pane**, not a Claude session. The pane is the routing target — `route` sends keystrokes to the pane. Claude sessions come and go (restart, resume), but the pane persists. If Claude restarts on the same pane, routing still works without re-claiming.

Last-call-wins: any `claim` overwrites the previous mapping for that document's session UUID.

## 9. Git Integration

- Commit: `git add -f {file}` (bypasses .gitignore) + `git commit -m "agent-doc: {timestamp}" --no-verify`
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one
