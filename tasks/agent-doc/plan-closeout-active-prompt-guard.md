# Plan — Active Prompt Closeout Guard

**Pending:** `#closeguard`
**Status:** Proposed

## Problem

`agent-doc-bugs2.md` reached a closeout boundary while the next prompt-bearing
exchange line was still unresolved:

```text
do [#pcops]. spec-test-build-install-commit-push
```

The same working tree also showed a malformed live backlog line:

```text
_- [ ] [#pcops] ...
```

Preflight recovered a stale `preflight_started` cycle on the next run, but the
user-visible effect was still a bad closeout: the document looked closed while
the intended backlog task had not been executed and the pending item shape was
damaged.

## Goal

Prevent a session document from closing successfully when a prompt-bearing
exchange tail remains unresolved or when the matching pending item is malformed
at the closeout boundary.

## Implementation

1. Extend `session-check` to treat prompt-bearing exchange lines after the last
   committed assistant boundary as hard interruptions, even when the previous
   cycle state was repaired or marked already committed.
2. Add a pending-shape validation pass for live `agent:backlog` items before
   accepting `--pending-done`, so malformed variants such as `_-
   [ ] [#id] ...` are surfaced as repairable document corruption instead of
   silently escaping the done guard.
3. Teach preflight repair to distinguish "previous response is committed" from
   "new prompt exists after that response"; only the former may close the old
   cycle as already committed.
4. Add regressions that recreate this document shape: committed response,
   appended `do [#id]` prompt, malformed matching backlog item, and a following
   closeout attempt.

## Verification

- Unit test for `session-check` interrupting on a post-boundary `do [#id]`
  prompt after stale cycle repair.
- Unit test for pending-shape validation rejecting malformed checklist prefixes.
- Integration test using `agent-doc finalize` or `agent-doc write --commit` to
  prove the closeout fails before the response is persisted.

## Exit Criteria

- A committed response cannot hide a newer unresolved prompt-bearing exchange
  line.
- A malformed live pending item cannot bypass the `pending_done_guard`.
- The recovery diagnostic names the exact prompt target and pending line that
  need repair.
