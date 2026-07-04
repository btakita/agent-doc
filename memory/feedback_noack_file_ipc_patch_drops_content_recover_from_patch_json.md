---
name: feedback_noack_file_ipc_patch_drops_content_recover_from_patch_json
description: "A crashed-session no_ack file_ipc patch can DROP its content entirely (never lands on disk) — recover the new text from .agent-doc/patches/<id>.json, don't replay the raw patch"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d845d3b3-1d66-43fb-9c53-5fa524744686
---

An `ipc_proof_insufficient ... source=file_ipc ... invariant=no_ack` dogfood log (a JB `editor_id`) whose session then crashed: the editor never acked a `file_ipc` patch, and unlike a hung `--pending-add` (which usually still lands on disk — [[feedback_hung_pending_add_recover_via_agent_doc_commit]]), a no_ack **file_ipc patch can leave its content nowhere in the worktree**. Confirm with `grep <new-tag> <doc>` → empty.

**Why:** the dropped write was a backlog item (`[#falsetyping-guard]`); prose in an earlier committed cycle even claimed it was "filed as #tag", but the actual item was in the no_ack patch and never persisted, so it was genuinely lost.

**How to apply:**
1. Read the stranded patch: `python3 -c "import json;d=json.load(open('.agent-doc/patches/<id>.json'));[print(p['component'],p['content']) for p in d['patches']]"`. A `component: backlog` patch is a FULL-backlog snapshot — the only NEW content is the added item(s). Its baseline predates any later crash-recovery edits, so DON'T replay the raw patch (it clobbers the recovery's edits, e.g. reverts drained items).
2. Diff the patch's content against the current doc; surgically Edit just the missing item(s) back in (single-quote-safe; the item text has backticks/parens that break `--pending-add`).
3. Commit path-scoped via git (`agent-doc(<slug>): <ts>`) — this also absorbs any co-pending exchange drift (the auto-appended `### Re: IPC proof diagnostic` dogfood block) — then `agent-doc reset --from-current --preserve-session`. doctor `closeout_commit`→ok, preflight `no_changes:true`.
4. `mv .agent-doc/patches/<id>.json` aside so it can't replay. Other lingering `patches/*.json` from prior days are consumed cruft (agent-doc doesn't GC them); only the crash-window mtime one matters.

Distinct from the LIVE dead-JB-write-socket case ([[feedback_no_ack_write_socket_finalize_fails_writecommit_recovers.md]], fix=IDE restart) — here the session is already dead, so use the git-native path (no IPC dependency), same as [[feedback_corrupt_controller_state_blocks_start_move_aside]].
