# Sync Command Route Error Follow-up

## Evidence

- `.agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs5.md.txt` still exists and reports:
  - `resync: stash pane ... is registered in its own project root -- skipping kill`
  - `[sync] recovered live pane %273 for session tsift-v0 via supervisor pid 2979498`
  - `[sync] recovered live pane %268 for session b26b9957 via supervisor pid 3081113`
  - `[route] target tmux session: 4`
  - `Error: project controller returned ok response without data`
- Recent `.agent-doc/logs/ops.log` entries show `agent-doc-bugs2.md` dispatch succeeding through the controller, including `controller_dispatch_accepted`, `route_dispatch_start_proven`, and `route_dispatch_only_sent`.
- Recent safe-passive sync entries are mostly healthy: `sync_lock_wait=0ms`, `prune_stash_panes=0ms`, and `safe_passive_total` around 725-839ms. The older over-budget records still exist, but the current failure surface is not primarily slow stash pruning.
- There are still intermittent `route_dispatch_only_starting_actor_direct_pane_submit` records followed by immediate `controller_dispatch_accepted ... state=ready`, which means route can act on a stale `starting` projection just before controller state catches up.

## Hypothesis

The sync command is recovering the right panes, but the route/focus path can still surface stale or misleading editor diagnostics when a controller RPC returns an envelope shape that the caller does not expect. The saved editor error is also not being cleared aggressively enough after later successful sync/route activity, so the editor can keep presenting an obsolete failure.

## Plan

1. Reproduce the `ok response without data` path with a focused controller-client test:
   - exercise each typed `request_controller<T>` caller used by route, focus, sync, and operator commands;
   - assert that every successful typed controller command returns `data`;
   - assert that command types intentionally returning no data are not called through `request_controller<T>`.
2. Improve the diagnostic:
   - include the controller command name and raw response envelope in the error context;
   - log the failing command to `ops.log` before returning to the editor;
   - preserve the original route output in `.agent-doc/state/editor-route-errors/`.
3. Fix stale editor-route-error cleanup:
   - clear a document's saved editor route error after a later successful `focus`, `sync`, `route_dispatch_start_proven`, or `route_dispatch_only_sent` for the same document;
   - add a regression for the existing `agent-doc-bugs5.md` shape where sync recovers panes before route fails.
4. Tighten the stale-starting projection boundary:
   - when route sees `actor_state=starting`, re-query the controller immediately before deciding to submit without a ready probe;
   - if the controller now reports `ready`, route through the ready path and avoid logging the starting fallback.
5. Keep latency regressions separate:
   - do not reopen broad stash-prune work unless new logs again show `prune_stash_panes` or `sync_lock_wait` over budget;
   - retain a small assertion that normal safe-passive sync records phase timings and clears stale diagnostics.

## Verification

- Add Rust unit coverage around controller response envelopes and route/focus error cleanup.
- Add or update editor-side test coverage that a saved `.agent-doc/state/editor-route-errors/<doc>.txt` file is removed after a later successful command for that same document.
- Run `cargo test -p agent-doc`.
- Run the project check target if code paths touch sync, route, or controller behavior.
