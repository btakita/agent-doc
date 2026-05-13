# Plan: direct chat must write session responses back before success

## Problem

A direct Codex chat response about a session document can satisfy the user in
chat while leaving the session document unresolved. The user then has to ask for
manual repair: "The response was not written to the document. #agent-doc-bug.
Write + commit the response to the document."

Current evidence:

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Prompt left in document: `Are the false positives fixed now?`
- The prior `agent-doc <file>` run failed before response write because the
  required-SSH guard falsely matched historical capture text.
- The assistant answered the bug in chat, but did not write the response into
  `tasks/monsterrodholders.md`.
- Manual repair used `agent-doc write --commit` and created submodule commit
  `5f245b0`, but parent pointer closeout initially failed on root
  `.git/index.lock`.
- A follow-up `agent-doc commit <file>` was required to create parent commit
  `4d648fba6`, after which `agent-doc session-check <file>` passed.

## Desired Behavior

1. When the active work is tied to a session document, direct chat closeout must
   either write the response through `agent-doc write --commit <file>` or state
   that the user requested chat-only output.
2. If `write --commit` succeeds in the submodule but parent pointer closeout
   fails, the turn must not report success until the parent boundary is
   completed or the blocker is reported.
3. The recovery guidance should be idempotent and should not require rerunning
   the original site/ops task.
4. `session-check` should remain the final guard and should name the missing
   layer: document repo, parent submodule pointer, or both.

## Investigation Steps

- [ ] Reconstruct the Monster Rod Holders false-positive-status repair:
      failed `agent-doc` child run, chat-only response, manual
      `agent-doc write --commit`, root index-lock failure, and final
      `agent-doc commit` recovery.
- [ ] Identify whether direct chat can infer the active session document from
      the latest `agent-doc <file>` command or whether it needs explicit user
      mention.
- [ ] Compare this with existing `#rspcmt5` evidence in
      `plan-response-patchback-uncommitted.md`.
- [ ] Decide whether this becomes a guard in the harness instructions,
      `session-check`, Codex hook state, or an explicit direct-chat wrapper.

## Implementation Steps

- [ ] Add an instruction/hook-level invariant: after a direct chat response that
      refers to a session-document prompt, run `agent-doc write --commit <file>`
      before final success.
- [ ] Make the required closeout command resilient to parent index locks by
      failing with a clear retry path and preserving the already-written
      response capture.
- [ ] Add a recovery helper or session-check hint that can finish only the parent
      pointer layer after the submodule response commit already exists.
- [ ] Avoid duplicate patchback if the user requests the repair after a partial
      write already crossed the submodule commit boundary.

## Regression Coverage

- [ ] Simulate direct chat response to a prompt-only session document tail and
      assert success is blocked until `write --commit` records the response.
- [ ] Simulate submodule write success plus parent index-lock failure and assert
      `session-check` reports the missing parent pointer.
- [ ] Simulate idempotent recovery with `agent-doc commit <file>` after the
      response is already committed in the submodule.
- [ ] Verify no duplicate response block appears after recovery.

## Exit Criteria

- A session-document prompt cannot be answered only in chat while the document
  remains unresolved.
- Partial response closeout across submodule/parent boundaries is visible,
  recoverable, and never reported as complete until `session-check` passes.
