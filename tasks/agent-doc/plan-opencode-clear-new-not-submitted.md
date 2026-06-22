# Plan — OpenCode `/new` not submitted by JB Clear Session Context

**Related:** New bug 2026-06-17; prior work `#7az7`, `#jbclearctxsubmit`, `#jbacceptwedge`, `#img-describe-toolcall`
**Status:** Pending operator input (screenshot `tasks/agent-doc/img_46.png` not viewable by non-vision models)

## Problem

Operator reports: JB `Clear Session Context` on OpenCode-backed sessions does not submit `/new` (screenshot: `tasks/agent-doc/img_46.png`, attached to queue head in `tasks/agent-doc/agent-doc-bugs2.md`).

## Investigation (this cycle)

- `harness_clear_command("opencode") == "/new"` (test `harness_clear_command_maps_opencode_to_new`); Claude/Codex/unknown keep `/clear`.
- `TmuxSubmitProfile::pending_draft_enter_resubmit()` is `pub const fn … -> bool { true }` — the bare-Enter retry is **not** Codex/Claude-only, despite the stale comment near `src/session_actor_cmd.rs:1894`. OpenCode benefits from the same retry.
- `prompt_requests_clear()` already accepts both `/clear` and `/new` (`codex_hook.rs:1448`).
- Current session `b26b9957-8de6-4eae-bb98-5d7fe4d6b781` (pane `%12`) is actually running OpenCode (`tmux capture-pane` shows `Build · GLM-5.2`, `ctrl+p commands`); supervisor socket `.agent-doc/supervisor/b26b9957-….sock` exists.
- `detect_document_harness_in` reads `frontmatter.agent` first; the document's frontmatter parses cleanly to `agent: opencode` (verified by `serde_yaml`/Python).
- Historical ops.log entries showing `harness=codex` for this file (e.g. `[1781575288] session_clear_queue_cooldown … harness=codex`, `[1781583813] idle_queue_watch_context_reset … cmd="/clear"`) are from older session generations before the document switched to `agent: opencode`. They are **not** current evidence — pane `%12` is OpenCode today.
- Could not view `img_46.png` (text-only model — see `#img-describe-toolcall`).

## Hypotheses (need operator input to disambiguate)

| # | Hypothesis | Diagnostic grep in `.agent-doc/logs/ops.log` around the click | Code path |
|---|------------|--------------------------------------------------------------|-----------|
| H1 | Clear was clicked mid-turn under active `agent:queue auto` loop → clear deferred as no-op, `/new` never sent | `session_clear_queue_preempt_deferred` or `session_clear deferred … alive-busy under an active \`agent:queue auto\` loop` | `src/session_actor_cmd.rs:848-861` (`reconcile_idle_projection_before_clear` → `ClearPreflightOutcome::DeferredPreempt`) |
| H2 | `/new` was sent to the OpenCode composer but the Enter-submit did not fire or OpenCode did not consume it | `session_clear_submit_observation … command_visible=true` followed (or not) by `session_clear_submit_resubmit action=submit_key key=Enter` and `session_clear_submit_blocked … command=/new` | `src/session_actor_cmd.rs:1657-1703` (`verify_clear_submit_after_delivery`), `poll_clear_direct_submit_acceptance` |
| H3 | Supervisor IPC `Clear` returned ok but the child-pane (PTY path) didn't deliver `/new` to the OpenCode TUI | `supervisor.ipc_clear` and `transform_event … normalize_lf_to_cr`; check whether supervisor used `inject_pane` (tmux send-keys) vs `inject_writer` (PTY write) | `agent-doc-orchestration/src/start/supervisor_io.rs:12-51,120-135` |

## Preconditions

- `#jbacceptwedge` `[operator-install]` gate: operator installs signed plugin `0.2.170` (`editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.170-signed.zip`) and restarts IntelliJ. Without the new per-connection-thread IPC accept loop, concurrent IPC may wedge and confound Clear verification.
- Operator describes `img_46.png` (text of any error / state shown in the OpenCode pane) OR re-runs Clear after the plugin install and reports which of H1/H2/H3 the new ops.log entries match.

## Fix steps (after hypothesis confirmed)

1. **H1 (busy-pane defer):** surface a `ui_outcome=blocked_with_exact_unblocker` line on `DeferredPreempt` so JB shows "Clear deferred — pane busy under active queue; retry at idle or use interrupt-clear" instead of a silent no-op. Add SimWorld coverage for `agent:queue auto` + Clear mid-turn.
2. **H2 (Enter not firing):** extend the OpenCode submit-acceptance heuristic — OpenCode's `/new` palette row (`clear_command_visible_detects_opencode_new_palette_row`) may need a different submit key sequence than a single Enter on the composer. Add a live-tmux regression under `make tmux-ci`.
3. **H3 (PTY delivery):** require supervisor `ipc_clear` to prove deliver (`inject_pane` preferred when available; PTY write must show the byte count actually flushed). Add regression in `start/supervisor_io.rs`.
4. **Stale comment cleanup (do alongside any of the above):** the comment near `src/session_actor_cmd.rs:1894` says "Codex/Claude only" but `pending_draft_enter_resubmit()` is now const `true`; correct the comment.

## Verification

- `cargo test -p agent-doc session_actor_cmd`
- `cargo test -p agent-doc clear_submit_proof`
- `make check` after the targeted fix lands
- Live JB repro on this opencode session once `0.2.170` is installed; grep ops.log for the four `session_clear_*` markers

## Scope this cycle

- Capture-only. No code changes — screenshot evidence unavailable to non-vision model; `#jbacceptwedge` `[operator-install]` blocks definitive IPC-layer verification.
