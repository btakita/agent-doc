---
name: feedback_session_check_marker_error_contradicts_preflight
description: "session-check \"closing marker without matching open\" on a doc preflight parses as clean = parser anomaly, not corruption — stop, don't fight"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 286e964b-40c1-4697-9f84-60ddd05a3019
---

`agent-doc session-check` can report `Error: closing marker <!-- /agent:exchange --> without matching open` on a session doc that is actually structurally fine: the disk file has exactly one `<!-- agent:exchange -->` open + one close in order, `grep -cF` confirms balance, the linked `.done.md` archive has no exchange markers, and **`agent-doc preflight` parses the SAME file as clean** (`no_changes:true`, cycle already `committed`). Observed 2026-06-21 in the EFS `equityfundingsource.md` session right after a `finalize --force-disk` deploy-drain (commit `a0458333e`); session-check had passed on the immediately prior commit (`b16cfa5f3`) with the identical marker structure (one boundary marker + one close).

**Why:** preflight (the authoritative recovery owner) and session-check use different parsers; session-check's complaint here did NOT reflect real committed-state corruption. The error **survives every standard recovery** — `reset --from-current --preserve-session`, moving `.agent-doc/{crdt,captures,state/cycles}/<pathhash>` aside — so it is not a stale CRDT/live-buffer replay either.

**How to apply:** When session-check errors on markers but (a) `grep -cF '<!-- agent:exchange'` / `'<!-- /agent:exchange'` both equal 1, (b) `git show HEAD:FILE` markers are balanced and ordered, and (c) `preflight` reports the doc clean + committed + no_changes — treat it as a session-check parser anomaly, NOT corruption. The work is safe in git; **STOP and report** (don't keep resetting/force-disking, which risks the clean committed state). It's an `#agent-doc-bug` to file from an agent-doc session (not from the project session — scope discipline [[feedback_session_document_scope_discipline]]). Likely clears on an IDE/session restart. Related recovery patterns: [[feedback_post_commit_worktree_drift_checkout_head_recovery]], [[feedback_supervisor_crdt_replay_reset_from_current]].
