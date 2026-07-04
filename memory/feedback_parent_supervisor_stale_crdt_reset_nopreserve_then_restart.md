---
name: feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart
description: "Stuck write_applied cycle replayed by your OWN parent supervisor's stale CRDT — reset --from-current (no --preserve) clears it; finalize still wedges per-cycle → operator restart, don't blind-loop."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 45f35e5a-db36-416b-aef7-3c8a4ae93a8c
---

When a session doc is wedged in `previous cycle still response_captured/write_applied` that re-applies on every preflight (re-injecting an archived/compacted response, mis-consuming queue heads), and the route-owned supervisor (`agent-doc start --route-owned <FILE>`) is **your own session parent** (`pstree -ps $$` shows `agent-doc(PID)---claude---zsh`) with a **current** binary (`agent-doc --version` == source `Cargo.toml`):

**Why:** the wedge is a stale *in-memory CRDT* in the parent supervisor (left by a mid-session compaction), NOT a stale binary. It does NOT self-heal by drain-lease expiry, and you must NOT kill/recycle it (it's the session parent — killing ends your session).

**How to apply:**
1. Clear the stuck cycle by hand-cleaning the doc (remove orphan fragments / de-corrupt queue+backlog asterisk-injection) then `agent-doc reset --from-current <FILE>` **WITHOUT `--preserve-session`** — this rebuilds snapshot+CRDT from disk AND drops the stuck cycle state (the `--preserve-session` variant keeps it stuck). Move the orphan `.agent-doc/captures|playback/<hash>/cycle-<id>.json` aside first. Preflight then comes back clean (`committed`, queue drainable).
2. You CAN now drain a head and do the real repo work (it lives in a clean submodule, independent of agent-doc IPC).
3. BUT every `finalize` still wedges on the live parent: `live_prompt_drift_after_preflight` → `content_ours_snapshot_next_cycle` → `queue consume: failed to write document`. Recover per-cycle via `agent-doc commit` (auto-recovers; HEAD gets the correct response) then `git checkout HEAD -- <FILE>` for the `postcommit_worktree_check match=false` corruption. `session-check` then shows `committed`.
4. An operator session restart drops the supervisor's stale in-memory CRDT and gives a **clean preflight**, BUT the finalize-time `postcommit_worktree_check match=false` worktree drift can still recur **every** cycle. The per-cycle recovery (`commit` auto-recovers HEAD + `git checkout HEAD -- <FILE>`) is deterministic and reliable, so once you've confirmed it twice you CAN keep draining inline (do NOT use the automated `/loop` skill — it expects clean finalizes and won't run the recovery). Each finalize: pass `--done <id>` (gate enforces it when the response completes a pending id); if it errors `queue consume: failed to write document`, run `agent-doc commit` then `git checkout HEAD -- <FILE>`, then `session-check`. See [[feedback_supervisor_crdt_replay_reset_from_current]] and [[feedback_post_commit_worktree_drift_checkout_head_recovery]].

Live proof 2026-06-25 `tasks/software/corky.md`: drained ALL FOUR go-queue heads this way across the wedge — `#ckypubsm` (corky `7ee6692`, social publish crash-safety), `#ckyemailsm` (`e6d8490`, email status enum), `#ckysyncsm` (`97e762e`, sync run-state + durable marker), `#ckywatchsm` (`15cc39f`, watch phase + circuit breaker). Queue fully drained (`no_drainable_work`); one follow-up `#ckysyncsm-resume` left in backlog. The first wedge-clear needed `reset --from-current` (no preserve); after the operator restart every cycle still needed the `commit`+`checkout HEAD` recovery but all closeouts reached `committed`.
