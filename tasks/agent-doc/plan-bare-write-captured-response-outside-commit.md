# Plan: bare write captures response but strands closeout outside commit

## Symptom

During the `src/boost-client/tasks/monsterrodholders.md` closeout for
`#seocrawl`, the operator attempted to repair the response with:

```bash
agent-doc write --done seocrawl tasks/monsterrodholders.md
```

The command preserved the response body in the live document, but then failed:

```text
Error: bare `agent-doc write` preserved the response body, but the session
closeout is still outside the commit boundary

[session-check] INTERRUPTED: cycle `cycle-1780108562498` is still
`response_captured` ... response was captured but no write/commit followed
```

The operator then had to manually inspect the diff, patch the queue/status
cleanup, commit `tasks/monsterrodholders.md`, and push. The response was
eventually committed (`boost-client` commit `c13c258`), but not by the
binary-owned write/commit boundary.

## Why This Is A Bug

`agent-doc write` performed the dangerous half of repair - it made the
assistant response visible - and then refused to finish the commit boundary.
That violates the closeout invariant in the root `AGENTS.md`: visible response
content is not complete until it crosses `finalize` or `write --commit`.

The diagnostic also left an ambiguous recovery path:

- the response body was already in the live buffer
- the cycle remained `response_captured`
- the command did not auto-resume commit
- the operator had to manually commit a session document, bypassing the normal
  agent-doc closeout path

## Likely Root Cause

The template writer accepted a zero-patch / unmatched-content repair path:

```text
WARNING: 0 template patches found
synthesizing exchange patch for unmatched content
socket IPC reported already_applied
```

After IPC reported `already_applied`, `write` treated the response as preserved
but did not transition into the same commit path as `write --commit` or
`finalize`. `session-check` then correctly detected that the cycle was still
`response_captured`, but the command had no deterministic continuation.

This is adjacent to the broader response-patchback-left-uncommitted family, but
the narrow bug is: **a repair write that mutates/preserves the response body must
either commit it or abort before making the body visible.**

## Fix Plan

1. Add a regression fixture for the exact state:
   - template-mode document
   - existing live-buffer drift / IPC `already_applied`
   - response body preserved by bare `agent-doc write`
   - cycle still recorded as `response_captured`
2. Change `agent-doc write` recovery semantics:
   - if a response body is preserved or synthesized, require the same
     write+commit boundary as `write --commit`
   - or fail before applying/preserving the response body
3. Treat `already_applied` IPC acknowledgements as proof of content placement,
   not proof of closeout. The next step must still be snapshot/commit/session
   check.
4. Add a resumable recovery command for this state:
   - detect `response_captured` plus matching live response body
   - finish commit without re-appending or duplicating the response
   - preserve pending mutations such as `--done`
5. Tighten diagnostics:
   - print one concrete recovery command that can succeed
   - avoid recommending bare `agent-doc write` when the body is already present
   - distinguish "body preserved, commit missing" from "body missing"
6. Update commit/runbook guidance only after the executable regression passes.

## Acceptance Criteria

- A zero-template-patch repair cannot leave a visible assistant response
  uncommitted while returning control to the agent.
- `agent-doc write --done <id> <file>` in the reproduced state either commits
  the document or fails without mutating the document.
- A matching `response_captured` state with body already present can be repaired
  idempotently without duplicate response insertion.
- The diagnostic names the owning repo and the exact missing closeout step.

## Non-goals

- Do not implement the fix in this planning turn.
- Do not broaden this plan to all IPC duplicate/deletion corruption. This plan
  only covers the write/commit closeout gap after the response body is already
  present.
