---
name: feedback_synthetic_noliveness_reap_frontmatter_drop_suprecyclespin
description: "Preflight \"crash\" = corrupt HEAD (frontmatter stripped + orphan boundary) from a no_liveness_signals synthetic reap; recover via revert-to-scaffold + reset --from-current."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 7ce6549e-e90f-4389-9699-ea60a9102fc2
---

A doc that "crashes during preflight" with an endless suprecyclespin
(`cycle_never_closed`, streak 40 → force_recycle → abandon → reopen) can be a
**corrupt HEAD**, not a live-process crash. Signature: a `route_owned`
`no_liveness_signals` **synthetic** auto-reap (cycle `synthetic-<millis>`,
`supervisor_host_inprocess_exit exit_code=1`) committed a snapshot that had
**dropped frontmatter keys** and emptied the exchange (orphan
`<!-- agent:boundary:... -->`), while the operator's correct content survived
only in the **working tree**. Because `git::commit` STAGES the snapshot (not the
working tree) and preflight's commit step collapses the snapshot to HEAD, the
corrupt HEAD re-poisons the snapshot every cycle → permanent `doc != snapshot`.

**Diagnose:** compare `git show HEAD:<doc> | wc -c` vs working-tree size; check
`git diff <scaffold-commit> HEAD -- <doc>` for dropped frontmatter lines; empty
`captures/<dochash>/` = no response was ever produced.

**Recover (working tree is authoritative):** `git restore --source=<scaffold>
--staged -- <doc>` → `git commit` (HEAD back to scaffold, prompt stays
uncommitted) → put scaffold in the file, `agent-doc reset --from-current
--preserve-session --force-disk`, restore the prompt. Verify `agent-doc diff` is
a clean prompt-only add (no frontmatter/boundary churn).

**Why:** the snapshot is on the commit hot path (selective-commit staging), so a
corrupt snapshot becomes a corrupt HEAD. **How to apply:** product fix `#fmdrop`
(`commit_integrity.rs` + `git.rs`) self-heals by overlaying live-doc frontmatter
onto the staged content + regenerating the snapshot — frontmatter is
authoritative from the live doc, never selectively committed. Related:
[[feedback_stale_binary_crdt_deadlock_direct_write_git_commit_reset]],
[[feedback_boundary_accum_worktree_only_ipc_repair_checkout_head]],
[[feedback_suprecyclespin_end_turn_does_not_persist_response_forcedisk]].
