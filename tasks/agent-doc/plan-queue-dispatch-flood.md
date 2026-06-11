# Plan: queue dispatch floods the running harness session with repeated triggers

## Symptom (operator reports, 2026-06-11)

- "As the queue runs, I'm getting `agent-doc tasks/agent-doc/agent-doc-bugs2.md`
  prompts submitted into the running Claude Code session. I have to manually
  delete the prompt from the Claude Code session queue."
- "`/agent-doc tasks/agent-doc/agent-doc-bugs2.md` accumulated into the Claude
  Code session queue as I was typing during finalize."
- "The lazily-rs.md session running Codex reproduced the error." → **cross-harness**,
  not Claude-Code- or Codex-specific.

Net effect: while a `go` queue drains, the dispatcher re-submits the harness
trigger (`agent-doc <FILE>` / `/agent-doc <FILE>`) into the *already-running*
agent pane, and the submissions pile up in the harness input queue. The operator
has to manually delete them. Reproduced on at least two documents/harnesses.

## New evidence (2026-06-11, after pause)

- The dispatch is the **controller `dispatch` command** (`handle_dispatch` in
  `project_controller.rs`). The operator saw it re-fire *while typing this prompt*
  and get correctly blocked by the pause:
  `project controller command dispatch failed: dispatch blocked ...
  failed_stage=queue_paused`. So the pause gate works; the bug is *what keeps
  invoking `dispatch`*.
- "I typed in an unrelated queue item into this queue and the queue stalled" — an
  unrelated queue edit perturbs in-flight dispatch state (related stall mode).
- "The queue did auto-start as I was typing" — the dispatch is triggered by live
  editor/file-change events (save while typing), not only by the Claude-Code
  `/loop`. This points at the route auto-start / sync / file-watch → controller
  `dispatch` chain firing per file change, with no pane-busy gate or trigger
  dedupe.

## Why this is not a safe blind fix

The flood spans multiple dispatch entry points (route auto-start on file change,
supervisor/idle continuation, `/loop`), all funneling into controller
`dispatch`. Pinning the exact offending invocation needs a **live ops.log repro**
(operator-drive) — which path logs the redundant `dispatch` while the pane is
mid-turn. A blind pane-busy/dedupe gate risks suppressing *legitimate* dispatch
or worsening the stall. Do not change dispatch concurrency without that repro.

## Suspected root cause (to confirm via live repro)

The queue-continuation dispatcher (controller idle-queue-watch / route) is not
treating an actively-running turn in the owned pane as "busy" before submitting
the next trigger. Instead of waiting for the current turn to reach idle, it
re-sends the routed trigger on each tick, so triggers accumulate in the harness
composer while the agent is mid-turn (and especially during `finalize`, when the
pane is busy committing and the operator is also typing).

Open questions to answer first (do NOT fix blind):
1. Is the source the Claude-Code `/loop` auto-loop, the controller idle-queue
   watch, the route `--dispatch-only` path, or the supervisor IPC inject?
   Grep `.agent-doc/logs/ops.log` for `idle_queue_watch_drain`,
   `route_submit_observation`, `route_dispatch_*`, and `codex_stop_queue_*`
   around a reproduction.
2. Does the dispatcher check live pane busy/idle state before submitting? Where
   is the busy gate (or its absence) — `route.rs` direct-pane path,
   `supervisor` idle watch, or `queue_dispatch.rs`?
3. Why does it re-submit rather than dedupe an already-pending identical trigger
   still visible in the composer? (`recent_lines_contain_trigger` already
   detects a still-visible trigger for submit-acceptance — the dispatcher could
   reuse that to skip re-submit.)

## Fix direction (after confirmation)

- Gate queue-continuation dispatch on proven pane idle: do not submit the next
  trigger while the owned pane shows an active turn. Defer until idle.
- Dedupe: if the routed trigger is already visible/pending in the composer, skip
  re-submission instead of stacking another copy.
- Emit proof: `queue_dispatch_skipped reason=pane_busy|trigger_already_pending`
  so the operator's repro is provable from `ops.log`.

## Mitigation applied this turn

`agent-doc admin queue pause tasks/agent-doc/agent-doc-bugs2.md` (controller
receipt_id=5) — halts further queue dispatch for this document so the flooding
stops while the fix is designed. Resume with `agent-doc admin queue resume` once
fixed.

## Verification (operator-drive live gate)

Cross-harness live repro is operator-drive. After the fix: resume the queue, let
it drain, confirm `ops.log` shows the dispatcher waiting for idle / skipping
already-pending triggers and the harness composer is NOT flooded.

## Files (likely)

- `agent-doc-orchestration/src/queue_dispatch.rs`, `route.rs` (dispatch + busy gate)
- supervisor idle-queue watch (`supervisor/` + `start.rs`)
- specs `07-session-tmux-commands.md` / `08a-session-actor-contract.md`
