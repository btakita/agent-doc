---
name: feedback_degraded_ipc_write_retry_duplicates_response
description: Retrying write/finalize on degraded editor IPC duplicates the response — each retry queues a patch that all apply on recovery.
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 991eaf67-65e0-48ca-a311-9c654f62af8e
---

On degraded editor IPC (socket ack timeout → file-IPC fallback, `recovery=retry_without_disk_write`, no_ack), each `agent-doc write --commit`/`finalize` attempt writes a patch to `.agent-doc/patches/<pathhash>.json`. When the IntelliJ plugin's IPC later recovers, it applies **every** queued patch — so N retries of the "same" response produce **N duplicate `### Re:` blocks** in the exchange (live repro 2026-06-20 agent-doc-bugs2.md: two retries → two near-identical responses committed).

**Why:** the patch queue is additive; a failed-to-ack write is not rolled back, it's left for editor retry. Retrying enqueues another.

**How to apply:** On degraded editor IPC, attempt the response write **exactly once**. If it doesn't prove, do NOT retry — leave the single queued patch for the editor and end the turn (the queued supervisor recycle + editor reconnect applies it once). If duplication already happened, dedup by removing the extra block, then `reset --from-current --preserve-session` + `commit` (sidecar-only, no session-doc disk write). Also: `git checkout HEAD -- FILE`, `Edit`/`sed` on a live-editor session doc are themselves FCC triggers (disk write behind the editor) — prefer `reset --from-current` + `commit`. Related: [[feedback_supervisor_streaming_socket_wedge_forcedisk]], [[feedback_post_commit_worktree_drift_checkout_head_recovery]], [[feedback_hung_agent_doc_write_stale_live_buffer]].

Also this session: a `compact` left a 30KB disk file MISSING the Queue/Backlog sections the live editor buffer (~53KB) still held — the editor-buffer restore (`postcommit_worktree_check match=false`, `#pcwc`) was the *fuller correct* document, not stale. When disk (compacted) and editor buffer diverge massively, don't assume the smaller disk file is canonical.
