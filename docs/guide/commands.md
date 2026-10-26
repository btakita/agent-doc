# Commands

## Project runbooks

`agent-doc runbook list <FILE> [--json]` lists committed project procedures and
their prompt-preset associations. `agent-doc runbook show <FILE>
<#preset|catalog-name|runbooks/path.md> [--json]` loads one validated procedure. Author a new
non-overwriting scaffold with `agent-doc runbook create <FILE> <kebab-name>
[--preset <#id>] [--description <text>]`; association promotes a scalar preset
to `{prompt, runbook}` while preserving its prompt. See
[`runbooks/preset-runbooks.md`](../../runbooks/preset-runbooks.md).

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
7. Save snapshot and close out through the commit boundary. Successful runs commit the response in the same cycle and clean agent-owned post-commit drift back to `HEAD`; only genuine later local edits remain uncommitted.

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

## read

```
agent-doc read <FILE> [--component NAME]
```

Print the current document, or one named template component. If `NAME` is not
present, the error lists the valid components. This is a command-usage error
whether the document is open in an editor or detached; it does not trigger the
dogfood terminal-failure banner.

## reset

```
agent-doc reset <FILE>
agent-doc reset --from-current <FILE>
```

Clear the session ID from frontmatter and delete the snapshot/CRDT sidecars. Use `--from-current` after manually cleaning a corrupted document; it rebuilds the snapshot and CRDT state from the visible markdown so old sidecars cannot resurrect stale content.

## dogfood

```
agent-doc dogfood policy <FILE> [--json]
agent-doc dogfood set <FILE> [--path <PROJECT_RELATIVE.md>] [--auto <BOOL>]
```

`policy` reports the typed effective policy after session-over-project
precedence. `set` enables a document-local policy through editor-aware document
authority; an omitted path means the current document and omitted `auto` means
false. It updates only the public `dogfood:` frontmatter field and preserves the
rest of the document. `--path` rejects absolute paths, parent traversal, and
non-Markdown destinations.

## clean

```
agent-doc clean <FILE>
```

Squash all `agent-doc:` git commits for the file into a single commit. Useful for cleaning up history after a long session.

## audit-docs

```
agent-doc audit-docs [--root DIR]
```

Audit instruction files (CLAUDE.md, AGENTS.md, README.md, SKILL.md) against the codebase:
- Referenced file paths exist on disk
- Combined line budget under 1000 lines
- Staleness detection (docs older than source)
- Actionable content checks
- Generated agent-doc instruction surfaces match the running binary. From a submodule checkout, the default release audit checks the superproject install root; explicit `--root DIR` checks generated surfaces under that root exactly.

## route

```
agent-doc route <FILE>
```

Route a `/agent-doc` command to the correct tmux pane. Looks up the session UUID from frontmatter, finds the pane in `sessions.json`, and sends the command via the managed supervisor or tmux submit path. If no live owner exists, route auto-starts a route-owned pane and reaps it after the new document cycle commits; a startup miss records diagnostics and kills the just-created idle pane.

## start

```
agent-doc start <FILE>
```

Start the configured harness in the current tmux pane and register the session. Ensures a session UUID exists in frontmatter, registers the pane in `sessions.json`, then runs the harness under the supervisor restart loop. Route-created panes use an internal one-shot mode so successful fresh cycles close the pane automatically; manually started panes remain persistent.

If the YAML frontmatter is malformed, `start` now fails with a file-targeted error that includes a compiler-style frontmatter excerpt with a caret at the reported line/column, then tells you to fix the `--- ... ---` block before retrying. The sync/auto-start path also mirrors the same warning into the document's `agent:status` component when present, so editor-driven auto-start failures are visible even when no tmux pane appears.

## claim

```
agent-doc claim <FILE>
```

Claim a document for the current tmux pane. Reads the session UUID from frontmatter and `$TMUX_PANE`, then updates `sessions.json`. Unlike `start`, does not launch Claude — use this when you're already inside a Claude session.

Last-call-wins: a subsequent `claim` for the same file overrides the previous pane mapping. Multiple files can be claimed for the same pane.

Also available as a Claude Code skill: `/agent-doc claim <FILE>`.

## queue lineage

```
agent-doc queue lineage <FILE> [--item ID_OR_TEXT] [--generation N] [--verbose | --json]
```

