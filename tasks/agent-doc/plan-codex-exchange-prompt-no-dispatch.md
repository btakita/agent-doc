# Plan: Codex `Run Agent Doc` ignores exchange-only prompts on a fresh session

**Bug id:** `#codex-exchange-prompt-no-dispatch`
**Source:** `tasks/agent-doc/agent-doc-bugs2.md` queue report (2026-06-08)

## Reproduction

1. Start a new agent-doc **Codex** session on a document.
2. Type a prompt into `agent:exchange`, then run JB **Run Agent Doc**.
   - **Observed:** nothing happens — no turn starts.
3. Type a prompt into `agent:queue` instead, then run JB **Run Agent Doc**.
   - **Observed:** the turn starts.

The dispatch path treats an exchange-only prompt and a queue prompt asymmetrically: only the queue write reliably starts a turn.

## Root-cause hypothesis

Two independent detection signals drive `route` dispatch, and only the queue one is robust on a fresh session:

- **Queue path** keys off queue activation (`fm.queue_active == Some(true)` at `route.rs:2121`, plus the queue-trigger/activation resolution in preflight). This signal is independent of the snapshot — a queue write activates and dispatches even before any cycle snapshot exists.
- **Exchange path** keys off `pending_prompt_bearing_context_for_route` (`route.rs:7727`, called from the dispatch-only resolver at `route.rs:5012` / `5362`), which delegates to `session_check::first_unstarted_prompt_bearing_change` (`session_check.rs:4649`).

`first_unstarted_prompt_bearing_change` has two gates that make an exchange prompt invisible on a fresh Codex session:

1. **Snapshot-gated** — `session_check.rs:4652` returns `None` when no snapshot exists. A brand-new session has no committed cycle snapshot yet, or route just snapshotted the file so snapshot == file (no diff). Either way the exchange prompt is not seen as "unstarted."
2. **Queue stripped** — `strip_queue_components_for_unstarted_prompt_guard` (`session_check.rs:4707`) removes `agent:queue` content before diffing, so the function is deliberately exchange-scoped; queue dispatch must (and does) come from the separate activation signal.

Net effect: a queue write has a snapshot-independent activation path; an exchange write does not, so on a fresh session the exchange prompt produces "nothing happens."

## Investigation steps

1. Confirm the fresh-session snapshot state when JB **Run Agent Doc** fires for Codex: does a snapshot exist at dispatch time, and does it already equal the file (so `unified_diff_from_contents` returns `None`)? Instrument `first_unstarted_prompt_bearing_change` / route ops-log.
2. Trace the Codex fresh-start trigger: route creates the pane and launches `agent-doc <file>`; verify whether the first in-pane `preflight`/`run` sees the exchange prompt as a diff or reports `no_changes`.
3. Determine the correct baseline for a fresh session exchange prompt — HEAD content vs. an empty snapshot — so a newly typed exchange tail prompt is always a detectable unstarted prompt.

## Fix approach (to refine after investigation)

- Give the exchange path a fresh-session fallback equivalent to the queue activation signal: when no snapshot exists, diff the exchange against committed HEAD (or empty) instead of returning `None`, so a fresh exchange tail prompt is detected as unstarted.
- Keep the queue-strip behavior (queue keeps its own activation path); only fix the snapshot-absent / snapshot==file gap for exchange.
- Ensure parity across harnesses — the fix lives in `session_check.rs` / `route.rs` (binary-owned), not per-harness.

## Test coverage

- New `route` test: fresh Codex session, exchange-only tail prompt, no snapshot (or snapshot == HEAD) → `pending_prompt_bearing_context_for_route` returns a prompt-bearing context and route dispatches.
- New `session_check` test: `first_unstarted_prompt_bearing_change` detects a plain exchange tail prompt when the snapshot is absent or equals HEAD.
- Regression guard: queue-write dispatch still works and queue content is still stripped from the exchange detector.

## Files

- `agent-doc-orchestration/src/session_check.rs` — `first_unstarted_prompt_bearing_change` (snapshot gate, queue strip)
- `agent-doc-orchestration/src/route.rs` — `pending_prompt_bearing_context_for_route`, dispatch-only resolver, fresh-start wiring
- Specs: `specs/02-document-format.md` (§2.5 queue activation), `specs/08-session-routing.md`, `SPEC.md` dispatch rules
