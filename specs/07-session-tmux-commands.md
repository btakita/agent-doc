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
- When `.agent-doc/session-actors.json` has a healthy authoritative record for the document, route must treat that actor generation as the owner-of-record and dispatch through supervisor IPC instead of re-electing a pane from tmux/process heuristics.
- Normal-path ownership proof is the authoritative actor record first, then the supervisor-backed registered binding from `sessions.json`.
- Existing managed reroutes must use supervisor IPC for the reopen path; they must not fall back to typing directly into a live Claude/Codex pane.
- Actor-backed reroutes may refresh `sessions.json` as a projection of the actor pane, but they must not opportunistically steal another same-file pane or re-register to a heuristic winner while the authoritative actor is healthy.
- Session-log owners, `registry_rebind` successors, and generic same-file process-tree matches are repair/diagnostic signals only; route must fail closed with explicit inspect/claim/kill guidance instead of promoting them back to authority on the normal path.
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
- `route --dispatch-only` still uses a one-shot bare reopen instead of the managed acceptance/cycle-ack path, but for any existing live managed session that one-shot reopen must submit directly through the resolved pane's tmux input path instead of routing back through supervisor IPC or writing raw bytes into the child PTY. That live-pane submit must use the same literal-text submit followed by a short delayed `Enter` tmux boundary as file-scoped `session clear`. It must reuse the same bounded repair/restart checks before injecting into an existing pane, but it must not impose an extra direct-pane "dispatch-ready prompt" gate that file-scoped clear does not have. A tracked Codex `/clear` may still force a fresh restart on the managed non-dispatch route, but dispatch-only editor reroutes must keep sending the bare reopen into the live session after `session clear`.
- When a prompt-bearing `route --dispatch-only` targets an authoritative actor pane whose runtime still reports `starting` or `busy`, dispatch-only must keep using that same live-pane submit path instead of queueing the reopen through supervisor IPC or surfacing a second boot-window refusal. The goal is to keep `Run Agent Doc` on the same tmux delivery boundary as the working file-scoped `session clear` path even during the short post-clear / post-restart window.
- When `route --dispatch-only` has a live authoritative actor pane but the supervisor socket no longer reports a healthy runtime or actor state, route must log the degraded authoritative decision and may still submit directly through that same authoritative pane when the pane matches the current registered/live owner binding. That fallback exists to keep editor reroutes aligned with the already-working file-scoped `session clear` direct-pane path instead of silently dropping to stale registry heuristics.
- File-scoped `agent-doc session clear <FILE>` must resolve a direct-pane submit target in the same order that routed reopen prefers an existing live session binding: authoritative actor pane first, then a current live-owner pane for the same file, then the registry pane. Only when none of those panes is directly addressable on the default tmux server may clear fall back to supervisor IPC inject.
- File-scoped `agent-doc session clear <FILE>` and `route --dispatch-only <FILE>` must each record their delivery branch in ops-log, including whether the command crossed the live pane's tmux submit boundary or supervisor IPC inject.
- Supervisor-owned inject coverage must include at least one real socket-backed tmux regression, not only mocked writer tests, so the live IPC listener proves it can hand submitted text off to a real pane through tmux.
- If dispatch-only has to fall back to supervisor IPC for a starting-pane reroute, route must still spend one bounded recovery window looking for a newer same-file startup generation or supervisor handoff before it surfaces a `still booting` refusal. A same-file successor pane may be followed; a cross-file rebind must still fail closed. The direct live-pane submit path does not wait on a separate ready-probe gate.

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

- When `.agent-doc/session-actors.json` has a live authoritative record for the
  document session, focus must select that actor-owned pane even if
  `sessions.json` still points at an older projection.
- Focus may use `sessions.json` only as a fallback binding helper when no live
  authoritative actor pane exists.

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
- When `.agent-doc/session-actors.json` has a live authoritative record for a
  visible document, sync must treat that actor-owned pane as the owner-of-record
  and refresh `sessions.json` only as a projection of that binding.
- An alive pane is not reusable solely because the pane id exists; normal sync may reuse only the authoritative actor pane or the supervisor-backed registered binding for that specific document.
- When ownership falls back to legacy associated-pane evidence (`session-log`, `registry_rebind`, generic same-file process tree), sync must fail closed and require explicit claim/repair instead of choosing a winner automatically.
- When ownership proof weakens but the alive pane still contains protected Codex drafted input or still appears as the newest open pane in the session log, sync must fail closed for that file instead of fabricating `registered_pane_missing`.
- If two visible files point at the same pane, sync must either find one decisive owner or drop the duplicate from the synthetic registry so tmux-router cannot alias both files onto one pane.
- Once a live pane is reserved for one file during the pass, later files in the same pass must treat it as unavailable.
- If a registered pane is stashed, sync must rescue it back into the visible `agent-doc` window rather than treating the stash copy as disposable.
- If an editor supplies `--window W`, `W` must already be an `agent-doc` window for the target tmux session. When the named session has no visible `agent-doc` window, normal sync must fail closed and preserve layout instead of reconciling remembered docs onto an arbitrary non-`agent-doc` window.
- Post-sync registry updates must fail closed if tmux-router reports a
  geometry-only pane assignment that disagrees with a still-live authoritative
  actor pane for that document.
