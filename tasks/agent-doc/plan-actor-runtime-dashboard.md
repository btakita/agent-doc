# Plan: actor runtime dashboard (live view over the IPC admin API)

**Pending:** `#actor-runtime-dashboard` (queue id `#admin-fleet-dashboard`)
**Status:** Phase 1 (read-only view) IMPLEMENTED 2026-06-02 — `agent-doc admin dashboard`
in `agent-doc-orchestration::dashboard`. Model/view split, highlight flags, and
render are unit-tested. Mutation keybinds (stop/rebind rows) remain deferred to
`#ipc-admin-api` Phase 2 (gated mutating verbs); the editor tool-window surface
is also still deferred.
**Class:** `#agent-doc-bug` feature — operator observability.

## Motivation

`#ipc-admin-api` adds the data plane (`agent-doc admin list/detect/rebind/stop`).
This item adds the **view**: a live dashboard of the current runtime actor fleet,
so an operator can *see* competing / cross-document actors at a glance instead of
running `admin list` repeatedly or doing manual `ps`/`pstree` forensics (the gap
exposed by the `#xdoc-route-sweep-commits-sibling-docs` incident).

## Goal

A live dashboard rendering the `#ipc-admin-api` `admin list` / `admin detect`
output, refreshing on an interval (and/or on supervisor IPC events), showing
every live agent-doc actor and flagging the problematic ones.

## Surface (proposed)

- **`agent-doc admin dashboard`** — a terminal TUI (one row per actor):
  `document` · `pane` · `harness` · `generation` · `state`
  (idle/busy/waiting) · `queue_active` · `supervisor_pid`/`child_pid` ·
  health, with competing / cross-document / orphaned / respawn-looping actors
  highlighted (from `admin detect`). Auto-refresh (default ~1s; also redraw on
  supervisor IPC event when available). Read-only by default; optional keybinds
  to drive `admin stop` / `admin rebind` on the selected row (fail-closed,
  confirm prompt; never the caller's own actor without `--force`).
- **Editor surface** — a JetBrains / VS Code tool-window panel rendering the same
  `admin list`/`detect` feed via the shared FFI (thin event-reporter; the panel
  shells the CLI/FFI, no logic in the plugin). Pairs with
  `#plugin-cleanup-menu-command`.

## Design notes

- **Pure view over `#ipc-admin-api`.** No new detection/control logic here — the
  dashboard consumes `admin list`/`admin detect` JSON. If a field is missing for
  the view, add it to `admin list` (in `#ipc-admin-api`), not here.
- **Refresh cheaply.** `admin list`/`detect` must stay read-only and cheap
  (registry + session logs + ps ancestry); the dashboard polls on an interval and
  optionally subscribes to supervisor IPC events for event-driven redraw.
- **Mutations are explicit + fail-closed.** Stop/rebind keybinds reuse the
  `#ipc-admin-api` verbs with their existing confirm / `--force` guards.

## Tests

- Render fixture: a fixed `admin list` JSON → deterministic dashboard model
  (rows, highlight flags) without a live terminal (model/view split).
- Detect-highlight: a fixture with one cross-document actor → that row is flagged.
- Mutation keybind dispatches the correct `admin stop`/`rebind` command and
  respects the self-actor fail-closed guard.

## Acceptance

- `agent-doc admin dashboard` shows the live actor fleet and highlights competing
  / cross-document / respawn-looping actors in real time.
- An operator can spot and (optionally) stop a runaway cross-document actor from
  the dashboard without manual `ps`/`pstree`.

## Related

- `#ipc-admin-api` (the data + control plane this renders — hard dependency).
- `#xdoc-route-sweep-commits-sibling-docs` (the contamination this makes visible).
- `#plugin-cleanup-menu-command` (editor surface; same thin-shell pattern).
