---
title: Prevent monsterrodholders reroutes from closing the previously running tmux pane
created: 2026-05-03
status: open
---

# Goal

Make `agent-doc` preserve the existing live `src/boost-client/tasks/monsterrodholders.md` tmux pane during reroute, restart, or owner-handoff paths unless the old pane is proven duplicate/dead and the replacement is explicit, logged, and regression-tested.

# Problem

New report from May 3, 2026:

- `agent-doc` closed the previously running tmux pane for `monsterrodholders.md`
- the failure is destructive, not just a stale focus/layout alias

This is adjacent to, but not the same as, the current validation-only items:

- `#mrreplay` validates the earlier backlog-normalization/repair replay path
- `#mrbusy` validates the same-document busy-pane optimistic reopen path

This new bug is about pane-retention and owner handoff. Once a reroute/restart path decides to continue work, it must not silently reap the prior live pane unless agent-doc can prove that pane is the disposable duplicate.

# Evidence To Collect

- the exact trigger shape:
  - editor `Run Agent Doc`
  - `agent-doc route`
  - `agent-doc sync`
  - restart/reopen after `startup-miss`
- pane id and window membership before the close
- `ops.log` and session-log events around the close:
  - `route_dispatch_only_sent`
  - `startup-miss`
  - `registered_pane_missing`
  - `session_end`
  - `registry_rebind`
  - any duplicate-pane reap / `respawn-pane -k` / `kill-pane` path
- whether the new pane claimed the same document/session UUID or started a fresh session era

# Steps

1. Reconstruct the `monsterrodholders.md` repro from logs and current code paths so the exact pane-closing transition is known.
2. Distinguish which class of destructive handoff occurred:
   - duplicate-pane cleanup
   - missing-pane recovery
   - restart/reopen respawn
   - cross-root/session retargeting
   - plugin-triggered sync/layout repair
3. Tighten the invariant for same-document reroutes:
   - if the old pane is still alive and still plausibly owns the document, fail closed instead of closing it
   - if a replacement pane is created, preserve the old pane until ownership proof and successful handoff are recorded
4. Audit the destructive operations boundary in `route`, `sync`, `resync`, and related tmux helpers:
   - no `kill-pane` / `respawn-pane -k` / rebind-over-live-owner path without explicit proof
   - duplicate-pane reap must prove the disposable pane is not the authoritative visible owner
5. Add diagnostics that state exactly why a prior pane was closed, which subsystem initiated it, and what evidence justified that action.
6. Add regression coverage for a `monsterrodholders`-style same-document reroute where:
   - an old pane is still alive
   - a reopen/restart path is attempted
   - the system either reuses the old pane or preserves it until the replacement is definitively authoritative

# Exit Criteria

- rerouting or restarting `monsterrodholders.md` no longer closes the previously running pane from weak or ambiguous evidence
- any pane closure now occurs only through a narrow explicit allowlist with provenance in logs
- live validation shows the old pane is preserved or intentionally superseded only after successful handoff
- regression coverage locks the behavior in for same-document reroute/restart flows
