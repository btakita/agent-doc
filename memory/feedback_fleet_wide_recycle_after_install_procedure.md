---
name: feedback_fleet_wide_recycle_after_install_procedure
description: Recycle ALL controllers + supervisors onto a fresh install; convergent and safe even under concurrent source edits
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 09b865af-292b-401b-aea1-df9d94e8360e
---

Procedure to guarantee the whole fleet runs the freshly-installed binary after an install:

1. `agent-doc admin recycle --all-projects` (idle-gated, **NO `--force`**) — recycles every project controller onto the installed binary at its next idle boundary and marks in-process supervisors. JSON reports `recycled/skipped/supervisors_marked`. Idle-gating makes it non-disruptive and **convergent**: each controller picks up whatever binary is installed when it hits idle.
2. Standalone `start --route-owned` supervisors are NOT covered by controller recycle — recycle each with `agent-doc session restart-supervisor <FILE>` (continue-mode; execve's in place, pid/lstart unchanged per [[feedback_verify_recycle_via_admin_inspect_freshness_not_lstart]]).
3. Verify per-actor: `agent-doc admin inspect <FILE>` → `freshness=controller:fresh,supervisor:{fresh|unknown}`. `supervisor:unknown` = in-process supervisor (no separate process to measure), NOT stale. Confirm project controller `controller-state.json` `modified_secs` == installed binary mtime and `controller_generation` advanced.

**Why:** After the 08b in-process-supervisor migration, most docs' supervisors run inside the project controller, so recycling the controller recycles them; only legacy route-owned docs need `restart-supervisor`. Orphan supervisors for docs not in `admin list` (deleted/moved doc) self-terminate or are killed directly, not recycled.

**How to apply:** Do NOT `make install` to "freshen" when uncommitted source is AHEAD of the install but being edited by a CONCURRENT session — that compiles a half-edited tree. Recycle onto the CURRENT completed install (convergent); the concurrent session's CPC+Supervisor auto-install path ([[feedback_supervisor_autoinstall_drives_install_not_manual.md]]) drives its own install+recycle when it finalizes. See [[feedback_no_mid_session_install_against_live_supervisor]].
