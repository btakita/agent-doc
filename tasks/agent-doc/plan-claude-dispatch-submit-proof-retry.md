# Plan: Claude/OpenCode dispatch-only submit proof + bounded Enter retry

Backlog: `#claudesubmit`
Trigger: "JB `Run Agent Doc` with Claude did not submit on spoton.md. We should have the loop detect the unsubmitted prompt and Enter."

## Root cause (confirmed)

`agent-doc route --dispatch-only` sends the trigger text + `Enter` (submit key) to the
pane for all harnesses (`TmuxSubmitProfile::submit_key()` == `"Enter"`,
`sessions.rs:249`; harness arg is ignored). Submission *proof* then diverges by harness:

- **Codex** has a `UserPromptSubmit` hook (`.codex/hooks.json`) → the binary gets
  dispatch-start proof and fails once with `accepted_only_dispatch_start_proof` when the
  hook never fires.
- **Claude Code / OpenCode have no equivalent prompt-submit hook.**
  `dispatch_only_dispatch_start_proof_required(harness)` returns false for them, so
  `require_dispatch_only_dispatch_start_proof`
  (`agent-doc-orchestration/src/route/dispatch_only.rs:275-289`) classifies the result as
  `Accepted` and returns `Ok(())` immediately — the binary never learns whether `Enter`
  actually submitted, and never retries.

So when the Claude composer is unfocused / mid-turn / not dispatch-ready, the drafted
`agent-doc <file>` trigger sits unsubmitted and the run "does nothing" — exactly the
spoton.md report. Route ledgers only record `route_success` (pane-input acceptance), not
pane-level submission.

## Fix shape

Add a bounded submit-retry for no-hook harnesses in the `DirectPaneSubmit` dispatch-only
path, reusing the existing prompt-ready pane-capture machinery (the same capture used at
`dispatch_only.rs:180-212` to detect a dispatch-ready composer):

1. After `dispatch_routed_reopen_with_mode` sends text + `Enter`, capture the pane after a
   short bounded delay.
2. If the pane still shows the drafted trigger text in the composer (prompt not consumed),
   re-send a single `Enter` (bounded retry — one retry, matching the Codex clear-resubmit
   budget at `session_actor_cmd.rs:1854-1860`).
3. Emit an operator-greppable proof marker for the retry outcome, e.g.
   `route_dispatch_only_submit_retry file=… pane=… harness=claude attempt=2 result={accepted|still_unsubmitted}`.
4. Keep `proof_scope=accepted_only` semantics (do not pretend hook-level proof); the retry
   only raises confidence that the prompt landed, it does not synthesize a hook proof.

## Scope / files

- `src/agent-doc/agent-doc-orchestration/src/route/dispatch_only.rs` — add retry after
  `DirectPaneSubmit` dispatch when `!dispatch_only_dispatch_start_proof_required(harness)`.
- `src/agent-doc/agent-doc-orchestration/src/flow/routed_reopen.rs` — add the retry
  proof-line helper alongside `dispatch_only_submit_unproven` (`:374`).
- `src/agent-doc/agent-doc-orchestration/src/sessions.rs` — no change needed (submit key
  is already harness-agnostic).
- Deterministic SimWorld coverage for the "drafted trigger still visible → one Enter
  retry" path and the "retry exhausted → still accepted-only" path.
- Spec alignment: `specs/07-session-tmux-commands.md` proof-scope rule (Claude/OpenCode
  accepted-only) — extend to describe the bounded retry.

## Validation

- `cargo test -p agent-doc-orchestration` (new + existing dispatch-only proof tests).
- `make check`.
- `cargo install --path .`.
- Operator live test: JB `Run Agent Doc` on a Claude pane with an unfocused composer;
  confirm the retry submits and the proof marker appears in `.agent-doc/logs`.

## Non-goals

- Not adding a Claude Code prompt-submit hook (separate, larger effort).
- Not changing Codex behavior (it already has hook proof).
