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

## 8. Git Integration

- Commit: `git add -f {file}` (bypasses .gitignore) + `git commit -m "agent-doc: {timestamp}" --no-verify`
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one
