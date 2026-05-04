> Extracted from [07-commands.md](07-commands.md)

# Session And Tmux Commands

This file covers the session-bound command surface: pane ownership, routing, sync/reconcile, stash handling, and tmux session selection.

## start

`agent-doc start <FILE> [--force]`

- Starts the configured harness in the current tmux pane and registers the pane as the session owner.
- If another alive pane is already bound to the same document session, normal `start` must fail closed instead of reusing, restarting, or replacing it.
- That failure must print concrete tmux inspection/capture/kill commands so the user can decide which pane to keep and which pane to kill manually.
- `--force` is the only supported escape hatch for intentionally rebinding the current pane during repair work, and the registry rebind must still record supersession provenance in the session log.
- When a fresh start falls back to a new session binding because the configured session is dead, `.agent-doc/config.toml` must be updated to the new live session.
- Harness launches must auto-add writable roots for parent-repo patchback and nested submodule git metadata when needed.

## route

`agent-doc route <FILE> [--pane P] [--debounce MS]`

- Routes a harness-native reopen command into the authoritative pane for the document.
- Ownership proof preference is: canonical path provenance from `sessions.json`, then tmux process-tree file-path proof, then supervisor-PID recovery.
- Route must fail closed on ambiguity and list concrete follow-up commands instead of guessing.
- Routed dispatch must target an actually idle composer. Drafted user input, queue-only Codex composer states, reverse-i-search, permission prompts, or similar blockers are not safe.
- Codex reroutes always send the bare `agent-doc <FILE>` reopen. Multiline payloads or content-edited payloads are invalid.
- If unresolved prompt-bearing drift exists and the pane is busy, route may attempt one scoped `agent-doc fix <FILE>` pass and then one bounded fresh-restart recovery, but it must still fail closed if no clean dispatch path emerges.
- For live same-document panes with no new prompt-bearing drift, route may focus the pane and return success without sending a duplicate reopen.
- Fresh auto-starts and live reroutes both require a real per-document cycle acknowledgment after dispatch; accepted input alone is not sufficient.
- Route auto-start may not create a duplicate hidden fallback pane just because split/join heuristics failed. If the target session already has an `agent-doc` window but no safe registered anchor, or if `split-window` fails beside the chosen anchor, route must fail closed with tmux cleanup commands instead of creating and stashing a second pane.
- Once route has created a fresh pane for a document, that pane stays authoritative for the reroute. A concurrent geometry-only registry rebind must not hand dispatch back to an older same-session pane and make the fresh pane disposable.
- Route must never transiently register an existing pane to a different file just to probe readiness. If a candidate pane is already bound to another document, reroute fails closed instead of emitting a temporary cross-file `session_superseded` / `session_end origin=registry_rebind`.
- Route progress diagnostics must be UTF-8 safe when trimming captured tmux lines for stderr/status output. Prompt/status lines containing Unicode glyphs such as `…` or `·` must never panic the binary during a live reroute.

### Startup-miss tracking

- When route/startup acknowledgment times out, the binary records `.agent-doc/state/startup-miss/<doc-hash>.json` with pane/session provenance and shows a visible diagnostic in tmux.
- On the next route/start/sync path, the tool must distinguish between a stale startup-miss marker and a still-stranded owner. A same-pane marker may only be cleared once newer session-log provenance proves a later open run.
- Successful cycle acknowledgment clears the startup-miss marker.
- `route --dispatch-only` still uses a one-shot bare reopen instead of the managed acceptance/cycle-ack path, but it must reuse the same bounded ready/repair/restart checks before injecting into an existing pane. A bare reopen must not be injected into a still-booting or otherwise busy Codex pane.
- If that first dispatch-only starting-pane probe times out, route must spend one bounded recovery window looking for a newer same-file startup generation or supervisor handoff before it surfaces a `still booting` refusal. A same-file successor pane may be followed; a cross-file rebind must still fail closed.

### Live-child ack rules

- A live `agent-doc` child or healthy supervisor is not itself proof that the rerouted prompt started a new cycle.
- For Codex reroutes, hook-backed submission proof is preferred before the later cycle-start health check.
- Frontmatter-only metadata churn must not count as prompt-bearing work that blocks or requires reroute acknowledgment.

## claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]`

- Claims a document for a tmux pane that is already running the harness.
- The command must enforce the one-live-pane-per-document binding invariant. If the requested pane already belongs to another alive document session, `claim` provisions a new pane instead of commandeering the old one.
- Cross-session claim is invalid unless the configured project session is stale or the user forced the claim explicitly.
- For new template documents, `claim` scaffolds the default `status` and `exchange` components and saves a baseline snapshot with empty exchange content.