- When a missing pane coincides with an open `preflight_started`,
  `response_captured`, or `write_applied` cycle, normal sync must fail closed
  and preserve the current tmux layout until an explicit repair surface closes
  that state. `agent-doc repair <FILE>` and `agent-doc session doctor <FILE>
  --repair` own that recovery boundary.
- Ordinary sync/preflight/finalize recovery paths must never kill a tmux pane. When sync observes a dead pane during missing-pane repair, it may capture diagnostics and keep the dead pane retained for manual inspection, but only explicit repair surfaces such as `fix` / `resync --fix` may escalate to pane-kill cleanup.
- Recent repeated `missing_pane` recoveries, unresolved startup-miss state, or a `registry_rebind` closeout whose recorded successor pane is still alive and rooted to the same document all block passive `--no-autostart` cold-start.
- If any visible file stays blocked under passive `--no-autostart`, sync must preserve the current visible tmux layout and warn instead of reconciling the remaining foreign pane set into a new authoritative layout.

### Sync-specific invariants

- `provision_pane` is the sync-specific pane-creation path. It chooses split direction by column position and does not block on prompt readiness.
- When sync creates new panes it should prefer splitting in the visible `agent-doc` window, not beside a stash pane when a visible anchor exists.
- Post-sync registration must fail closed if one pane would be mirrored back into the registry for multiple documents.
- Cross-session stash rescue is intentionally non-destructive: if a live stashed pane belongs to another tmux session, preserve it in place and report the mismatch instead of moving or killing it.
- Retained-dead pane cleanup regressions must drive the pane from a confirmed idle shell state before sending the exit command; split-pane shell startup is asynchronous under parallel tmux test load, so verification must fail closed on an unready shell instead of assuming the pane already accepted input.

## repair_layout

`repair_layout` is an explicit repair primitive, not a normal sync side effect.

- `agent-doc session doctor <FILE> --repair` and other repair-oriented commands
  may call it to consolidate duplicate stash windows, recreate the `agent-doc`
  window from stash when needed, and normalize window indices so `agent-doc`
  remains window `0` with stash windows immediately after it.
- Ordinary `agent-doc sync` resolves the target session/window without invoking
  `repair_layout`; if stash/window drift is detected, sync warns and leaves the
  destructive or heuristic layout repair for an explicit repair command.

## session

`agent-doc session` shows the configured project tmux session.

`agent-doc session set <name>` updates config and migrates the `agent-doc` and `stash` windows when possible.

`agent-doc session clear` with no file still clears the configured tmux-session
pin and returns the project to auto-detect mode.

### Actor session operator commands

The same `session` namespace now also exposes the operator-facing
single-owner actor controls:

- `agent-doc session status <FILE>` prints the authoritative actor record,
  registry projection, supervisor runtime state, startup-miss marker, and
  latest session-log summary for the document.
- `agent-doc session history <FILE>` prints the actor/session transition
  history from `.agent-doc/logs/<session>.log`, filtered to ownership and
  lifecycle boundary events.
- `agent-doc session attach <FILE> --pane %123` performs an explicit
  authoritative handoff onto the requested pane, creating a new generation and
  refreshing the registry projection from that result.
- `agent-doc session restart <FILE> [--fresh]` requests an actor-owned
  supervisor restart through IPC instead of relying on route-side restart
  heuristics.
- `agent-doc session clear <FILE>` injects the harness-native `/clear`
  equivalent into the authoritative session through the same canonical
  single-line submit command used by routed reopen and queued slash-command
  dispatch. When the authoritative pane is alive on the default tmux server,
  the command must submit directly through that pane's tmux input path;
  otherwise it may fall back to supervisor IPC inject. For Codex, it still
  records the clear prompt state so the next reroute can reapply the original
  launch contract.
- Any tmux-bound command submit in this surface (`route --dispatch-only`,
  file-scoped `session clear`, queued slash-command dispatch, supervisor-owned
  reopen inject) must normalize trailing line endings once and use exactly one
  tmux literal-text submit plus short delayed `Enter` submission. These paths must not layer
  follow-up synthetic `Enter` retries on top of the first submit.
- `agent-doc session doctor <FILE> [--repair]` reports actor/registry/supervisor
  drift in one read-only summary, with `--repair` explicitly escalating into the
  destructive repair path before re-checking status.

### Shared session resolution

The session-targeting precedence is shared across start/route/sync/session-aware helpers:

1. Explicit non-empty context session from `sync --window`
2. Live project `.agent-doc/config.toml` `tmux_session`
3. Current tmux session
4. Harness fallback only for start/route when no live tmux session exists

Windowless sync must prefer the live project pin over the caller's currently attached tmux session.
