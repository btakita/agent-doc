---
name: feedback_jb_run_agentdoc_route_owned_pane_processing_prompt_collision
description: "JB \"Run Agent Doc\" spawns a live route-owned tmux pane that IS processing the prompt; a manual /agent-doc in another pane collides — check for a live owner before writing."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 04455a3d-0890-4b78-abaf-f9847af8fefb
---

When the operator says JB `Run Agent Doc` "auto-started a tmux pane, prompt added but not submitted," do NOT assume the prompt is stranded and start writing the response yourself. JB launches `agent-doc start --route-owned <FILE>` in its own tmux pane, which becomes the **live session lease owner** and may already be actively processing the same prompt.

**Diagnose before any write:**
- `agent-doc admin inspect <FILE>` → note the owning `pane=%NNN` and `state`.
- If your own pane differs (a manual `/agent-doc` runs in the operator's interactive pane, e.g. `%81`, while JB's pane is `%139`), you are the *colliding* session.
- `ps -o cmd -p <pid>` on the owner → `agent-doc start --route-owned` confirms JB spawned it.
- `tmux capture-pane -p -t %NNN | tail` → if it shows a Claude "thinking/Shimmying" spinner and a transcript analyzing the same prompt, the owner is mid-cycle and about to finalize. **Stand down.**

**Why:** two sessions finalizing the same doc is the double-append/CRDT-scramble class. The owning `route-owned` pane did its own preflight recovery (moved stuck cycle + stale capture aside) and holds the lease; your writes get correctly rejected (`nack file_apply_failed`, then `pane ownership mismatch`).

**How to apply:** report that the JB pane already owns and is processing the prompt, let it finish, then verify HEAD. Do NOT `claim`/`--force-disk` to wrest ownership from a *live, actively-processing* owner. NOTE the failure trap: a plain `agent-doc write` can still land its patch on disk *even after printing a nack* — so if you already fired one, confirm what leaked (`grep` your id in the doc; check `git status`) and, if it's a non-exchange component (backlog/review) that per-cell CRDT can merge, restore any referenced artifact (e.g. a deleted plan file) rather than surgically editing the foreign-owned doc, which risks a worse scramble. Related: [[feedback_concurrent_sessions_force_push_shared_superproject]], [[feedback_concurrent_agent_doc_refactor_session_build_coordination]], [[feedback_finalize_from_wrong_pane_binding_during_fleet_recycle]], [[feedback_send_failed_no_ide_running_forcedisk_not_restart]].
