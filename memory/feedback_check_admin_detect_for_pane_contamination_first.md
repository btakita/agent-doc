---
name: feedback_check_admin_detect_for_pane_contamination_first
description: "When JB Run Agent Doc no-ops or a queue item won't process while the session looks idle, run `agent-doc admin detect` for cross_document_pane FIRST — don't theorize busy/auto-loop."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: f2c00b5b-145d-437d-8f0e-c77e0cc6b648
---

When the user reports "the Codex/agent session is idle but my queue item didn't process" or "JB Run Agent Doc does nothing," the first diagnostic is `agent-doc admin detect` (from the owning project root, e.g. `src/boost-client`) looking for a `cross_document_pane` finding — a tmux pane bound to two documents at once. A stale second binding (e.g. `astro-listings [claude-code]` on a pane actually running `monsterrodholders [codex]`) leaves no single authoritative actor, so queue dispatch and route land nowhere even though the doc looks idle.

**Why:** 2026-06-06 I repeatedly misdiagnosed this — capturing the pane (`Working …`), theorizing "busy auto-loop, user catching idle gaps," and nearly shipping a dangerous Codex stale-cue detection change that would have injected into a live turn. The user was right ("you're missing the evidence / it was idle"). The actual cause was `cross_document_pane` contamination; `admin detect` showed it immediately. Fix shipped as bind-time eviction (`#monsterrod-pane-cross-doc-contamination`, commit 25ff1116: `evict_cross_document_pane_bindings` in project_controller, wired into `session_actor::store_record_in` — a pane binding closes any other document's actor on the same pane). Live-verified: astro-listings → closed, monsterrodholders → sole owner, queue resumed processing.

**How to apply:** For "idle session won't process / button no-ops," run `admin detect` and check pane ownership (`admin list | grep pane=%N`) and the real process tree (`pstree`) BEFORE theorizing about busy/auto-loop state. Verify *which document* owns the pane, not just whether the pane is busy. Don't ship busy-vs-idle detection changes on a Codex pane without confirming the live turn is stale — a wrong call injects into a real turn. See [[feedback_verify_proxy_before_kill]], [[feedback_no_hallucinate_uncertainty]].
