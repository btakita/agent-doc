---
title: Fix monsterrodholders closeout drift that blocks sync and claim recovery
created: 2026-05-02
status: open
---

# Goal

Make `src/boost-client/tasks/monsterrodholders.md` fail closed with one deterministic repair path when the document contains an out-of-band or malformed response patchback, instead of letting passive sync, claim, and routed reopen churn through `registered_pane_missing` / `registry_rebind` while the pane registry itself is still correct.

# Evidence

- `src/boost-client/.agent-doc/sessions.json` currently binds `monsterrodholders.md` to pane `%85` in window `@84`, so the registry is not the immediate problem.
- `src/boost-client/.agent-doc/logs/ops.log` records `route_dispatch_only_sent file=tasks/monsterrodholders.md pane=%85 harness=codex` immediately followed by `preflight_blocked_uncommitted_closeout_drift`.
- The session log `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` shows repeated `sync_missing_pane_closeout_recovery_failed ... ERR: pending/backlog patch changed non-list content` before later `registered_pane_missing` / `registry_rebind` churn.
- The live document contains a malformed assistant patchback shape around the failed-order review section: a prefixed `❯ ### Re:` line plus a duplicate `### Re: ... (HEAD)` block, which matches the `session-check` direct-response / uncommitted-closeout drift family.

# Steps

1. Reconstruct the exact write / repair / session-check failure chain for the current `monsterrodholders.md` state, including which detector classifies the visible `❯ ### Re:` + duplicate `HEAD` response as uncommitted closeout drift instead of a recoverable replay.
2. Decide the invariant for sync- and claim-triggered recovery when the registry still points at a live pane but the document is already in closeout drift:
   - do not keep rebinding or replacing panes
   - surface a deterministic repair-needed result
   - preserve the current owner pane until the document state is repaired
3. Audit the normalization boundary in `write`, `repair`, and `session-check` for this mixed shape:
   - partially normalized backlog mutation
   - direct response patchback markers in exchange
   - committed-vs-snapshot drift in a submodule-owned session doc
4. Add regression coverage for a `monsterrodholders`-style template document that:
   - contains the visible malformed exchange tail
   - has a healthy registry/live pane binding
   - is hit by passive sync, claim-follow-up reopen, and routed reopen
   - now fails once with a precise repair instruction instead of pane churn
5. Re-run the live `monsterrodholders.md` selection / claim repro after the fix and verify the tmux pane is either rescued cleanly or the user gets one stable repair error without pane replacement.

# Exit Criteria

- `monsterrodholders.md` no longer falls into repeated `registered_pane_missing` / `registry_rebind` churn when the real problem is document closeout drift.
- Passive sync, claim, and routed reopen preserve the existing pane binding until the document repair path completes.
- The user-facing error points directly at the needed `write --commit` / repair step when the document is poisoned.
- Regression coverage locks the behavior in for submodule-owned template session docs.
