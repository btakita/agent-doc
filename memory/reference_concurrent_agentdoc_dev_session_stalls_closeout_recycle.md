---
name: reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle
description: "A concurrent agent-doc DEV session mid-refactor of src/agent-doc (stale binary + 100s of uncommitted files) can wedge another doc's session-doc closeout — editor-authority/disk divergence, auto-recycle never idles. Don't force it."
metadata: 
  node_type: memory
  type: reference
  originSessionId: a5dfc28f-1c4e-4845-bcac-45b8ca1871f1
---

Observed 2026-07-15 while draining the EFS queue in `tasks/professional/equityfundingsource.md`.

**Symptom chain (in order):** `stale_install` warning (installed agent-doc predates local source edits) → `finalize` times out at `delivery_ack_pending` / `timed out after 0.8s waiting for project controller response` → `session-check` INTERRUPTED `write_applied` (response landed, no terminal commit) → after retry, `session-check` INTERRUPTED `canonical editor authority and disk projection diverge (authority_hash != disk_hash); Automatic supervisor recycle status: requested ... Do not rerun finalize, run write --commit, repair, or force-disk`. The response cell reaches the realtime backbone (`response cell added via realtime backbone (r:...)`) but `merge_by_component rejected divergent whole-document replay on theirs; refusing unsafe whole-document fallback`.

**Root cause:** a *different* live session was mid-refactor of `src/agent-doc` itself — `session-check` warned that `src/agent-doc` had 148+ uncommitted companion files / 241+ changed string literals, running a **stale binary**. That churn (a) keeps the project supervisor from ever hitting an idle boundary, so the requested recycle never fires, and (b) keeps the operator's live JetBrains editor authority diverged from disk. `agent-doc admin recycle` marks the controller to recycle "at next idle boundary" but idle never comes under the churn.

**Correct response (do NOT fight it):** the binary is right to refuse — forcing would risk clobbering operator/editor text. Do not loop finalize / write --commit / repair / force-disk; each just re-drifts the baseline. The captured response auto-reconciles once the concurrent session settles or the supervisor recycles. **De-risk by committing + pushing the real work to its own repo remote first** (EFS commits landed on the EFS remote independent of the superproject session-doc), so only session-doc bookkeeping is stuck, never the engineering work. Then report the pending session-doc closeout and stop; let the operator settle the other agent-doc session.

Related: [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]] (stale *plugin* ACK wedge — reload-lib fixes that one; this is a stale *binary* + cross-session source churn, which reload-lib/recycle can't clear until the other session idles). [[feedback_concurrent_sessions_force_push_shared_superproject]].
