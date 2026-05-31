# Plan: closeout repair should converge in one path instead of causing repair churn

## Problem

Live Codex run on 2026-05-30 against
`src/boost-client/tasks/monsterrodholders.md` completed the requested repo/API
work, but closeout required a long sequence of `agent-doc` repair commands:

1. `agent-doc write --commit` with no response body marked `#rodkwfix` done but
   failed because the open exchange cycle had no assistant response body.
2. A raw response piped to `agent-doc write --commit` was treated as unmatched
   template content and rejected as a late fallback patch.
3. `agent-doc finalize` and then `agent-doc write --commit --template` repeated
   the same fallback path; one attempt committed escaped patch directives into
   the live exchange instead of applying them.
4. Manual exchange edits were needed to turn the visible closeout into a
   `### Re:` response block.
5. Only after `agent-doc write --commit` absorbed that direct patchback did
   `session-check` pass.
6. A final `agent-doc commit` was still required to update the outer
   superproject submodule pointer.

The requested work did eventually close cleanly, but the operator had to run
multiple repair commands, inspect live document state, patch response structure
manually, and commit nested parent pointers. That is too much recovery churn for
a common Codex closeout failure.

## Existing related bugs

- `#finalize-retry-ipc-response-duplication` covers duplicate response bodies
  and `❯`-prefixed assistant prose after repeated finalize/write retries.
- `#duplicate-response-after-commit` covers late IPC re-inserting an already
  committed response.
- `#codex-final-response-live-verify` covers missing final response body
  detection.
- `#codex-owned-pane-prompt-miss` covers recursive same-pane invocation missing
  prompts.

This item is narrower: once the missing-response/direct-patchback condition is
detected, the binary should offer and execute a single convergent recovery path
instead of asking the operator through several partially-valid repair commands.

## Desired invariant

For a template-mode document whose repo side effects are already committed but
whose assistant closeout body is missing or directly patched:

- `agent-doc write --commit <FILE>` or `agent-doc finalize <FILE>` must either
  converge the document in one command or fail before committing anything.
- Raw response text without `<!-- patch:exchange -->` must not be escaped and
  committed into `agent:exchange` as literal `&lt;!-- agent:... --&gt;`.
- Once a visible `### Re:` response exists after the active prompt, the recovery
  should absorb it directly without requiring manual boundary surgery.
- Nested parent submodule pointer repair should be part of the same successful
  closeout or produce one exact follow-up command before success is reported.

## Proposed fix

1. Add a closeout recovery state machine shared by `finalize`, `write --commit`,
   `repair`, and `commit`.
   - classify states as `missing_response_body`, `direct_response_patchback`,
     `escaped_template_patch`, `boundary_only_drift`, `reaped_item_pending_parent`,
     and `nested_parent_pointer_stale`.
   - every state gets exactly one allowed transition and a stable diagnostic.

2. Make raw-template fallback fail before commit.
   - In template mode, if stdin contains `<!-- agent:status -->` or
     `<!-- agent:exchange -->` instead of supported `<!-- patch:* -->` blocks,
     reject it before snapshot save / commit.
   - The diagnostic should say: use `<!-- patch:exchange --> ...` or rerun
     `agent-doc write --commit <FILE>` to absorb an already-visible response.

3. Add direct-patchback absorption for visible `### Re:` closeouts.
   - If `session-check` detects direct response patchback and the response block
     is after the active prompt with no unresolved prompt after it, `write
     --commit` should normalize boundary placement and commit it.
   - Do not require the operator to manually add/remove boundaries.

4. Make reaped-item and parent-pointer follow-up atomic.
   - If `--done` reaps an item and side-effect repos/submodules changed, the
     same successful closeout should update every required parent pointer.
   - If a pointer cannot be committed, fail before marking the cycle committed.

5. Improve diagnostics.
   - Replace the current chain of “try finalize / write --commit / commit” with
     one typed recovery instruction including the state name and exact command.
   - If the typed command would be invalid because of the current state, do not
     commit partial repairs.

## Tests

1. Template doc with a prompt and repo side effect committed, no response body:
   `write --commit` fails once with `missing_response_body` and no new commit.
2. Same doc with a visible `### Re:` block directly patched after the prompt:
   `write --commit` absorbs it, places one boundary, commits, and
   `session-check` passes.
3. Stdin containing escaped/unsupported `agent:exchange` patch directives in
   template mode fails before commit and leaves HEAD unchanged.
4. Reaped backlog item plus nested submodule pointer requires and completes all
   parent pointer commits before `session-check` reports success.
5. Re-running the recovery command after success is a no-op: no new repair
   commits, no boundary churn, no done-archive duplication.

## Landed (typed recovery classifier, 2026-05-30)

- `flow::closeout::CloseoutRecoveryState` enum + `classify_closeout_recovery_state`
  + `recovery_command` instruction table: one classified state → one recovery
  command, replacing the static recovery hint. Classified today: `clean`,
  `open_cycle`, `missing_response_body` (no-capture committed response turn, or a
  captured body absent from HEAD via `stuck_captured_cycle`), and
  `escaped_template_patch` (escaped `<!-- agent: -->` markers in the committed
  exchange).
- `session_check::closeout_recovery_hint` now renders `Recovery [<state>]:
  <command>` for all nine closeout-guard call sites at once (was a single static
  line). Spec: `specs/07-closeout-commands.md`.
- Unit tests: enum→instruction mapping; classifier for clean/open-cycle/
  missing-response/response-in-HEAD. `make check` green; FlowCore hot-path token
  budget untouched (the enum lives in `flow/closeout.rs`, not a budgeted file).

## Remaining (deferred)

- Precise detection wiring for the reserved enum states
  `direct_response_patchback`, `boundary_only_drift`, and
  `nested_parent_pointer_stale` (the mechanisms exist; the classifier currently
  returns `clean` for them rather than guessing).
- Items 2–5 of the proposed fix (raw-template fallback fail-before-commit,
  direct-patchback absorption, atomic reaped-item/parent-pointer) are already
  substantially covered by `PatchbackShape::EscapedComponentMarkers`,
  `write --commit` absorption, and `complete_required_closeout`; remaining work
  is folding their diagnostics through the typed classifier.

## Acceptance

- The monsterrodholders closeout shape can be reduced to one deterministic
  recovery command after the missing response is visible.
- No escaped template directives can enter a committed exchange.
- `agent-doc session-check` reports one actionable state instead of causing a
  multi-command repair loop.
- Existing duplicate-response and prompt-prefix regressions remain covered by
  their own items; this fix only owns convergence and diagnostics for the
  recovery path.
