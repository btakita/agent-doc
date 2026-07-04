---
name: feedback_finalize_from_wrong_pane_binding_during_fleet_recycle
description: "Claude session pane bound to a DIFFERENT doc + target doc mid-fleet-recycle → response lands in live buffer, leave captured; don't force-commit"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 00b6590b-fcea-4c80-bd1e-b85fe430a02c
---

When the Claude Code session's execution pane (`$TMUX_PANE`) is bound to a
DIFFERENT agent-doc document than the one you're asked to drive, `finalize`
fails `pane ownership mismatch: owned by pane %X, current pane is %Y`. `claim`
from your pane won't rebind (your pane is already claimed by its own doc) — it
just provisions a new pane for the target.

**Route as the doc's owner pane instead of stealing yours:** run the command
with `TMUX_PANE=%owner agent-doc finalize ...` (owner from `admin inspect`).
Do NOT `claim --force --pane %yourpane` — that steals your pane from its own
doc and contaminates that session.

`--force-disk` is WRONG when a live editor holds the buffer: verify with
`ps -p <pid>` (a broad `ps|grep idea` can miss it) before assuming a stale
endpoint. If `idea`/editor is alive, the visible-buffer guard is correct;
force-disk would desync the operator buffer.

**During a fleet install/recycle (a pane titled "Install and recycle" is live)
with a `stale_install` preflight warning**, the IPC finalize hits
`invariant=live_prompt_drift_after_preflight` and baseline drift recurs even
right after `reset --from-current --preserve-session` (concurrent mutation +
version skew). The response still DELIVERS to the live editor buffer
(grep it — one copy) and is retained as a pending patch
(`.agent-doc/pending/<dochash>.md`) — the designed "retry-retained" state.

**Root cause is usually STALE pane bindings, not a live conflict.** Check
`sessions.json` + live tmux: if the doc's recorded owner pane is now a bare
`zsh` (its session exited) AND your live pane carries a stale binding to a
DIFFERENT doc that isn't live anywhere, both records are leftovers. A still-open
JetBrains buffer keeps the dead owner from being reaped.

**Correct fix (clean, not just self-heal):** reclaim the doc to YOUR live pane
with `agent-doc claim --pane %<yours> --force <FILE>` — it overwrites the stale
binding, removes the stale registry entry, and reaps stale live-buffer
sidecars. Then `agent-doc write --commit <FILE>` lands the captured response in
ONE copy (pane now matches, no TMUX_PANE spoof needed). Verified: reclaim +
write --commit committed exactly one copy. Only fall back to "leave captured /
self-heal" if you genuinely cannot reclaim (a real live session owns the pane).
Do NOT `admin recycle` for the drift (this is mid-cycle churn, not staleness —
see
[[feedback_live_prompt_drift_wedge_is_midcycle_recycle_not_staleness]]), do NOT
keep re-piping (boundary accumulation), do NOT force-disk against a live
buffer. Verify the ACTUAL work is committed (non-doc git commits are
pane-independent and safe) and report honestly that the doc-record is captured
but its git-commit is pending fleet settle. Related:
[[feedback_check_admin_detect_for_pane_contamination_first]],
[[feedback_hung_pending_add_recover_via_agent_doc_commit]].
