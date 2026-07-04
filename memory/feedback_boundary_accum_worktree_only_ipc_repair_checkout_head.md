---
name: feedback_boundary_accum_worktree_only_ipc_repair_checkout_head
description: agent:boundary repeated only in working tree (HEAD clean) from ipc_visible_repair recovery → checkout HEAD
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ac29a920-720f-4271-bc4e-b59b9ab1c046
---

When `agent:boundary` markers are reported repeated (e.g. 4 stacked at end of
`agent:exchange` + an orphan mid-exchange), FIRST check `git show HEAD:<doc> |
grep -c agent:boundary` and the full `git diff -- <doc>`.

In the 2026-07-01 laura-s.md recurrence the accumulation was **working-tree-only**:
HEAD was clean at exactly one boundary (`f13ef957`), and the uncommitted diff was
pure interrupted-cycle recovery corruption — stacked boundaries, duplicated `(HEAD)`
markers across multiple `### Re:` headings, an orphan boundary after a binary-authored
`ipc_proof_insufficient` diagnostic block, and status bullets misplaced into the top
of the exchange. No live agent-doc supervisor process was running (`ps`).

**Why:** the commit-time boundary collapse worked (HEAD=1), but the
`ipc_visible_repair` / `ipc_proof_insufficient` disk-writeback re-emitted the exchange
with an appended boundary per repair attempt. HEAD is authoritative and lost no
operator content (all responses committed; no new `❯` prompt in the diff).

**How to apply:** confirm HEAD has all responses + single boundary + no new operator
prompt in the worktree diff, confirm no live supervisor (`ps`), then
`git checkout HEAD -- <doc>` to discard only the corruption. Do NOT hand-edit the
5-boundary mess. Feeds root-cause plan [[project_agent_doc_percell_crdt_merge_intent]]
and #boundaryaccum1 (tasks/agent-doc/plan-boundary-accumulation-committed-blob.md):
the self-healing invariant must cover the recovery/repair writeback path, not just
pre-commit staging. Related: [[feedback_checkout_head_discards_uncommitted_operator_prompt]].
