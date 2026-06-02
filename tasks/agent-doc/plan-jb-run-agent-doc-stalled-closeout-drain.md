# Plan: JB `Run Agent Doc` stalled on closeout drain / busy wait

**Pending:** `#jbstalldrain` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Plan only. Do not implement in this item.

## Report

Operator report: "JB `Run Agent Doc` stalled. #agent-doc-bug"

Recent local evidence from 2026-06-02:

1. `11:57:57` EDT, IntelliJ `Run Agent Doc` on
   `tasks/agent-doc/agent-doc-bugs2.md` ran:

   ```text
   agent-doc route --dispatch-only --plain-trigger --debounce 0 --wait-for-ready 60 tasks/agent-doc/agent-doc-bugs2.md ...
   ```

   It failed quickly after route tried to drain an open closeout:

   ```text
   authoritative actor generation 218 ... owns pane %67 but route could not drain
   the active closeout before dispatch: [session-check] INTERRUPTED: open backlog
   item(s) exist only outside live agent:backlog: #queue-user-edit-overwrite
   ```

2. `11:58:27` EDT, another routed action for
   `src/boost-client/tasks/monsterrodholders.md` ended after the full busy wait:

   ```text
   authoritative actor generation 251 for tasks/monsterrodholders.md owns pane %61
   but dispatch-only route will not inject a new trigger because the authoritative
   actor is busy did not return to a dispatch-ready prompt in the current generation
   after waiting 60s.
   ```

3. The plugin did log the final failure, but while the 60 second wait was in
   progress the operator-facing behavior looked like a stall. The immediate
   closeout-drain blocker and the delayed busy-wait failure both surface as
   "Run Agent Doc did not start" rather than as a typed route state with an
   actionable next step.

## Relationship To Existing Plans

This is not a replacement for:

- `#snrun` / `tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md`,
  which targets Codex-specific routed readiness and accepted-but-unconsumed
  proof gaps.
- `#jb-run-agent-doc-busy-queue-dispatch-deadlock` /
  `tasks/agent-doc/plan-jb-run-agent-doc-busy-queue-dispatch-deadlock.md`,
  which targets prompt-bearing busy-queue dispatch.
- `#jberr` / `tasks/agent-doc/plan-jb-route-error-capture.md`, which targets
  persistent copyable failure text.

This plan covers the operator-visible stall shape across two route stages:
closeout-drain blockers before dispatch and long busy-authoritative-actor waits
before a final fail-closed result.

## Goal

JetBrains `Run Agent Doc` should never look inert while route is waiting or
blocked. It should quickly expose:

- document path
- authoritative pane and generation
- route stage
- whether route is waiting for an active turn, draining closeout, or blocked
- elapsed time / timeout
- exact recovery action or copyable error

## Fix Direction

1. Add typed route progress events for long waits.
   - Emit a structured "waiting for authoritative actor to become dispatch-ready"
     event at the start of the wait, with document, pane, generation, harness,
     timeout, and last live pane evidence.
   - Update the event periodically or at least once near timeout.
   - Clear the event on successful dispatch or terminal fail-closed result.

2. Make closeout-drain blockers first-class route results.
   - When route attempts to drain an open closeout and `session-check` blocks,
     classify the failure as `closeout_drain_blocked` instead of folding it into
     a generic route failure.
   - Preserve the exact blocker text (`#queue-user-edit-overwrite` in this repro).
   - Do not retry busy dispatch until the closeout blocker is resolved.

3. Add a lightweight persistent route-status sidecar for editor plugins.
   - Reuse or sit beside `.agent-doc/state/editor-route-errors/...`.
   - Store the current route stage while the subprocess is alive and the final
     route result after exit.
   - Ensure repeated `Run Agent Doc` supersedes the prior sidecar/process rather
     than leaving an old "still waiting" state.

4. Update the JetBrains action surface.
   - On long waits, show a non-success status such as "Waiting for active turn"
     with pane/generation details.
   - On closeout-drain blockers, show a persistent/copyable route error with the
     exact session-check blocker and recovery command.
   - Keep the plugin a thin wrapper: route classification remains binary-owned.

5. Add deterministic coverage.
   - Route unit/SimWorld case: active closeout drain reaches session-check
     backlog-shadow blocker and returns a typed `closeout_drain_blocked` result.
   - Route unit/SimWorld case: busy authoritative actor wait emits a progress
     sidecar/event before timeout and a terminal typed timeout after the budget.
   - Plugin test: `SubmitAction` / route runner maps in-progress, blocked, and
     failed route states to persistent Event Log entries without success wording.

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/ops_log.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/...`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

## Verification

- Reproduce the `agent-doc-bugs2.md` closeout-drain blocker and confirm JB shows
  a copyable `closeout_drain_blocked` error naming `#queue-user-edit-overwrite`.
- Reproduce the `monsterrodholders.md` busy-authoritative-actor wait and confirm
  JB surfaces "waiting for active turn" promptly, then either dispatches when the
  pane becomes ready or fails once with the typed 60s timeout.
- Confirm repeated `Run Agent Doc` supersedes the prior route attempt cleanly.
- Run the project verification suite and install the updated binary/plugin before
  live verification.

## Exit Criteria

- `Run Agent Doc` no longer looks stalled during a 60s route wait.
- Closeout-drain blockers are typed, copyable, and do not get retried as generic
  busy-route failures.
- The two 2026-06-02 repros either dispatch or fail with a stable, stage-specific
  IDE-visible status.
