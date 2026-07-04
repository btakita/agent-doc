---
name: feedback_dead_editor_granular_write_scrambles_head_forcedisk_from_clean_commit
description: Granular writes against a dead-editor (no IDE) doc scramble HEAD via stale-base merge; recover by force-disk from the last CLEAN commit
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 09b865af-292b-401b-aea1-df9d94e8360e
---

When a session doc's live editor is DEAD (no JetBrains/IDE process in `ps`, `verify-op-capture` shows no accepted marker, writes fail `no_ack` / `recovery=retry_without_disk_write`), do NOT issue granular `agent-doc write` mutations (`--backlog-add`, etc.) hoping they land. The stale-base CRDT merge can SCRAMBLE the commit — a `--backlog-add` for an item actually REMOVED that item from HEAD (working buffer ≠ HEAD), making things worse and re-arming the "open backlog item absent" guard.

**Why:** with no live editor to prove the write, the working buffer and HEAD diverge; each granular write merges against a drifted/truncated base and persists the wrong side. Repeatedly poking a dead-editor doc corrupts it.

**How to apply — deterministic force-disk recovery (operator must explicitly authorize `--force-disk`, per SKILL):**
1. Confirm the editor is truly dead: `ps` for the IDE (none), `verify-op-capture <doc>` (no accepted marker), stale-sidecar reaps in preflight. No live buffer = force-disk is safe (nothing authoritative to clobber). See [[feedback_send_failed_no_ide_running_forcedisk_not_restart]].
2. Pick the last CLEAN operator commit (NOT current HEAD if HEAD is the scrambled one — `git log --oneline -- <doc>`, diff to find the good base) and `git checkout <clean-commit> -- <doc>`. Verify no uncommitted operator prompt is lost first ([[feedback_checkout_head_discards_uncommitted_operator_prompt]]).
3. `agent-doc reset --from-current --preserve-session <doc>` (snapshot/CRDT rebuilt from the clean disk base).
4. `cat <<RESPONSE | agent-doc finalize <doc> --force-disk --done <id> --origin skill` with the `<!-- patch:exchange -->` answer; quote the queue head as `> **Queue prompt:** <exact head>` so it strikes.
5. Verify HEAD has the answer + resolved item + struck head; then `git checkout HEAD -- <doc>` to clear post-commit working-tree drift ([[feedback_post_commit_worktree_drift_checkout_head_recovery]]). `--done` on a resolved queue prompt removes it from backlog + strikes the head (may not land in `.done.md`; that's fine — item is resolved, not dangling).

Ties to [[feedback_stale_binary_crdt_deadlock_direct_write_git_commit_reset]] and [[feedback_ipc_crdt_response_loss_forcedisk_recovery]].
