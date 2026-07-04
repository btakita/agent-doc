---
name: feedback_verify_recycle_via_admin_inspect_freshness_not_lstart
description: "After an install, verify controller+supervisor recycle via admin inspect freshness=, NOT ps lstart (execve keeps old start time)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 09b865af-292b-401b-aea1-df9d94e8360e
---

To confirm "all controllers + all supervisors recycled after an install," the authoritative signal is `agent-doc admin inspect <doc>` → `freshness=controller:<fresh|stale>,supervisor:<fresh|unknown|stale>`. Controllers also expose `version`/`handoff_state` in `.agent-doc/controller-state.json` (want `stable` on the installed version).

**Why:** `ps ... lstart` is misleading. `session restart-supervisor <doc>` (continue-mode) recycles the supervisor onto the new binary via **in-place execve** — the pid AND lstart stay unchanged, but the process image is now the fresh binary. `admin inspect` reports `supervisor:fresh` correctly; ps still shows the old start time. Trust freshness, not lstart. (Continue-mode also bounces the harness child: `child_pid` changes, `restart_count` +1.)

**How to apply:**
- Controllers: check `.agent-doc/controller-state.json` version+handoff_state per project root (superproject + each submodule with a controller). A `/tmp/.tmp*` debug-build controller is a test artifact — ignore.
- Live supervisors: `admin inspect` each alive actor; `supervisor:fresh` = done. Recycle a stale one with `session restart-supervisor <doc>` (continue-mode, respects busy-state, non-disruptive).
- `supervisor:unknown` ≠ stale: means no live route-owned supervisor (registry pid dead or points at the pane `-zsh`). Nothing to recycle — stale registry entry, out of scope for a post-install recycle.
- Orphan supervisor for a **deleted** doc is NOT route-registered, so `admin kill-supervisor` returns "no running route-owned supervisor" and `session doctor` errors "failed to read <doc>". It can't be recycled (no session to continue) — kill the pid tree directly after confirming (it may own a live codex/harness child in a real tmux pane). Its leaked defunct children reap when the parent dies.

Related: [[project_agent_doc_stale_supervisors_not_recycling_need_force_restart]], [[feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs]], [[feedback_no_mid_session_install_against_live_supervisor]].
