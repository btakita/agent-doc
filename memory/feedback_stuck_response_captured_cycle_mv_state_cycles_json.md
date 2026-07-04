---
name: feedback_stuck_response_captured_cycle_mv_state_cycles_json
description: A response_captured cycle that hijacks every preflight survives reset --preserve-session; mv the state/cycles/<dochash>.json aside to clear it.
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 4e5600cf-b650-40bf-b0c8-543b1aee7820
---

When `finalize` gets IPC `already_applied` but the response never reaches disk (stale-binary supervisor wedge), the cycle sticks in `phase: response_captured`. Every subsequent `preflight`/`write --commit` re-runs its recovery FIRST and fails with `visible editor buffer differs from expected disk state` — even `--force-disk` is gated by that precheck.

**Why:** `reset --from-current --preserve-session` preserves the poisoned capture, so preflight keeps re-adopting the same stuck cycle. `--force-disk` on the repair-replay path still runs the buffer-differs guard before writing.

**How to apply:** The hijacking bookkeeping lives at `.agent-doc/state/cycles/<dochash>.json` (keyed by the PRE-response doc content hash, e.g. `961610ec...`, with `phase: response_captured`). Recovery: (1) verify operator text is safe on disk and HEAD is clean; (2) `mv .agent-doc/state/cycles/<hash>.json .agent-doc/_stale-aside/`; (3) `agent-doc preflight` now reports the clean "snapshot differs from HEAD without an open cycle" state; (4) re-`finalize --force-disk` with the response text in hand — CRDT merges clean and commits. The response body is safe (you authored it); the cycle file is just failed-IPC recovery state. Extends [[feedback_agent_doc_stale_capture_blocks_commit]] with the concrete file location; pairs with [[feedback_suprecyclespin_end_turn_does_not_persist_response_forcedisk]] and [[feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs]].

**Refinement (2026-07-01, stale-JB-cdylib variant):** When the wedge is a stale JetBrains cdylib (`invariant=live_prompt_drift_after_preflight`, preflight `stale_install` warning; the live editor buffer `content_ours` is bigger than the ACK sidecar but never incorporates the patch), the cleaner recovery is **operator restarts JetBrains** (fresh cdylib) rather than force-disk. After the restart: the wedge auto-recovers via editor IPC convergence but the cycle sticks at `response_captured`; `reset --from-current --preserve-session` still keeps the poison, so mv the cycle file aside (step 2). Then preflight goes clean but reports **`no_changes: true`** — because `reset --from-current` folded the operator prompt into the baseline. The response is missing from disk, so close it with `agent-doc write --commit <FILE> --baseline-file <new> --stream` piping the response text (the "missed response repair" path). With the fresh cdylib this **proves through the editor IPC and commits without `--force-disk`**. The post-write "latest cycle `synthetic-...` is already `committed`; refusing explicit-baseline response" error fires AFTER the write already landed — check git (new commit + clean worktree) before retrying. If a `--backlog-add` from the original failed `finalize` already reached disk, do NOT re-add it on the repair write (dedupe).
