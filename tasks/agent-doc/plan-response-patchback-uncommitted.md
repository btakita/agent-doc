# Plan: fail closed when response patchback remains uncommitted

## Problem

A session-document response patchback can be visible in the working tree while
the closeout path has not committed the owning repository state. The user then
has to notice the dirty patchback and ask for a manual commit.

Current evidence:

- SessionShare root turn: `src/session-share/tasks/root.md`
- User report: "The response patchback is uncommitted."
- Manual recovery command:
  `agent-doc commit /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md`
- Recovery output said the target document's staged snapshot already matched
  `HEAD`, but the parent submodule pointer was stale and had to be committed
  afterward.

This overlaps with older patchback/closeout drift plans, but the invariant here
is simpler: after a response patchback is reported complete, the owning repo
and any required parent submodule pointer must either be clean and committed or
the command must fail closed with an explicit recovery command.

## Desired Behavior

1. A successful agent-doc turn that writes a response patchback must also finish
   the commit boundary for:
   - the document's owning repository
   - parent repositories whose submodule pointers changed
2. If the closeout cannot commit any required layer, it must fail closed before
   reporting success.
3. `agent-doc session-check` must detect visible response patchbacks that lack a
   matching committed closeout and name the exact missing layer.
4. Running `agent-doc commit <file>` as recovery should be idempotent and should
   explain whether it committed:
   - target document content
   - snapshot repair only
   - parent submodule pointer only

## Investigation Steps

- [ ] Reconstruct the SessionShare root closeout chain that produced a visible
      response patchback but still needed a later manual commit.
- [ ] Identify whether the missing boundary was the target document commit,
      snapshot repair, parent submodule pointer commit, or editor-side working
      tree drift.
- [ ] Compare this shape with:
      `plan-preflight-started-patchback-closeout.md`,
      `plan-buildparty-write-stream-closeout-gap.md`, and
      `plan-claudescore-3-turn-complete-drift.md`.
- [ ] Decide whether this should become a new invariant in `session-check` or
      an extension of an existing closeout invariant.

## Implementation Steps

- [ ] Add a closeout finalizer check that verifies all required owning repos are
      clean or have been committed before success is returned.
- [ ] Teach submodule-owned document closeout to report parent pointer commits as
      first-class closeout work, not incidental post-processing.
- [ ] Add diagnostics that distinguish "target doc already committed" from
      "parent pointer still dirty."
- [ ] Add a recovery path that can safely finish only the missing layer without
      replaying or duplicating the assistant response.

## Regression Coverage

- [ ] Simulate a submodule-owned document response where the target repo commit
      succeeds but the parent pointer remains dirty; closeout must not report
      full success.
- [ ] Simulate a visible response patchback with no target repo commit; next
      `session-check` must fail closed and prescribe `agent-doc commit <file>`.
- [ ] Simulate idempotent recovery after the target snapshot already matches
      `HEAD`; recovery commits only the stale parent pointer and reports that
      precisely.

## Exit Criteria

- No agent-doc turn can leave a visible response patchback uncommitted while
  reporting success.
- Manual `agent-doc commit <file>` recovery remains safe, but should not be
  required after a normal successful response.

## OpenCode-Specific Evidence (#rspcmt4)

### Reproduction

- **Document:** `boost-client/tasks/monsterrodholders.md` v1.29 deploy
- **Session:** a9421282, GLM-5.1 agent
- **Harness:** OpenCode
- **Symptom:** Response was patched into the exchange section but agent-doc
  finalize/closeout did not commit the document. User had to manually request
  commit.
- **Same class as:** #rspcmt2, #rspcmt3

### Root Cause Analysis

The binary-level closeout path (`write.rs` finalize_commit, session_check.rs)
is harness-agnostic and correctly commits when invoked. The failure is at the
**instruction surface layer**:

1. **Missing session-check in OpenCode runbook** — The OpenCode section of
   `runbooks/harness-invocation.md` did not require running `agent-doc
   session-check <FILE>` after finalize (unlike the Codex section which has
   explicit session-check + fail-closed instructions).

2. **No Stop hook backstop** — Codex has `codex_hook.rs` Stop handler that
   auto-closes open cycles from `last_assistant_message`. OpenCode has no
   equivalent hook mechanism. If the agent process ends or the user interrupts
   before finalize commits, there is no automatic recovery.

3. **No active session tracking** — `detect_active_session_post_commit_drift()`
   in `session_check.rs:1165` uses `codex_hook::load_active_session_for_current_file()`
   which always returns `None` for OpenCode sessions because there is no OpenCode
   equivalent of `.agent-doc/codex-hooks/` session state.

4. **Codex-only error messages** — session-check error messages referenced "the
   active Codex session" and "let the Stop hook recover" which are misleading
   for OpenCode sessions.

### Instruction Surface Alignment

The shared `SKILL.md` (line 32) and `SPEC.md` (line 73) already mandate
session-check for "Codex/OpenCode/direct-exec" paths. The gap was that the
OpenCode-specific harness runbook section did not implement this requirement.

