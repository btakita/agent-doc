# Plan - fail closed on direct invocation preflight hangs

**Pending:** `#preflighthang`
**Status:** Implemented

## Problem

Running `agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md`
from inside the already active Codex-backed `tasks/root.md` session created a
new cycle and then hung after:

```text
Submitting to codex...
```

Observed evidence from the SessionShare root document run:

- direct process: `agent-doc .../tasks/root.md`
- parent process: the existing Codex child for `agent-doc start tasks/root.md`
- cycle: `cycle-1778376674007`
- persisted phase: `preflight_started`
- no response capture was written
- actor state later reported `ready`
- the direct invocation stayed silent for several minutes and required manual
  interrupt/kill

This is not just stale-cycle cleanup after a crash. The command itself remained
blocked after opening preflight, even though the managed actor had already been
marked ready and no later capture/commit could complete the direct invocation.

Related items:

- `#staleflt` covers stale empty `preflight_started` recovery on a later
  preflight invocation.
- `#root-empty-preflight` captured an earlier root-document empty preflight
  recovery test need.
- `#panehang` covers completed panes and startup routing misses.

This item covers the live blocking behavior: direct invocation must not hang
indefinitely after it opens a preflight cycle.

## Desired Behavior

1. A direct `agent-doc <file>` invocation that opens preflight must either:
   - observe response capture and closeout progress, or
   - time out with a precise diagnostic that names the stuck phase, cycle id,
     actor id, pane id, and suggested recovery command.
2. If the target document is already owned by the current Codex pane, agent-doc
   must detect recursive self-dispatch before waiting on its own child process.
3. A self-dispatch should choose one of these explicit outcomes:
   - handle the cycle inline without routing back into the same pane, or
   - fail closed with "recursive direct invocation would deadlock", or
   - route to a fresh/rebound pane only after proving it is not the current
     blocking process.
4. The command must leave a recoverable cycle state:
   - no open `preflight_started` cycle without a timeout/recovery reason
   - no silent no-op after `Submitting to codex...`
   - next invocation can repair, abandon, or retry with exact guidance

## Investigation Steps

- [ ] Reconstruct the call chain for the SessionShare repro:
  - direct command process
  - existing `agent-doc start tasks/root.md` supervisor
  - Codex child process
  - controller actor generation and pane binding
- [ ] Trace where direct invocation waits after `Submitting to codex...`.
- [ ] Determine whether route dispatch sent the prompt to the same pane that
      was blocked waiting for the direct invocation to return.
- [ ] Inspect cycle state writes for `cycle-1778376674007` and identify why no
      timeout or failure event was persisted.
- [ ] Compare with the older stale-preflight recovery path to avoid duplicating
      only the cleanup half of the fix.

## Implementation Steps

- [x] Add self-dispatch detection for `agent-doc <file>` when the target actor
      is the current harness/pane/process tree.
- [x] Add a bounded wait around post-preflight dispatch/capture progress.
- [x] Persist a terminal or recoverable timeout state when capture does not
      appear within the budget.
- [x] Surface a diagnostic that includes:
  - file path
  - cycle id
  - phase
  - actor generation
  - pane id
  - whether self-dispatch was detected
  - exact retry/restart command
- [x] Teach next preflight/session-check to recognize this timeout state and
      avoid opening another ambiguous empty cycle.

## Regression Coverage

- [x] Direct invocation from inside the same Codex-owned document does not hang.
- [x] Recursive self-dispatch is detected before waiting on the same pane.
- [x] Dispatch with no capture within the timeout persists a recoverable timeout
      event.
- [x] Later invocation sees the timed-out cycle and emits precise recovery
      guidance instead of silently starting another preflight.
- [x] Normal direct invocation from an outer shell still works and commits a
      response.

## Exit Criteria

- No `agent-doc <file>` command can stay indefinitely silent after
  `Submitting to codex...`.
- Empty `preflight_started` cycles caused by direct invocation hangs are
  classified by cause, not left as generic stale state.
- The SessionShare `tasks/root.md` recursive direct invocation repro becomes a
  deterministic test case.
