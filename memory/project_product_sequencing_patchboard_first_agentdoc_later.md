---
name: product-sequencing-patchboard-first-agentdoc-later
description: agent-doc is a long-term product play (market later); Signals Patchboard is marketed FIRST; current agent-doc module extraction is deliberate architecture paydown from the turn-based→realtime migration
metadata: 
  node_type: memory
  type: project
  originSessionId: 9de43b95-223d-4dbd-a84d-9089f29dc16d
---

Stated by Brian 2026-07-04: **agent-doc is a long-term product play**, not just internal tooling. It has been net-beneficial to his productivity, but the move from turn-based to realtime interactions hit architectural walls — the current module-extraction commit stream is deliberate architecture debt paydown, not churn. He plans to market agent-doc **later**.

**Signals Patchboard will be marketed first.** Its full 9-phase implementation plan (#spb1–#spb9, Slint UI, signal-space substrate) lives at `tasks/software/plan-signals-patchboard-full.md` with backlog in `tasks/software/signals-patchboard.md`; as of 2026-07-04 no phase had started.

**Why:** Prioritization advice should treat agent-doc refactoring as product engineering with a defined stability bar, not as tooling distraction — but weigh it against Patchboard's first-to-market position.

**How to apply:** When assessing allocation, ask whether agent-doc work is on the critical path to its dogfood-stability bar (per-cell CRDT [[project-agent-doc-percell-crdt-merge-intent]] + realtime IPC reliability) versus whether Patchboard #spb1 should take the primary slot. Synergy note: #spb8 (agent-supervisor wedge) consumes agent-doc signals, so the products converge late. Related: [[project_agent_doc_module_decomposition]], [[feedback_stop_cutting_agent_doc_releases]].
