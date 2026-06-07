# Plan: JetBrains Run Agent Doc Stall Regression

**Pending:** `#jbrunstallreg` in `tasks/agent-doc/agent-doc-bugs2.md`

## Trigger

User report on June 3, 2026:

> JB `Run Agent Doc` stalled. #agent-doc-bug

This is a recurrence after the current Unreleased work that says JetBrains
`Run Agent Doc` now has a 75-second route-process watchdog and visible warning
for typing-debounce timeouts.

## Evidence

- `src/agent-doc/VERSIONS.md` already documents the intended fix:
  plugin-spawned route subprocesses should be bounded, stale processes should
  be terminated, and pre-route typing debounce timeouts should be visible.
- `src/agent-doc/editors/jetbrains/SPEC.md` requires:
  - repeated Run cancels stale route subprocesses
  - route subprocesses have a bounded watchdog
  - route failure output is persisted under
    `.agent-doc/state/editor-route-errors/`
  - typing debounce timeout is visible and does not route
- Current JetBrains code:
  - `SubmitAction.actionPerformed` waits on `TypingTracker.awaitIdle`, then
    schedules `FileDocumentManager.saveAllDocuments()` and
    `TerminalUtil.sendToTerminal(...)` on the EDT.
  - `TerminalUtil.sendToTerminal` persists failures only after the route
    subprocess starts and returns or times out.
  - `runRouteProcessWithTimeout` bounds the route process after launch.
- Recent logs show repeated debounce timeouts around the report window:
  - `debug.log.2026-06-03` has preflight debounce timeout entries for
    `tasks/agent-doc/agent-doc-bugs2.md` at `03:07:30` and `03:07:55`
    (`mtime not settled`, waited about 3002ms).

## Working Hypothesis

The watchdog covers only the spawned `agent-doc route` subprocess. A manual
Run can still appear stalled before the watchdog exists, or without a durable
operator-facing artifact, in at least these stages:

1. Waiting for editor typing idle (`TypingTracker.awaitIdle`).
2. Scheduling and running document save on the EDT.
3. Resolving cwd/binary/layout arguments before process launch.
4. Launching the route process.
5. Route process timeout or failure notification/persistence.

If one of the pre-launch stages blocks, throws, or returns silently, the user
sees the same symptom as the old route-process stall: the action was invoked,
but no useful feedback appears.

## Fix Plan

1. Add a plugin-side Run Agent Doc stage ledger.
   - Record one durable attempt file per route key under
     `.agent-doc/state/editor-route-attempts/`.
   - Include cwd, relative path, focused file, command when available, stage,
     started_at, elapsed_ms, plugin version, thread name, and last error.
   - Stages should include at minimum:
     `clicked`, `await_typing_idle`, `typing_idle_timeout`,
     `save_scheduled`, `save_started`, `save_completed`,
     `route_build_started`, `route_process_started`, `route_timed_out`,
     `route_failed`, `route_queued`, and `route_succeeded`.

2. Bound and surface the pre-route stages.
   - Wrap the `SubmitAction` background thread in a top-level
     `try/catch/finally` that logs and notifies on unexpected failures.
   - Add a visible persistent warning when `TypingTracker.awaitIdle` returns
     false, and also write the attempt ledger before returning.
   - Add a timeout/diagnostic around the EDT save/route handoff so a stuck
     save cannot look like a silent Run action.
   - Register prompt polling only after route launch is attempted, and record
     that decision in the ledger.

3. Keep repeated Run semantics immediate.
   - Preserve `InFlightRouteRegistry.replace(...)` for route subprocesses.
   - Add similar supersession state to the pre-route attempt so a new click can
     mark the prior attempt superseded before it reaches process launch.
   - Ensure stale pre-route attempts do not later register polling or surface
     stale errors after a newer Run has started.

4. Persist diagnostic artifacts for every non-success path.
   - Route subprocess failures continue to use
     `.agent-doc/state/editor-route-errors/`.
   - Pre-route failures/timeouts should either use the same persistent route
     error surface or link to the new stage ledger from the notification.
   - Notifications must include copy/open actions where the existing route
     error path supports them.

5. Add tests before implementation.
   - Kotlin tests for `SubmitAction` proving:
     - typing timeout writes an attempt/failure artifact and notifies
     - exceptions before route launch are caught and surfaced
     - EDT save handoff cannot silently disappear
     - a second click supersedes a prior pre-route attempt
   - Kotlin tests for `TerminalUtil` proving route process stage transitions
     are written for success, timeout, queue, busy-running, and failure.
   - Keep existing watchdog tests for the route subprocess.

6. Live verification.
   - Install the built JetBrains plugin.
   - Reproduce a typing-idle timeout by keeping the markdown file mtime moving
     or typing continuously, then invoke `Run Agent Doc`; confirm a visible
     warning plus durable artifact.
   - Reproduce a busy/queued route and a route timeout; confirm the IDE does
     not appear stalled and the persisted artifact identifies the exact stage.

## Acceptance

- A `Run Agent Doc` click always produces one of:
  - a route subprocess start with normal success/failure handling
  - a visible pre-route warning/error
  - a durable superseded-attempt record when a later click replaces it
- No stage before route process launch can fail silently.
- The operator can inspect or copy the last attempt stage after the notification
  disappears.
- Repeated Run clicks remain immediate and do not wait behind stale plugin-local
  route or pre-route work.
