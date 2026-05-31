# Plan — opt-in stash-session TTL prune (`#stash-session-ttl-prune`)

## Problem

`resync.rs` step 8 (`purge_unregistered_stash_panes_*`) only reaps panes that are
**unregistered + no owned file + no live supervisor**. Live registered /
supervisor-backed agent-doc sessions are correctly never auto-killed — which is
why ~9 `agent-loop` agent-doc panes accumulate in the shared session-0 `stash`
window over a long working session. The user wants an **optional** hygiene knob:
reap agent-doc stash panes whose session has been **idle in the stash beyond a
configurable TTL**.

This is **destructive** — it kills live, registered, supervisor-backed panes that
are merely idle. So it must be **opt-in (default off)** and **live-verified before
enabling**.

## Why this needs a design decision (not a blind code change)

1. **Idle signal.** `started` (registry) is *start time*, not last-activity — a
   long-running but active session has an old `started`. Using it would reap
   active work. The correct idle signal is **last activity**: tmux
   `#{window_activity}` (epoch secs) for the pane's window, or a supervisor
   heartbeat timestamp. This query does not exist in resync today and must be
   added.
2. **Config home.** TTL prune is **project-level** hygiene, not per-document, so
   the knob belongs in `.agent-doc/config.toml` (e.g. `stash_session_ttl_secs`),
   not document frontmatter. Confirm before wiring.
3. **Rollout safety.** Recommend a **report-only first** increment: when the TTL
   is configured, resync *logs* which panes it WOULD reap (ops_log) without
   killing, so the operator can confirm the idle detection on their real layout
   before a later flag enables the kill.

## Decision core (this cycle — safe, non-destructive)

Landed the pure, deterministic decision the rollout will reuse:
`stash_ttl_prune_candidate(idle_secs, ttl_secs, is_active_pane, is_agent_doc_stash_pane) -> bool`
with conservative guards:
- TTL must be configured (`ttl_secs > 0`); `0`/unset disables (returns false).
- Only an agent-doc pane parked in the `stash` window is eligible.
- The active/visible pane is NEVER reaped regardless of idle time.
- `idle_secs` must strictly exceed `ttl_secs`.

Plus `stash_ttl_prune_targets(...)` to filter a candidate list. Unit-tested.

## Remaining (gated)

1. **Idle signal wiring.** Add a tmux `#{window_activity}` query for stash-window
   panes; compute `idle_secs = now - window_activity`.
2. **Config knob.** `.agent-doc/config.toml` `stash_session_ttl_secs` (default
   unset = disabled). Plumb into `resync --fix` step 8.
3. **Report-only first.** When configured, log ops_log
   `stash_ttl_prune_candidate pane=… idle_secs=… ttl=…` without killing.
4. **Destructive enable behind an explicit flag** (e.g. `--ttl-prune` or a second
   config `stash_session_ttl_prune_enabled`), default off.
5. **Live verification** on the real ~9-pane agent-loop stash before enabling the
   kill. Add a SimWorld/tmux-ci regression for the decision + a live replay.

## Non-goals

- Do not change the existing orphan reap semantics (unregistered + no-owned-file
  + no-supervisor) — registered live sessions stay protected by default.
- Do not enable destructive reaping by default, ever.
