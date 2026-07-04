---
name: feedback_auto_recommendations_autodrain_confirm_before_implement_push
description: "AI-generated [recommended] backlog items auto-drain in a go-queue doc — confirm scope before implement+push instead of silently building."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 86cba79a-11be-47b0-a631-5a857925a2b2
---

In a doc configured for continuous-backlog-loop (`<!-- agent:backlog priority queue -->` + `<!-- agent:queue ... go -->`, e.g. `tasks/software/tsift.md`), any backlog item — **including `[recommended]` items the agent itself generated from a research/analysis answer** — auto-mirrors into the go-queue and becomes a drain target. Same-cycle-added items show `queue_drainable_head_count: 0` / `queue_continuation_required: false` (deferred as same-cycle bookkeeping), but on the **next** preflight they flip to drainable=N / continuation_required=true and the loop will try to drain them under the queue's preset (e.g. `#spec-test-build-install-commit-push` → implement + build + commit + **push**).

**Why:** the operator asked an analysis question ("is there anything in this blog that benefits tsift?"), I answered + filed `[recommended]` follow-ups, and they auto-promoted into the loop. Auto-implementing + pushing a substantial speculative feature off my own recommendation is scope-overreach and hard to reverse.

**How to apply:** When the next drain head is an AI-authored `[recommended]` design/feature item (not an operator-written directive), do NOT silently implement+push. Use `AskUserQuestion` to confirm: plan-only / implement+push / implement-hold-push / stop-draining. The drain-don't-stall contract still governs *real* operator-queued work — but the "confirm hard-to-reverse outward-facing actions" rule wins for an unrequested push. The contract-aligned defer for such items is to tag them so go-mode defers (like `[operator-verify]`), or drain them as a plan file (`--done` the recommendation once the plan exists). 2026-06-20: operator chose "Plan only, no push" → wrote `tasks/software/plan-tsift-zonemap-segment-stats.md`, `--done`'d both items. Relates to [[feedback_session_document_scope_discipline]] and [[feedback_go_queue_defer_undrainable_tags]].
