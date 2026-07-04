---
name: feedback_post_commit_worktree_drift_checkout_head_recovery
description: "In a degraded session, finalize commits the correct response to HEAD but the supervisor mangles the WORKING TREE post-commit; recover with git checkout HEAD -- FILE, not re-finalize."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a25c7728-df2d-472a-9937-bed61808c50b
---

In a degraded / restart-heavy `/agent-doc` session (preflight `session_accretion.level: warn`, "N restart-heavy events"), a single-`patch:exchange` `finalize --stream` can hit `live_prompt_drift_after_preflight` yet still commit the FULL CORRECT response to HEAD via `content_ours` recovery (`[write] snapshot from content_ours`). The damage is post-commit and WORKING-TREE-ONLY: the route-owned supervisor's CRDT replay rewrites the working file with a MANGLED response (bullets reordered, `### Re:` heading + intro/recommendation prose dropped). This trips `session-check INTERRUPTED: unresolved prompt-bearing user changes` — a FALSE positive, because the classifier reads the mangled working-tree response bullets (queue-id lines like `` - `#s760wire` ... gated ``) as new user prompt_targets.

**Why:** distinct from [[feedback_ipc_crdt_response_loss_forcedisk_recovery]] — there the commit itself wedges (snapshot > visible file) and `agent-doc commit` auto-recovers. HERE `agent-doc commit` does NOT help: it reports "HEAD already contains the committed response ... leaving later local working-tree edits uncommitted" because HEAD is already correct; only the working tree drifted.

**How to apply:**
1. Don't trust the session-check interrupt or the mangled visible file. Verify HEAD: `git show HEAD:tasks/.../doc.md | sed -n '<exchange range>p'` — if the heading + ordered bullets + prose are all present, HEAD is authoritative.
2. `git diff HEAD -- FILE` confirms the working-tree-only drift (reordered/truncated response).
3. Recover with `git checkout HEAD -- FILE` (NOT re-finalize, NOT `reset --from-current`). Then `agent-doc session-check FILE` → `ok ... commit_already_current`.
4. Reduce recurrence: keep `### Re:` response prose from being a dense list of `#id` + `gated`/`BLOCKED`/`do` tokens — that shape pattern-matches as prompt_targets and feeds the CRDT-replay/classifier drift.

Root-fix is [[project_agent_doc_supervisor_migration_intent]] (#pcpconsol single ordered write queue removes the supervisor/finalize self-race). Until then, do NOT drain fresh-session/live-gated queues in a degraded restart-heavy session.
