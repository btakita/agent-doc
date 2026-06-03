# Plan: IPC admin API for actor detection / rebind / termination

**Pending:** `#ipc-admin-api`
**Status:** Phase 1 (read-only `admin list` + `admin detect`) IMPLEMENTED + tested + shipped 2026-06-02 (submodule `31efa50c`, both binaries rebuilt + installed). Phase 2 (mutating verbs `rebind` / `stop`) and Phase 3 (FFI + editor menu surface) remain.
**Class:** `#agent-doc-bug` feature — operational control plane.

## Phase status

- [x] **Phase 1 — read-only control plane** (`31efa50c`): `agent-doc admin list [--json]` enumerates the fleet from the sqlite actor store (session/pane/window/supervisor-pid/harness/generation/state/liveness); `agent-doc admin detect [--json]` derives `cross_document_pane` (a *live* pane bound to >1 non-closed document) and `stale_dead_pane` (non-closed actor on a dead pane). Logic is pure + unit-tested in `agent-doc-orchestration/src/admin.rs` (6 tests); verified live (surfaced a real cross-binding + 19 orphaned bindings in the agent-loop fleet). This delivers the "detect competing actors without manual ps/pstree" acceptance criterion.
- [ ] **Phase 2 — mutating verbs** (`rebind`, `stop --halt-queue --terminate`): need supervisor-IPC quiesce + durable `queue_active:false` + live tmux/process verification; the stop verb must compose with the queue loop so the stop sticks (no respawn). Fail-closed on the caller's own session.
- [ ] **Phase 3 — FFI + editor surface**: thin JetBrains/VS Code menu actions shelling the CLI/FFI (pairs with `#plugin-cleanup-menu-command`).

## Motivation

The 2026-06-02 cross-document contamination incident
(`#xdoc-route-sweep-commits-sibling-docs`) required **manual process
forensics** to resolve: `ps`/`pstree`/`pgrep` to map codex/claude PIDs to tmux
panes, `.agent-doc/sessions.json` + `agent-doc session status` to find the
authoritative owner, then a raw `kill` of the supervisor + child tree — which
**did not stick** because `queue_active:true` + the route/supervisor
re-provisioned the actor on a fresh pane.

agent-doc already exposes per-document `session status`, but there is no
**fleet-wide control plane**: no single call to enumerate every live actor, ask
"which actor is touching document X", detect a sibling actor committing a foreign
document, rebind an actor to its correct authoritative pane, or stop/terminate a
runaway actor (with its queue loop) cleanly.

## Goal

Add an **admin API to all agent-doc IPC processes** (the per-project supervisor
sockets under `.agent-doc/supervisor/<session>.sock` and the route/registry
layer) exposing three capabilities, reachable from the CLI (`agent-doc admin
...`), the FFI layer, and the editor plugins (thin event-reporters):

1. **Detect** — enumerate live actors across the project fleet and flag
   competing / cross-document ones.
2. **Rebind** — reassign a document to its correct authoritative pane/actor, or
   move a misbound actor, without a manual kill+reroute.
3. **Stop / terminate** — halt an actor's queue loop and terminate it so the
   stop is durable (no supervised respawn).

## API surface (proposed)

- `agent-doc admin list [--json]` — every live actor: `session_id`, `document`,
  `pane`, `window`, `supervisor_pid`, `child_pid`, `harness`, `generation`,
  `state` (idle/busy/waiting), `queue_active`, `cwd`. One row per actor.
- `agent-doc admin detect [--json]` — derived diagnostics: actors whose live
  `cwd`/recent commits touch a document they do not authoritatively own
  (cross-document authorship), duplicate owners for one document, orphaned
  supervisors, respawn-looping sessions. Reuses the
  `log_cross_document_execution_context` signal.
- `agent-doc admin rebind <document> [--to-pane <pane> | --to-session <id>]` —
  set/repair the authoritative actor binding for a document (registry + lease +
  live pane select), the programmatic form of the manual claim/reroute.
- `agent-doc admin stop <document|--session <id>|--pane <pane>> [--halt-queue]
  [--terminate]` — durable stop: `--halt-queue` flips `queue_active:false` first
  so the loop cannot continue, then signals the supervisor to quiesce; with
  `--terminate`, also kills the child + supervisor tree and marks the session
  closed so route/sync will not re-provision. Fail-closed: refuse to stop the
  caller's own actor unless `--force`.

## Design notes

- **Control plane lives in the supervisor IPC, not the plugins.** Per the
  Shared-Foundation rule, the detect/rebind/stop logic is FFI + supervisor-socket
  owned; plugins (JetBrains "Resync / Fix Sessions", VS Code) shell the CLI/FFI.
  Pairs with `#plugin-cleanup-menu-command`.
- **Stop must compose with the queue loop.** Killing a process is insufficient
  (see the respawn note in `#xdoc-route-sweep-commits-sibling-docs`); `admin stop
  --halt-queue --terminate` must (a) set `queue_active:false` durably, (b)
  quiesce the supervisor so it does not restart the child, (c) terminate, (d)
  leave the pane idle. Confirmable via `admin list` showing the actor gone.
- **Detect should be cheap + safe.** Read-only enumeration from the registry +
  session logs + `ps` ancestry, no mutation. The mutating verbs (rebind/stop)
  are explicit and fail-closed on the caller's own session.
- **Ownership identification reuses existing primitives:** registry `pane`,
  `controller_lease`, `supervisor_process` from `session status`, and the
  `cmdline_owns_other_document` / cross-document execution-context detection.

## Tests

- `admin list` fixture: 3 registered docs on distinct panes → 3 rows with correct
  owner/pane/state.
- `admin detect`: a session whose recent commit authored a foreign document is
  flagged cross-document; a clean single-owner fleet reports none.
- `admin stop --halt-queue --terminate` (SimWorld): asserts `queue_active` flips
  false, supervisor receives quiesce, child marked closed, and a follow-up
  `route`/`sync` does NOT re-provision the actor.
- `admin stop` refuses the caller's own session without `--force`.

## Acceptance

- An operator can run `agent-doc admin detect` to see the competing-actor
  situation that previously required manual `ps`/`pstree`.
- `agent-doc admin stop <doc> --halt-queue --terminate` durably stops a runaway
  cross-document actor in one command (no whack-a-mole).
- `agent-doc admin rebind` repairs a misbound document without manual claim.
- Editors expose these as menu actions via the shared FFI/CLI.

## Related

- `#xdoc-route-sweep-commits-sibling-docs` (the contamination this would let you
  detect + stop).
- `#plugin-cleanup-menu-command` (editor surface for resync/gc — same thin-shell
  pattern).
- `#clear-stops-running-process` (stop-running-child overlap).
