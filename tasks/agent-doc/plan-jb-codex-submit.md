# Plan: JB `Run Agent Doc` on Codex does not submit the routed prompt

## Symptom (operator report, 2026-06-11)

JB `Run Agent Doc` action against a Codex pane enters the routed prompt into the
Codex composer but does **not** submit it. The operator had to press Enter
manually to start the queue.

## Root cause (diagnosis)

The JB action routes through `agent-doc route` → direct-pane dispatch for Codex
(Codex is not owned-PTY managed; only `claude`/`opencode` get `OwnedPty`, see
`tmux_router::strategy_for_harness`). The Codex direct-pane submit path is
`route.rs::send_command_once_unchecked` →
`sessions::send_submitted_text_for_harness` →
`tmux_router::submit_text_for_harness` → `Tmux::send_keys`, which sends the
trigger text + `\r` as **one** `send-keys -H` hex payload
(`CARRIAGE_RETURN_SEQUENCE`).

`route.rs::send_command_unchecked` (route.rs:6564) already **detects** the
non-submit: it polls the pane capture and, when the trigger text is still
visible after the acceptance timeout, logs
`route_submit_issue ... issue=prompt_not_submitted`
(`RouteSubmitObservation::TriggerStillVisible`, route.rs:6624). But it then just
returns `CommandDispatchStatus::TimedOut` — **there is no re-submit fallback**.
So the prompt stays drafted in the Codex composer exactly as the operator
described.

Hypothesis for *why* the merged `\r` does not submit on Codex: the Codex TUI
composer treats a trailing carriage return that arrives in the same byte stream
as the pasted text as a literal newline (multi-line draft) rather than a submit
key. (The single-payload merge in `Tmux::send_keys` was added to fix the inverse
"drafted but not submitted when text and Enter are separate events" problem for
other managed composers — Codex appears to want the opposite.)

## Fix approach

1. **Re-submit fallback in `send_command_unchecked`** — when direct-pane
   acceptance detects `TriggerStillVisible` for a Codex pane, send a *separate*
   bare `Enter` key event (`sessions::send_key(tmux, pane, "Enter")`), then
   re-poll the acceptance check once. This is the minimal, harness-scoped fix
   and matches the "Enter as a separate key event" hypothesis.
   - Gate on harness == codex (do not change claude/opencode behavior).
   - Bound it: at most one re-submit attempt, then fall through to the existing
     `TimedOut` result so we never loop.
2. **Proof logging** — emit `route_submit_resubmit file=... pane=... harness=codex
   action=enter_key result=accepted|still_visible` so the operator's live test is
   provable/disprovable from `ops.log` without a live editor on the agent side.
3. **Deterministic coverage** — add a SimWorld / route test asserting that a
   Codex direct-pane dispatch whose first submit leaves the trigger visible
   issues exactly one `Enter` re-submit and records the resubmit proof line.

## Verification (operator-drive live gate)

Agent cannot reproduce the live Codex TUI submit behavior. After the fix lands
(`make check` + `cargo build --release` + `cargo install --path .` + `agent-doc
lib-install`), the operator runs JB `Run Agent Doc` against a Codex pane; the
agent confirms from `ops.log`:
- `route_submit_issue ... issue=prompt_not_submitted` (first attempt), then
- `route_submit_resubmit ... action=enter_key result=accepted`, and
- the queue actually starts.

## Files

- `agent-doc-orchestration/src/route.rs` (`send_command_unchecked`, resubmit + proof)
- `agent-doc-orchestration/src/route/tests.rs` (deterministic coverage)
- `src/sim_world.rs` (SimWorld coverage if modeled there)
- specs `07-session-tmux-commands.md` / `08a-session-actor-contract.md` (submit-proof contract)