Inspect durable worktree, integration, and release provenance for queue attempts.
The default prints one compact row per attempt. Use `--verbose` only when you
need repository paths, commit evidence, or diagnostics; use `--json` for tools.
Normal queue lifecycle commands record lineage silently and do not add lineage
annotations to the session document.

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

## doctor

```
agent-doc doctor <FILE> [--preflight-json PATH] [--session-check-json PATH] [--limit N] [--json]
```

Diagnose the session document against the workflow invariant catalog (alias: `diagnose`). Each invariant reports `ok`, `recoverable`, `operator`, or `blocked` with the exact repair commands or operator steps; the report goes to stdout (JSON with `--json`). The exit status is the overall (most severe) outcome, so scripts and CI can act on it:

| Exit | Outcome | Meaning |
|------|---------|---------|
| 0 | `ok` | Every invariant holds. |
| 3 | `recoverable` | A safe repair exists: run the printed `repair:` commands or `agent-doc autofix <FILE> --apply`. |
| 4 | `operator` | A person must act on the printed `operator:` steps. |
| 5 | `blocked` | Required evidence is missing; gather it with the printed command (for example `--preflight-json` from `agent-doc preflight <FILE> --probe`). |
| 1 | — | The doctor itself failed (unreadable document or capture). |

## compact

```
agent-doc compact <FILE> [--component exchange] [--message TEXT] [--keep N] [--commit]
```

Archive old exchange/component content and rewrite the document atomically. Use `--commit` to close out through the binary-owned `agent-doc commit` path so editor VCS refresh signaling also runs.

## skill

```
agent-doc skill install [--harness HARNESS | --all] [--root DIR] [--reload MODE]
agent-doc skill check
```

Manage the harness skill definition and its shared support bundle.

| Subcommand | Description |
|------------|-------------|
| `install` | Write the bundled skill for the detected or selected harness. Idempotent. |
| `check` | Compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated. |

The skill content is embedded in the binary at build time. After `agent-doc upgrade`, run `agent-doc skill install` in each project to update the skill definition. If an install changes skill content, `--reload restart` prints `SKILL_RELOAD=restart`; other explicit reload modes print `SKILL_RELOAD=compact`. The signal is consistent with `--harness`, `--root`, Grok, and `--all`, and is omitted when the skill was already current.

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

## memory

```
agent-doc memory index <FILE> [--db PATH] [--json]
agent-doc memory search <FILE> --query TEXT [--db PATH] [--limit N] [--json] [--rebuild]
```

Index and search session memory from `agent:backlog`, `agent:review`, `agent:done` (including `archive=` files), `agent:icebox`, and live exchange `### Re:` sections. The default store is `<project>/.tsift/memory.db`, using the shared `tsift-memory` library crate rather than shelling out to the tsift CLI.

## upgrade

```
agent-doc upgrade
agent-doc upgrade --auto
agent-doc upgrade --auto --interval-seconds 300
```

Check GitHub Releases for the latest stable version and upgrade. The prebuilt
archive must match the release checksum; `pip install --upgrade` is the fallback.

`--auto` stays in the foreground, checks immediately, and then watches every 15
minutes (or the requested interval, minimum 60 seconds). It keeps the binary and
only already-installed JetBrains/VS Code-family plugins current. It never installs
a plugin into a new IDE. Transient failures retry on the next poll, and a single
per-user watcher lock prevents duplicate upgraders. Stop it with Ctrl-C; use your
service manager if you want it to run persistently.

## queue add

```bash
agent-doc queue add <FILE> --source <NAME> [--key <KEY>] [--attach <PATH>]... [--json] [TEXT]...
```

Appends an attributed head, `[from:<source>] <text>`, to the document's `agent:run`, so an external tool (an editor plugin, a play-mode bridge, a CI watcher) can hand work to a session. The text comes from the trailing arguments or from stdin. The head is written through the binary-owned queue write path, so it lands in a live editor's buffer instead of conflicting with it on disk. Secrets in the text and attachments are redacted first. Each attachment is stored, redacted, under `.agent-doc/inbound/` and referenced from the head. `--key` dedupes: while a live head from the same source carries the key, a repeat add does nothing. A running session sees the head through mid-turn steering as `dispatch=drain_after_current`. It runs after the current item and does not interrupt it.

```bash
agent-doc queue add plan.md --source ci --key build-1234 --attach build.log "build 1234 failed on main"
```

## Global flags

```
agent-doc --version    # Print version
agent-doc --help       # Show help
```