## focus

`agent-doc focus <FILE> [--pane P]` focuses the pane that currently owns the document session.

## layout

`agent-doc layout <FILE>... [--split h|v] [--window W]`

- Mirrors editor split layout in tmux by rejoining the wanted panes into a target window and preserving non-session panes.

## resync

`agent-doc resync [FILE] [--fix]`

- Prunes dead panes from the registry, reaps idle stash windows, and reports orphaned windows.
- In scoped mode, it must limit repair to the target document and fail closed if ownership is still ambiguous.
- `--fix` may deregister wrong-process panes, move wrong-window panes into stash, and either kill or relocate wrong-session panes, but only when no stronger live-owner proof keeps the current pane authoritative.
- Stash cleanup must preserve recoverable agent panes that still prove ownership of a registered document or still host a live supervisor socket.

## fix

`agent-doc fix [FILE] [--session <name>]`

- Canonical repair surface; document-scoped form shares the `resync --fix` implementation but limits mutations to the target document.

## sync

`agent-doc sync [--col <FILES>,...] [--window W] [--focus FILE] [--no-autostart]`

- Declaratively mirrors editor layout into tmux columns.
- Files with session ids are managed even when their current registry entry was pruned; `claim` is the only command that creates a new session id.
- Sync must synthesize a per-run tmux-router registry from each visible file's own nearest `.agent-doc` root instead of forcing all files through the caller's current root.
- An alive pane is not reusable solely because the pane id exists; it must still prove live ownership for that specific document.
- When multiple ownership hints disagree, sync must prefer the freshest file-specific proof in this order: path/supervisor provenance, then the latest open session-log owner, then the latest alive `registry_rebind` successor pane, and only then generic same-file process-tree matches.
- When ownership proof weakens but the alive pane still contains protected Codex drafted input or still appears as the newest open pane in the session log, sync must fail closed for that file instead of fabricating `registered_pane_missing`.
- If two visible files point at the same pane, sync must either find one decisive owner or drop the duplicate from the synthetic registry so tmux-router cannot alias both files onto one pane.
- Once a live pane is reserved for one file during the pass, later files in the same pass must treat it as unavailable.
- If a registered pane is stashed, sync must rescue it back into the visible `agent-doc` window rather than treating the stash copy as disposable.
- Before replacing a missing pane, sync must first attempt closeout recovery for `response_captured` or `write_applied` cycles. If that recovery fails, sync must fail closed and preserve the durable capture instead of provisioning another pane.
- Ordinary sync/preflight/finalize recovery paths must never kill a tmux pane. When sync observes a dead pane during missing-pane repair, it may capture diagnostics and keep the dead pane retained for manual inspection, but only explicit repair surfaces such as `fix` / `resync --fix` may escalate to pane-kill cleanup.
- Recent repeated `missing_pane` recoveries, unresolved startup-miss state, or a `registry_rebind` closeout whose recorded successor pane is still alive and rooted to the same document all block passive `--no-autostart` cold-start.
- If any visible file stays blocked under passive `--no-autostart`, sync must preserve the current visible tmux layout and warn instead of reconciling the remaining foreign pane set into a new authoritative layout.

### Sync-specific invariants

- `provision_pane` is the sync-specific pane-creation path. It chooses split direction by column position and does not block on prompt readiness.
- When sync creates new panes it should prefer splitting in the visible `agent-doc` window, not beside a stash pane when a visible anchor exists.
- Post-sync registration must fail closed if one pane would be mirrored back into the registry for multiple documents.
- Cross-session stash rescue is intentionally non-destructive: if a live stashed pane belongs to another tmux session, preserve it in place and report the mismatch instead of moving or killing it.

## repair_layout

`repair_layout` runs before sync and again after tmux-router reconciliation.

- It consolidates duplicate stash windows, recreates the `agent-doc` window from stash when needed, and normalizes window indices so `agent-doc` remains window `0` with stash windows immediately after it.

## session

`agent-doc session` shows the configured project tmux session.

`agent-doc session set <name>` updates config and migrates the `agent-doc` and `stash` windows when possible.

### Shared session resolution

The session-targeting precedence is shared across start/route/sync/session-aware helpers:

1. Explicit non-empty context session from `sync --window`
2. Live project `.agent-doc/config.toml` `tmux_session`
3. Current tmux session
4. Harness fallback only for start/route when no live tmux session exists

Windowless sync must prefer the live project pin over the caller's currently attached tmux session.