### Fixes Applied

1. `runbooks/harness-invocation.md` OpenCode section now requires:
   - `agent-doc session-check <FILE>` immediately after finalize
   - Fail-closed guard (nonzero check → continue recovery, do not report success)
   - Session-check after manual repair `write --commit` too

2. `runbooks/commit.md` updated to name "Codex and OpenCode" instead of just
   "Codex" for the direct-exec session-check requirement.

3. `README.md` updated from "Codex post-write guard" to "Direct-exec post-write
   guard" covering both Codex and OpenCode.

4. `session_check.rs` error messages changed from "active Codex session" and
   "Stop hook" to harness-agnostic "active harness session" and "hook" language.

### Remaining OpenCode Gaps (follow-up items)

- **No OpenCode hook backstop** — Unlike Codex, there is no `opencode_hook.rs`
  or equivalent Stop/UserPromptSubmit mechanism. If the OpenCode agent crashes
  mid-turn, recovery is manual.

- **No active session tracking** — `detect_active_session_post_commit_drift()`
  silently returns `None` for non-Codex sessions. A future OpenCode-specific
  session tracking mechanism could close this gap.

- **No OpenCode hook tests** — The codebase has extensive Codex hook tests but
  no equivalent for OpenCode lifecycle events.

## Direct Codex Chat Evidence (#rspcmt5)

### Reproduction

- **Document:** `src/session-share/tasks/root.md`
- **Harness:** Codex, normal chat turn rather than an `agent-doc <FILE>` child run
- **Prompt:** `commit-push`
- **Symptom:** The requested commit/push work completed and was reported in chat,
  but no response patchback was written to the session document. The user had to
  explicitly request: "The response patchback was not written to the document.
  #agent-doc-bug. write response to document. commit-push".
- **Same class as:** #rspcmt2, #rspcmt3, #rspcmt4

### Root Cause Hypothesis

The direct-chat preset path can complete repository commit/push work without
entering the binary-owned session-document write-back boundary. Existing
closeout guards focus on `agent-doc finalize`, `agent-doc write --commit`, and
harness child-agent runs; they do not force an ordinary chat response that
operates on a session document's preset to be mirrored into that document before
success is reported.

### Required Invariant

When a user prompt is a session-document preset or explicitly asks to "write
response to document", the turn is not complete until either:

1. the response is written with `agent-doc write --commit <FILE>` and
   `agent-doc session-check <FILE>` passes, or
2. the assistant explicitly reports that the user requested chat-only output and
   no document patchback was attempted.

### Regression Coverage

- Add a direct-chat/preset closeout test or harness simulation where a
  `commit-push` style prompt performs side-effect commits, then assert final
  success is blocked unless `agent-doc write --commit` records the response in
  the session document.
- Ensure the guard does not double-write when the turn already used
  `agent-doc finalize` or `agent-doc write --commit`.
- Ensure the recovery command is explicit and idempotent when the side-effect
  commits are already pushed but the session document lacks the response.

## OpenCode Response Patched But Not Committed (#rspcmt6)

### Reproduction

- **Document:** `boost-client/tasks/monsterrodholders.md` v1.32 deploy
- **Session:** a9421282, GLM-5.1 agent
- **Harness:** OpenCode
- **Symptom:** The diff-summary response after "Run the post docker commands." was
  written into the exchange section but `tasks/monsterrodholders.md` remains dirty
  (uncommitted patchback lines). Additionally, the v1.32 surgical-revert code changes
  in `src/monsterrodholders-dev/` (mrh-performance.php + test script) were also left
  uncommitted despite the status saying "awaiting spec-test-commit-push."
- **Same class as:** #rspcmt2, #rspcmt3, #rspcmt4, #rspcmt5

### Root Cause Analysis

Unlike #rspcmt4 (no session-check) and #noexchopencode2 (no patchback at all), in
#rspcmt6 the response WAS successfully patched into the exchange section. The failure
was that finalize/closeout never committed the document.

The existing guards could not catch this state:

1. **`prompt_only_exchange_tail` returns `None`** — the response IS in the exchange,
   so the tail has an assistant heading, not a prompt-only block.

2. **`detect_active_session_post_commit_drift` returns `None`** — it calls
   `crate::codex_hook::load_active_session_for_current_file()` which requires
   `CODEX_THREAD_ID` or `CODEX_SESSION_ID`. OpenCode sets neither, so it always
   returns `None` for non-Codex sessions. The drift goes undetected.

3. **`detect_unstarted_prompt_bearing_diff` may not fire** — the new exchange content
   looks like a legitimate assistant response, not a prompt-bearing user edit.

4. **`detect_bypassed_response_write` may not fire** — the response was patched into
   the exchange through a prior cycle's binary-owned path or direct edit.

The net effect: a committed cycle state + dirty working tree with exchange content
changes + no Codex session tracking = silent pass through all drift guards.

### Fix

