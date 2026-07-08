---
name: feedback_turn_unfinalized_live_editor_authority_blocks_commit_forcedisk_gitcommit
description: "Turn didn't finalize because a live JB editor held CRDT authority and agent-doc commit refused disk; recover via force-disk reset + direct git commit + realign snapshot"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 249fae54-7a60-4c70-bf2a-2353758d3c82
---

Symptom: an agent-doc turn produced a response on disk but never committed. `agent-doc commit <doc>` refuses with `Error: editor is the current authority for <doc>, but CRDT relay convergence is still pending; disk is a non-authoritative replica and was not used as commit authority`. A live JetBrains IDE (`idea` pid) holds the repo-root `~/.agent-doc/ipc.sock` listener; `ss -xlp | grep agent-doc` shows it even when `ps | grep -i agent-doc` finds nothing (the sandbox `ps` can't see other PIDs, but `ss` reads `/proc/net/unix`). The editor's persisted `live-buffer/<hash>.jetbrains-<pid>-*` is stale (old mtime) and won't converge. `commit` has NO `--force-disk` flag (only `write`/`finalize` do, and those append a new response — wrong when the response already exists).

**Why:** the commit guard conservatively defers to any live IPC listener; `reset --from-current` (non-force-disk) can't force a wedged editor to converge.

**How to apply (recovery, no live-process clobber risk once verified):**
1. Confirm the response is on disk and repair any CRDT churn directly with Edit (this turn: `doingA`→`doing —` dropped-em-dash UTF-8 corruption; stray `— opus-4-8 (HEAD)` header suffixes; a prior `accept.n regenerates` prompt/response splice).
2. `agent-doc reset <doc> --from-current --preserve-session --force-disk` — rebuild CRDT/snapshot from cleaned disk.
3. Direct `git add <doc> && git commit` (agent-doc's timestamp convention `agent-doc(<name>): <ts>`). This lands clean content in HEAD definitively; if the stale editor later flushes and drifts the working tree, `git checkout HEAD -- <doc>` recovers.
4. `agent-doc reset <doc> --from-current --preserve-session --force-disk` again to realign the snapshot boundary with committed HEAD (the direct git commit skips agent-doc's boundary reposition).
5. Verify `git show HEAD:<doc>` has the response + 0 churn markers, and `agent-doc admin inspect <doc>` shows `state=ready ... fresh`.

Fully clearing the stale editor buffer needs the operator to reopen the IDE tab; the commit is safe in HEAD regardless. Related: [[feedback_reload_lib_cant_unhang_wedged_plugin_handler_writecommit_hang_recover_via_commit]], [[feedback_degraded_ipc_blocks_granular_pending_writes]], [[feedback_post_commit_worktree_drift_checkout_head_recovery]]. Beware [[reference_bin_agent_doc_symlink_is_reporoot_relative]] when invoking the CLI during recovery.
