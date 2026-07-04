---
name: feedback_concurrent_sessions_force_push_shared_superproject
description: Multiple live agent-doc sessions share+rewrite the agent-loop superproject git — a collided session must not force-push/finalize onto rewritten history.
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 22a38744-b7ad-4026-89e4-52a11528c752
---

The agent-loop dogfooding env frequently runs **multiple concurrent agent-doc sessions** (e.g. agent-doc-bugs2.md + reitrades.md + equityfundingsource.md) that all commit to the **same superproject repo** AND the shared `src/agent-doc` submodule. They rebase + **force-push origin/main** every few seconds (observe `partial_staging_closeout_guard_fired` across multiple `repo=` in ops.log, and commit messages that reappear with different hashes).

Consequence for a session that finds its own working-tree edit already committed under a different message (a concurrent `git::commit()` sweep swept your uncommitted change into origin): **verify the deliverable is durable on origin (submodule branch + superproject pointer), then STOP.** Do NOT `git push`, rebase onto the rewritten origin/main, or run a session-doc `finalize` from the diverged/collided local history — that fights the authoritative drain and produces the recovery churn all over [[feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart]] / [[feedback_queue_consume_race_forcedisk_recovery]].

**Why:** origin/main is a contended, actively-rewritten shared resource; a collided session's local main is stale/diverged and its push/finalize will conflict or be reverted.

**How to apply:** confirm fix on origin (`git show origin/main:<file>`, `git ls-tree origin/main src/agent-doc`), undo any redundant local pointer commit (`reset --mixed` — from the SUPERPROJECT root, mind the [[feedback_git_rebase_cwd_submodule_hazard]]), leave the queue-head strike to the authoritative drain/operator, and report. Beware: the cwd/submodule hazard means a `reset` run from inside `src/agent-doc` resets the submodule, not the superproject.