Added `detect_uncommitted_exchange_drift()` in `session_check.rs` — a
harness-agnostic guard that compares the current working tree content against the
committed snapshot when the cycle state is `committed`. If the exchange component
has changed (not just status or other non-exchange components), it fails closed
regardless of harness.

This guard runs as a fallback after `detect_active_session_post_commit_drift` in all
three committed-cycle branches of `inspect_core`:

1. Cycle state exists and is committed (line ~640)
2. No ops.log event (line ~712)
3. Terminal ops.log event (line ~806)

### Regression Coverage

- `uncommitted_exchange_drift_detected_without_codex_session` — proves the new guard
  fires when a committed cycle has exchange content drift and no Codex session is
  active (the exact #rspcmt6 reproduction).
- `uncommitted_exchange_drift_ignored_when_only_status_changed` — proves the guard
  does NOT fire when only the status component changed (non-exchange drift is benign
  for this invariant).

## Repeated OpenCode Patchback Not Committed (#rspcmt8)

### Reproduction

- **Document:** `boost-client/tasks/monsterrodholders.md` shopcozi mobile CSS
  fix v0.4.9
- **Session:** a9421282, GLM-5.1 agent
- **Harness:** OpenCode
- **Symptom:** The response was visible in the exchange section, but
  `tasks/monsterrodholders.md` remained dirty and the user had to request
  "commit the document."
- **Same class as:** #rspcmt2, #rspcmt3, #rspcmt4, #rspcmt5, #rspcmt6, #rspcmt7

### Additional Fix

`detect_uncommitted_closeout_drift()` now reuses the harness-agnostic
`detect_uncommitted_exchange_drift()` guard for promptless exchange drift before
falling back to generic `snapshot != HEAD` checks. That closes the gap where an
OpenCode response body could be appended to `agent:exchange` without a fresh
response heading and still avoid the preflight/sync closeout-drift blocker until
a later manual commit, while preserving normal prompt-bearing user edits for the
next preflight diff.

### Regression Coverage

- `preflight_fails_closed_on_uncommitted_exchange_drift_without_response_heading`
  proves the next `agent-doc <FILE>` invocation fails closed when a prior
  OpenCode-style response body is visible in the exchange section but absent
  from the committed snapshot.

## Recursive Direct-Run Recovery Evidence (#rspcmt9)

### Reproduction

- **Document:** `boost-client/tasks/monsterrodholders.md` checkout incident
  harness status update
- **Session:** a9421282, Codex owner pane
- **Harness:** Codex direct invocation from the pane that already owned the
  document
- **Symptom:** `agent-doc <FILE>` tripped the recursive-direct-invocation guard.
  It committed the existing snapshot state, left the cycle recoverable, and left
  the visible response patchback dirty in the working tree. A later
  `agent-doc preflight <FILE>` repaired the stale `preflight_started` cycle but
  reported the snapshot already matched `HEAD`, then still left the visible
  status, exchange, and pending-item response patchback uncommitted.
- **Same class as:** #rspcmt2, #rspcmt3, #rspcmt4, #rspcmt5, #rspcmt6, #rspcmt8

### Root Cause Hypothesis

The stale-cycle recovery path distinguishes "snapshot already committed" from
"working tree contains later drift", but it does not recognize that the later
drift can be the response patchback for the interrupted cycle. Because the
snapshot matches `HEAD`, recovery can close the cycle as already committed even
though the user-visible response state is still absent from `HEAD`.

### Required Invariant

Recovery must not close a cycle as committed while a valid response patchback for
that cycle remains uncommitted in the working tree.

If the drift is a provable agent-doc response patchback, recovery should commit
it through the normal document commit path. If it cannot be proven safe, recovery
should fail closed with an explicit diagnostic such as
`response_patchback_uncommitted` instead of reporting an already-committed cycle.

### Implementation Outline

1. Extend stale-cycle recovery to compare `HEAD`, the snapshot, and the current
   working tree after the recursive direct-run guard fires.
2. Detect response-patchback-shaped drift in template components, including
   `agent:exchange`, `agent:status`, and pending-item mutations.
3. Refuse to classify the cycle as already committed while that drift exists
   unless it is either committed or explicitly marked as unsafe user drift.
4. Add a `session-check` failure for committed-cycle records whose document
   still has uncommitted response patchback drift.
5. Keep unrelated user edits protected: only auto-commit when the diff can be
   proven to come from the response patchback path for the active cycle.

### Regression Coverage

Add an integration regression that simulates the observed sequence:

1. A template session document has `HEAD` and snapshot aligned.
2. The cycle state is left at `preflight_started` by a recursive direct-run
   guard.
3. The working tree contains a valid response patchback that mutates status,
   exchange, and pending items.
4. `agent-doc preflight` or the recovery entrypoint runs.

Assert that recovery either commits the response patchback and leaves the
working tree clean, or fails with `response_patchback_uncommitted`. It must not
return an already-committed success while the response patchback remains
uncommitted.
