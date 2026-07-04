---
name: feedback_jb_restart_reopen_dont_fix_stale_cdylib_corruption
description: Stale-cdylib CRDT corruption (prompt_duplication_in_ack_content 7x) — JB restart AND close/reopen both fail; only make install or force-disk resolves
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 0a5b1f29-42d1-4939-b454-69f64b1bdc3b
---

When `finalize`/`write --commit` repeatedly fails with `invariant=prompt_duplication_in_ack_content` (`duplicate_prompt_count=7`) / `live_prompt_drift_after_preflight` **and** every preflight prints the `stale_install` warning, the root cause is the on-disk JB cdylib being stale relative to agent-doc source — its CRDT merge logic splices partial/duplicated prompts.

**Why:** Both `restart JetBrains` and `close/reopen the document` reload the **same stale on-disk cdylib**, so neither clears the corruption — they only reset buffer state, not merge code. The corruption reproduced identically after both. The cdylib is rebuilt **only** by `make install` (JB hot-reloads it by mtime).

**How to apply:**
1. Don't burn cycles on restart/reopen for this signature — they can't fix cdylib code. Offer `make install` (root fix) or `force-disk` (immediate) instead.
2. `finalize --force-disk` lands the CORRECT complete exchange in HEAD even while the live buffer stays mangled — but it prints a **false-negative** `session-check INTERRUPTED: ... prompt-only closeout tail and no assistant response` because session-check reads the stale live buffer, not HEAD. **Check HEAD and STOP** (see [[feedback_forcedisk_committed_but_false_closeout_error_stop_retrying]] and [[feedback_session_check_marker_error_contradicts_preflight]]). Verify HEAD has prompt 1x + response 1x + single boundary + clean closing marker; do NOT retry or `reset --from-current` (re-poisons).
3. HEAD after preflight may have an EMPTY exchange (prompt lives only on disk/buffer) — so `checkout HEAD` would lose the prompt. force-disk's CRDT merge folds the disk prompt + captured response together correctly. See [[feedback_checkout_head_discards_uncommitted_operator_prompt]].
4. Confirm before running `make install` mid-session per [[feedback_no_mid_session_install_against_live_supervisor]].
