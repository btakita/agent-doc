---
name: feedback_compaction_stale_snapshot_reset_not_commit
description: "After a compaction, snapshot>visible drift is NOT response-loss — use reset --from-current, never plain agent-doc commit (it un-compacts)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 0e867af3-68d6-4da3-93dc-fbdc0b370658
---

When `preflight` reports `committed:false` with "snapshot is N bytes but the visible file is M bytes (N>M), looks like a manual cleanup" **and the cause is a just-run compaction** (visible file shrank, fresh `.agent-doc/archives/<hash>-<ts>.md` exists, HEAD still holds the pre-compact size), this is the INVERSE of the response-loss case.

**Why:** `agent-doc commit`'s 0.34.0+ auto-recover ([[feedback_ipc_crdt_response_loss_forcedisk_recovery]]) assumes the larger snapshot holds the FULL correct response that the visible file lost — so it restores the snapshot. After a compaction the larger snapshot is the PRE-compact content, so auto-recover wrongly **un-compacts** the doc. That is the `#compactdrift` trap.

**How to apply:** declare the compacted visible file authoritative first: `agent-doc reset --from-current --preserve-session <FILE>` → `agent-doc commit <FILE>`. Verify HEAD shrank to the compacted size and worktree==HEAD. Do NOT run plain `agent-doc commit` on this drift class. If finalize then blocks on fenced `<!-- agent:queue -->` example markers in the queue, retry with `--lint off` ([[feedback_lintfence_fenced_component_markers_break_lint_and_parser]]). Related: [[feedback_supervisor_crdt_replay_reset_from_current]].

**Stale-CRDT replay compounding this (seen 2026-06-18, binary 0.34.9):** if a live editor/IDE held the PRE-compact 30KB buffer, the persisted Yjs CRDT at `.agent-doc/crdt/<ph>.yrs` (+`.overlay.yrs`) keeps replaying that content into the snapshot, so every `reset --from-current` is overwritten within seconds and `finalize` fails `live_prompt_drift_after_preflight` with `content_ours_len`≈full-size. Fix: `mv` the stale `.agent-doc/crdt/<ph>.yrs`, `.overlay.yrs`, `.yrs.lock` (and `captures/<ph>`) ASIDE to stop the replay, then hand-rebuild the exchange (record the consumed queue prompts as a `> **Queue:**` blockquote + your `### Re:` response, empty the `agent:queue`), `reset --from-current --preserve-session`, `commit`. AFTER commit, `session-check` may still INTERRUPT with `dropped_queue_prompt_guard` / `#queue-user-edit-overwrite` (quoting a truncated mid-keystroke queue) because the stale `.agent-doc/state/cycles/<ph>.json` carries `dropped_queue_prompts`/`active_queue_heads` from the failed cycle, and `.agent-doc/turn-scope/<ph>.json` is stale too — `mv` BOTH aside; session-check then returns ok (`no_drainable_work`). Drafts under the `mail/` submodule are gitignored from the superproject — leave them uncommitted for review.
