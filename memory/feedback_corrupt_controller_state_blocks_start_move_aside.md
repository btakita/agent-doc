---
name: feedback_corrupt_controller_state_blocks_start_move_aside
description: "agent-doc start \"timed out waiting for project controller\" from a died auto_install_reexec recycle → 0-byte controller-state.json blocks launch; move wedge markers aside"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d845d3b3-1d66-43fb-9c53-5fa524744686
---

`agent-doc start --route-owned <doc>` failing with `Error: timed out waiting for project controller at .agent-doc/controller.sock`, and NO agent-doc/supervisor process alive anywhere.

**Cause:** a supervisor `auto_install_reexec` recycle (auto `make install` → hot-reload via execve onto the fresh binary) **died mid-hot-reload**, tearing down the whole controller/supervisor stack and leaving a truncated **0-byte `controller-state.json`**. `agent-doc controller status` then fails with `failed to parse ... EOF while parsing a value at line 1 column 0`. Because the state file is corrupt (not absent), lazy-launch can't treat it as "no controllers," so every `start` retry times out. Confirmed by `recycle-inflight/supervisor.json` = `{"reason":"auto_install_reexec",...}` with no live owner, plus stale `locks/controller-launch.lock` and a `supervisor-stale` marker.

**Why:** durable work is safe — the doc's response is in git HEAD + worktree; only the runtime control plane is wedged.

**How to apply:** move the corrupt/stale runtime state aside (non-destructive, matches existing `_*-aside-*` convention) — `controller-state.json` (0-byte), `recycle-inflight/supervisor.json`, `supervisor-stale`, `locks/controller-launch.lock`, `locks/route-starting-timeout-*.lock`, and dead unix sockets (`ipc.sock`, `supervisor/*.sock`, `controller-handoff-*.sock`). Then `agent-doc controller status` lazy-launches a fresh, healthy controller (verify `stale:false` + `matches_installed:true`).

For the crashed session's uncommitted `### Re:` response (doctor flags `direct_response_patchback`), `write --commit` needs pane ownership and `claim` churns stale `sessions.json` bindings from a Claude pane — instead commit the well-formed exchange block directly via git (path-scoped, `agent-doc(<slug>): <ts>` msg), then `agent-doc reset --from-current --preserve-session` to sync the snapshot. doctor `closeout_commit`→ok, preflight `no_changes:true`.

Do NOT `make install` to clear the follow-on `stale_install` warning mid-session — that recycle is what crashed the stack. Related: [[feedback_recycle_deadlock_open_cycle_end_turn_dont_poll]], [[feedback_no_mid_session_install_against_live_supervisor]], [[feedback_queue_consume_race_forcedisk_recovery]].

**Root cause FIXED + SHIPPED (2026-07-01, submodule commit `5c19aee1`, pushed to origin/main, installed + live-verified):** two defects in `agent-doc-orchestration/src/project_controller.rs` — (1) `write_bootstrap_state` used non-atomic `std::fs::write` (truncate-then-write → 0-byte on interrupted execve); now uses new `agent_doc_fs::write_atomic` (temp+rename). (2) `read_bootstrap` hard-errored on parse failure; now treats empty/corrupt state as `Ok(None)` (re-bootstrap) with a stderr warning. 5 regression tests, `make check` green. Live proof: `agent-doc controller status` against a hand-made 0-byte `controller-state.json` now warns `empty controller bootstrap state ...; treating as absent (will re-bootstrap)` and returns `active:false` instead of the old hard `EOF while parsing` error. This manual move-aside recovery is only needed on binaries BEFORE this fix (i.e. any install predating 5c19aee1). The fix landed bundled into a concurrent session's `refactor: extract freshness and context helpers` commit — no version bump / formal crates.io+PyPI release was cut (416 commits of concurrent refactor work sit unreleased since v0.34.65; publishing that snapshot is a separate operator decision, not part of this fix).
