---
name: reference_agent_doc_start_crash_cross_root_submodule_autostart
description: "agent-doc start monsterrodholders.md crash root cause: superproject sync auto-started the src/boost-client submodule copy from the wrong root"
metadata: 
  node_type: memory
  type: reference
  originSessionId: e30e4f1b-d9b0-4bbe-ab1a-b9b778b89362
---

Two `monsterrodholders.md` exist in agent-loop: `tasks/research/monsterrodholders.md`
(superproject) and `src/boost-client/tasks/monsterrodholders.md` (a submodule with
its OWN `.agent-doc/` root). The reported "agent-doc start monsterrodholders.md
crashing on start" was the superproject `sync` restoring the **submodule** copy from
remembered layout memory (`.agent-doc/state.db` `layout_states` row + `last_layout.json`
both listed the cross-root path) and auto-starting it from the wrong root → slow pane
(`ownership_proof ~7980ms` vs 750ms budget) that dies on start.

Fixed in binary (submodule commit `447e6c53`, tag `#sync-cross-root-autostart`):
`sync.rs` now sanitizes remembered layout at load and guards the auto-start loop so a
document owned by a different (nested/submodule) `.agent-doc` root is never provisioned
by the superproject — its own controller starts it. Also hardened
`agent-doc-start-io` `&session_id[..8]` → char-safe `session_id_short` (a short/non-UUID
`session:` value would panic the pane-spawned `agent-doc start` as a raw backtrace).
The fix self-heals polluted layout memory on next full sync.

Debug tip: when a `start` "crash" is really slow/dead pane creation on editor focus,
check `layout_states` / `last_layout.json` for cross-root (submodule) document paths.
The pane-spawned `agent-doc start --route-owned` is a bare CLI (no FFI catch_unwind /
panic hook), so its panics show as raw backtraces in the new pane and are invisible to
JB `TurnStateBridge.routeFailurePresentation` (fire-and-forget `skip_wait=true`).
