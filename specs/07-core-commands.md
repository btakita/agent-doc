> Extracted from [07-commands.md](07-commands.md)

# Core Commands

This file covers the lower-churn command surface that is not primarily about tmux/session routing, response closeout, or orchestration.

## run

`agent-doc [run] <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

- `agent-doc <FILE>` and `agent-doc run <FILE>` are equivalent.
- `run` computes the diff, resolves document mode from frontmatter, sends the prompt to the configured backend, durably captures the final parsed response, applies the response through the matching write path, updates the resume/session id, records `write_applied`, and then runs the same strict closeout helper used by `finalize`.
- After its pre-commit repair step, `run` rechecks the diff before child-agent dispatch. If the repair consumed the whole diff because the response patchback was already committed and no new assistant response body was supplied, `run` must fail before invoking the configured backend and point the operator to `agent-doc write --commit <FILE>`.
- After opening the response `preflight_started` cycle, `run` emits parent-visible heartbeat stderr during long child-agent waits every `AGENT_DOC_RUN_HEARTBEAT_SECS` seconds, defaulting to 30. Each heartbeat preserves the open phase while updating the cycle state's `updated_at` and `last_event` with the current phase, elapsed time, timeout budget, and agent name.
- If the pending diff contains executable directives such as `do #id`, `run tests`, `build + install`, `commit + push`, `go`, or imperative pending-item prose, status-only or meta-only agent replies are invalid. The response must contain either concrete execution evidence or a concrete blocker.
- If the diff contains a bare `compact exchange` request, `run` must fail closed and direct the caller to `agent-doc compact <FILE> --commit`.
- Once a cycle records `committed`, later repair bookkeeping must not rewind the persisted cycle state to `response_captured` or `write_applied`.

## init

Two modes:

- `agent-doc init` initializes the project-level `.agent-doc/` directories and installs bundled skill content.
- `agent-doc init <FILE> [TITLE] [--agent NAME]` scaffolds a new session document and lazily runs project init first when needed.

## install

`agent-doc install [--editor jetbrains|vscode] [--skip-prereqs] [--skip-plugins]`

- Verifies `tmux` and the configured agent CLI are present unless skipped.
- Installs editor plugins either for the requested editor or for auto-detected editors.
- Local source installs inside the `agent-loop` workspace must resolve sibling crates without ad hoc Cargo patch flags.

## diff

`agent-doc diff <FILE>` prints the unified diff between the saved snapshot and the current document.

## response-toc

`agent-doc response-toc <FILE> [--id BACKLOG_ID] [--query TEXT] [--limit N] [--json]`

- Lists lightweight locators for current live `### Re:` sections plus matching archived response sections for the same document.
- `--id` accepts either `restoc` or `#restoc` and filters both live and archived entries.
- `--query` matches normalized heading/body text.
- Output locators are stable enough for follow-up `response-fetch` calls, for example `live:3` or `archive:.agent-doc/archives/hash.md#2`.

## response-fetch

`agent-doc response-fetch <FILE> --locator LOCATOR [--before N] [--after N] [--json]`

- Loads the exact live or archived response section referenced by a `response-toc` locator.
- `--before` / `--after` include adjacent response sections from the same source so agents can pull bounded neighboring context on demand instead of rereading whole exchanges or archives.
- Archive fetches read from the derived archive index; callers do not need to open sqlite directly.

## archive-index

`agent-doc archive-index <FILE> [--rebuild]`

- Builds or refreshes the derived sqlite compacted-turn index at `.agent-doc/archive-index.db`.
- The index is rebuildable from `.agent-doc/archives/*.md`; archive markdown remains the canonical history artifact.
- `--rebuild` drops all derived rows and recreates them from the archive corpus.

## archive-search

`agent-doc archive-search <FILE> [--query TEXT] [--id BACKLOG_ID] [--session SESSION_ID] [--limit N] [--json] [--rebuild]`

- Queries indexed compacted-turn chunks rather than rereading archive markdown manually.
- Results are ranked to prefer the current document, exact `#id` matches, and recent archives.
- `--id` accepts either `sqlarcidx` or `#sqlarcidx`.
- `--rebuild` refreshes the derived index before search.

## reset

`agent-doc reset <FILE>` clears the saved session id and deletes the snapshot for the document.

## clean

`agent-doc clean <FILE>` squashes all `agent-doc:` commits for the file into one via `git reset --soft`.

## gc

`agent-doc gc [--root DIR] [--dry-run]`

- Garbage-collects orphaned snapshots, captures, locks, hooks, status files, repair diagnostics, Codex blocked-stop diagnostics, sockets, and dead registry entries under `.agent-doc/`.
- The orphaned-socket cleanup keeps sockets whose supervisor PID is alive or whose socket still answers.
- Stale `starting` actor records older than one hour are closed unless a live supervisor PID still has a fresh supervisor heartbeat proving the actor is booting; this updates the controller SQLite store and re-emits `session-actors.json` as a projection. A live PID with a stale heartbeat is treated as stuck startup state.
- `preflight` runs the full orphan-file GC automatically at most once per day via `.agent-doc/gc.stamp`; `preflight`, `start`, and `sync` still run the lightweight stale-`starting` actor cleanup every cycle.

## audit-docs

`agent-doc audit-docs [--root DIR]`

- Audits instruction files such as `CLAUDE.md`, `AGENTS.md`, `README.md`, and `SKILL.md` for path accuracy, staleness, actionable content, and line budget.
- Discovery prunes heavy skip directories before descent so audit time is spent on real instruction surfaces.
- Generated agent-doc instruction surfaces are audited as release artifacts: if a root `AGENTS.md`, `.codex/AGENTS.md`, `.opencode/skills/agent-doc/SKILL.md`, or `.claude/skills/agent-doc/SKILL.md` still carries the agent-doc managed frontmatter/sections, it must match the content rendered by the running binary. Custom root instruction files that do not look agent-doc-managed remain user-owned and are not rewritten or failed for content mismatch.

