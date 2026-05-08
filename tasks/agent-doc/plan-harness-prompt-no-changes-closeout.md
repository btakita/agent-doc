# Plan — Harness Prompt Lost When Preflight Reports No Document Changes

**Pending:** `#harnessprompt`
**Status:** Draft

## Problem

A direct Codex invocation can carry an actionable prompt even when the session
document itself has no new markdown diff. In this repro, the user sent:

```text
agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
```

The binary preflight correctly reported `no_changes: true` for the document
snapshot, but the harness turn still had a live user directive. The assistant
stopped with "No document changes were detected" and did not append a response
through `agent-doc finalize`.

The follow-up prompt made the loss visible:

```text
You missed my prompt. #agent-doc-bug. Please do [#tigerfault]. spec-test-build-install-commit-push
```

## Goal

Make harness-native `agent-doc <FILE>` turns fail closed when the document diff
is empty but the harness prompt contains actionable text that is not yet present
in the document exchange.

## Root Cause To Prove

The current Codex/direct-exec path treats `preflight.no_changes` as sufficient
to end the turn. That is correct for a pure document-watcher cycle, but it is
not sufficient for a harness-native entrypoint where the chat message itself can
contain the user directive.

The toolchain needs one explicit ownership rule for this mixed input case:

- if the prompt is only the bare `agent-doc <FILE>` entrypoint, `no_changes` can
  end the cycle after `session-check`
- if the same chat message includes extra directive text, or the current
  harness prompt is the actionable source, the cycle must persist a response or
  first inject/route that prompt into the exchange through a binary-owned path

## Implementation Steps

- [ ] Add a Codex/direct-exec regression for a bare `agent-doc <FILE>` prompt
      with `preflight.no_changes=true`; it should still stop cleanly after
      `session-check`.
- [ ] Add a regression for a harness prompt that includes an actionable directive
      while the document diff is empty; it must not silently report "nothing
      changed."
- [ ] Extend the harness invocation runbook and generated Codex skill text to
      distinguish "bare entrypoint" from "entrypoint plus directive."
- [ ] Decide whether the fix belongs in the harness wrapper, `agent-doc
      preflight`, or a small `agent-doc` subcommand that records harness-only
      prompt text into the exchange before planning.
- [ ] Ensure `agent-doc plan` returns an execution contract that prevents manual
      closeout when the harness prompt is the only source of actionable work.
- [ ] Add a `session-check` or finalize-time guard that catches a just-ended
      harness cycle whose live prompt was never written to the exchange.

## Files Likely To Change

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/planning.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/skill.rs`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/tests/`

## Exit Criteria

- A bare no-op `agent-doc <FILE>` invocation can still close without creating
  noise in the session document.
- A harness-only actionable prompt cannot be lost behind
  `preflight.no_changes=true`.
- The final closeout path either writes a response through `finalize` or fails
  with a diagnostic naming the unpersisted harness prompt.
