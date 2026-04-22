# Commands

All commands are available through the `agent-doc` CLI.

## run

```
agent-doc run <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]
```

Diff, send to agent, append response. The core command.

| Flag | Description |
|------|-------------|
| `-b` | Auto-create branch `agent-doc/<filename>` on first run |
| `--agent NAME` | Override agent backend |
| `--model MODEL` | Override model |
| `--dry-run` | Preview diff and prompt size without sending |
| `--no-git` | Skip git operations (branch, commit) |

Flow:
1. Compute diff from snapshot
2. Build prompt (diff + full document)
3. Pre-commit user changes (unless `--no-git`)
4. Send to agent
5. Append response as `## Assistant` block
6. 3-way merge if file was edited during response
7. Save snapshot (no post-commit — agent response stays as uncommitted changes)

## init

```
agent-doc init <FILE> [TITLE] [--agent NAME]
```

Scaffold a new session document with YAML frontmatter and a `## User` block. Fails if the file already exists.

## diff

```
agent-doc diff <FILE>
```

Preview the unified diff that would be sent on the next run. Useful for checking what changed before running.

## reset

```
agent-doc reset <FILE>
```

Clear the session ID from frontmatter and delete the snapshot. The next run starts a fresh session.

## clean

```
agent-doc clean <FILE>
```

Squash all `agent-doc:` git commits for the file into a single commit. Useful for cleaning up history after a long session.

## audit-docs

```
agent-doc audit-docs
```

Audit instruction files (CLAUDE.md, AGENTS.md, README.md, SKILL.md) against the codebase:
- Referenced file paths exist on disk
- Combined line budget under 1000 lines
- Staleness detection (docs older than source)
- Actionable content checks

## route

```
agent-doc route <FILE>
```

Route a `/agent-doc` command to the correct tmux pane. Looks up the session UUID from frontmatter, finds the pane in `sessions.json`, and sends the command via `tmux send-keys`. If the pane is dead, auto-starts a new Claude session.

## start

```
agent-doc start <FILE>
```

Start Claude in the current tmux pane and register the session. Ensures a session UUID exists in frontmatter, registers the pane in `sessions.json`, then execs `claude`.

## claim

```
agent-doc claim <FILE>
```

Claim a document for the current tmux pane. Reads the session UUID from frontmatter and `$TMUX_PANE`, then updates `sessions.json`. Unlike `start`, does not launch Claude — use this when you're already inside a Claude session.

Last-call-wins: a subsequent `claim` for the same file overrides the previous pane mapping. Multiple files can be claimed for the same pane.

Also available as a Claude Code skill: `/agent-doc claim <FILE>`.

## prompt

```
agent-doc prompt <FILE>
agent-doc prompt --all
agent-doc prompt --answer N <FILE>
```

Detect permission prompts from a Claude Code session by capturing tmux pane content.

| Flag | Description |
|------|-------------|
| (none) | Detect prompts for a single file |
| `--all` | Poll all live sessions, return JSON array |
| `--answer N` | Answer prompt by selecting option N (1-based) |

## commit

```
agent-doc commit <FILE>
```

Git add + commit with an auto-generated `agent-doc: YYYY-MM-DD HH:MM:SS` timestamp message.

## skill

```
agent-doc skill install
agent-doc skill check
```

Manage the Claude Code skill definition.

| Subcommand | Description |
|------------|-------------|
| `install` | Write the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md`. Idempotent. |
| `check` | Compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated. |

The skill content is embedded in the binary at build time. After `agent-doc upgrade`, run `agent-doc skill install` in each project to update the skill definition.

## patch

```
agent-doc patch <FILE> <COMPONENT> [CONTENT]
```

Replace content in a named component. Components are bounded regions marked with `<!-- agent:name -->...<!-- /agent:name -->`.

| Argument | Description |
|----------|-------------|
| `FILE` | Path to the document |
| `COMPONENT` | Component name (e.g., `status`, `log`) |
| `CONTENT` | Replacement content (reads from stdin if omitted) |

Behavior depends on `.agent-doc/components.toml` config:

| Config | Default | Description |
|--------|---------|-------------|
| `mode` | `replace` | `replace`, `append`, or `prepend` |
| `timestamp` | `false` | Auto-prefix with ISO timestamp |
| `max_entries` | `0` | Trim entries in append/prepend (0 = unlimited) |
| `pre_patch` | none | Shell hook: transform content (stdin → stdout) |
| `post_patch` | none | Shell hook: fire-and-forget after write |

See [Components](components.md) for full configuration and hook documentation.

## watch

```
agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]
```

Watch session files for changes and auto-submit.

| Flag | Default | Description |
|------|---------|-------------|
| `--stop` | | Stop the running watch daemon |
| `--status` | | Show daemon status |
| `--debounce` | `500` | Debounce delay in milliseconds |
| `--max-cycles` | `3` | Max agent-triggered cycles per file before pausing |

The daemon watches all claimed files (from `sessions.json`), debounces per-file, and triggers `agent-doc run` on changes. PID stored in `.agent-doc/watch.pid`.

Loop prevention: bounded cycles (default 3) and convergence detection (stop if agent response matches previous). See [Dashboard-as-Document](dashboard.md) for the full workflow.

## upgrade

```
agent-doc upgrade
```

Check crates.io for the latest version and upgrade. Tries GitHub Releases binary download first, then `cargo install`, then `pip install --upgrade`.

## Global flags

```
agent-doc --version    # Print version
agent-doc --help       # Show help
```