## prompt

`agent-doc prompt <FILE>`

- Detects active permission prompts from Claude Code and OpenCode panes by scanning the captured pane footer.
- Supports Claude Code bracketed legacy options, Claude Code numbered-list options, and OpenCode horizontal `Allow once` / `Allow always` / `Reject` permission rows.
- `prompt --answer` uses Claude Code's vertical Up/Down movement for Claude prompts and OpenCode's horizontal Left/Right movement for OpenCode permission prompts. The OpenCode supervisor path preserves Kitty keyboard-mode negotiation so arrow keys and tab-style selection keys pass through to OpenTUI instead of leaking as literal escape text. Selecting OpenCode `Allow always` also sends the follow-up confirmation Enter because OpenCode opens a second `Always allow` confirmation prompt before persisting that choice.
- `--answer N` selects an option using the prompt's navigation contract and presses Enter.
- `--all` polls every live session.

## skill

`agent-doc skill install` writes the bundled skill into the current project, and `agent-doc skill check` compares the installed copy to the bundled version.

- The installed skill always renders `agent-doc-version` from the running binary version.
- Harness-specific reload flows must use explicit `--harness` selection rather than environment guessing.
- Harness installs refresh a managed root `AGENTS.md` mirror when it still looks generated, so `.codex/AGENTS.md` and the root mirror cannot drift across `agent-doc-version` bumps. Custom root `AGENTS.md` files are opt-in and must be preserved.
- Generated Claude, Codex, and generic hot-path instruction surfaces must stay compact: the shared source template is budgeted at 140 lines, and rendered harness-specific surfaces are budgeted at 150 lines. Rare recovery detail belongs in bundled runbooks rather than the always-loaded skill body.

## outline

`agent-doc outline <FILE> [--json]` reports markdown heading structure, line counts, and approximate token counts.

## upgrade

`agent-doc upgrade` checks crates.io for a newer release and upgrades through the GitHub Release / `cargo install` / `pip` cascade.

The runtime version warning cache lives at `~/.cache/agent-doc/version-cache.json`.

## plugin

`agent-doc plugin install|update|list <EDITOR>`

- Supports JetBrains and VS Code.
- Pulls assets from GitHub Releases, preferring signed assets when available.

## rename

`agent-doc rename <OLD_PATH> <NEW_PATH>`

- Migrates hash-keyed state files such as snapshots, baselines, locks, pending state, CRDT state, and pre-response artifacts to the new path hash.
- Auto-migration through `ensure_initialized` still handles the common rename path; `rename` remains the explicit fallback.

## watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]`

- Watches registered session files and re-submits them when they change.
- CRDT/reactive documents use zero debounce.
- Busy documents are skipped so the watch daemon cannot race the live write path.

## history

`agent-doc history <FILE>` lists exchange history from git.

`agent-doc history <FILE> --restore <COMMIT>` prepends a historical exchange back into the current exchange component.

## transfer

`agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ...] [--referral]`

- Full transfer moves an entire component, optionally carrying backlog and icebox context too.
- Selective `--items` transfer operates on backlog/icebox parent items keyed by `[#id]` and moves the full tracked block, including indented continuation lines.
- `--bypass-claim` is the explicit cross-pane override.
- `--referral` leaves the source content in place and inserts a structured pointer in the target instead of moving content.

## extract

`agent-doc extract <SOURCE> <TARGET> [--component NAME]`

- Moves the last exchange entry from the source into the target's matching component and preserves both documents' snapshots.

## backlog

`agent-doc backlog <FILE> <ACTION>`

- Canonical surface for tracked work. `agent-doc pending` remains a deprecated alias only.
- Supports add/edit/done/reorder/prune/list/gate operations against the canonical `agent:backlog` component.
- Non-item separator lines and headings inside backlog/icebox must be preserved during mutation.
- Flush-left parent items are the tracked units; indented nested lists travel with the parent during edit/reorder/reap/transfer.

## boundary

`agent-doc boundary <FILE> [COMPONENT]`

- Inserts a transient `agent:boundary` marker into the working-tree document and signals the editor so the next IPC write can use a current insertion point.
- It must not update the saved snapshot, stage files, or create a git commit. The marker is setup state, not a response closeout boundary.
- A later preflight/commit may normalize marker-only working-tree churn as already committed, but standalone boundary insertion must never become the snapshot basis for a boundary-only commit.

## terminal

`agent-doc terminal <FILE> [--session NAME]`

- Opens an external terminal that attaches to the target tmux session, but only when another attached client does not already exist.
- The terminal command comes from user config or `$TERMINAL`.

## migrate

`agent-doc migrate [FILES...] [--all] [--dry-run]`

- Migrates deprecated `agent:pending` markers to the canonical `agent:backlog` markers and strips deprecated backlog tag attributes.
- Skips fenced code blocks and inline code.

## dedupe

`agent-doc dedupe <FILE>`

- Removes consecutive duplicate `### Re:` response blocks and updates the snapshot.
- Also deletes the stale queued patch file so a plugin restart cannot replay the removed duplicate.
- The normal template write/finalize path runs the same consecutive-response dedupe before saving snapshots, CRDT state, disk content, or sidecar-normalization full-content repair payloads. `session-check` fails closed if a duplicate survives closeout instead of reporting success.
- Stream IPC timeout closeout also removes the queued fallback patch after its local write and commit succeed, so `dedupe` is not required as a second cleanup commit for that timeout shape.
