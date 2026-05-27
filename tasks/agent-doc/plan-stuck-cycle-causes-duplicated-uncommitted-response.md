# Plan: #agent-doc-bug stuck cycle causes duplicated + uncommitted response

## Problem

A single `agent-doc finalize` cycle can wedge into a state where:

1. The cycle is recorded in `.agent-doc/captures/<doc>/cycle-<id>.json` with
   `state=captured` and the full `response_body` saved.
2. Every subsequent `agent-doc finalize <FILE>` and
   `agent-doc write --commit <FILE>` against that cycle reports
   `[write] rejecting late fallback patch: cycle <id> already committed` and
   then `[commit] staged snapshot already matches HEAD ... closing cycle as
   already committed`, yet the document body in HEAD does not contain the
   captured response.
3. `agent-doc repair <FILE>` does not unstick the cycle. It returns success
   without applying the captured response and prints the same
   `[commit] detected post-commit local drift ... HEAD already contains the
   committed response` message.
4. The agent recovers only by composing a **different** response body so a new
   cycle handle is allocated and a fresh capture/finalize path runs end to
   end.

After the eventual successful close, the stuck capture is still on disk. The
next `agent-doc preflight <FILE>` finds it as an "orphaned response" and
re-applies it to the document, producing a **duplicate** `### Re: ...` block
in the exchange — the same response now appears twice, and the working tree
diverges from HEAD without any open cycle the agent can close cleanly. The
preflight error in that state is:

```
Error: snapshot differs from HEAD without an open or recoverable agent-doc
cycle (...); tracked side-effect edits: ... Use `agent-doc write --commit
<FILE>` once the visible response body is final, then re-run
`agent-doc session-check <FILE>`.
```

This is the failure mode the user labeled "response was duplicated and
uncommitted." Both halves of the description are accurate: it stays
uncommitted in the wedged cycle, and the eventual recovery produces a
duplicated response after the next preflight.

Observed timeline (single user prompt `do [#gdbprephot]`,
`tasks/software/tsift.md`, 2026-05-24 18:21–18:47 EDT):

- `cycle-1779661282039` captured the long-form response (5.2 KB) but never
  reached `committed`. Repeated finalize/write/repair calls all reported
  "already committed".
- A second composition with a shorter body produced
  `cycle-1779662795717` (3.9 KB), which finalized cleanly via
  `[main b532ae4fc]`.
- The next `agent-doc preflight tasks/software/tsift.md` invocation re-applied
  the orphaned capture, inserting a second copy of the response after the
  newly committed one and leaving the working tree dirty without an open
  cycle.

This is distinct from `#agent-doc-bugs2 plan-empty-response-repair-write`
(empty stdin repair path) and from
`plan-cancelled-stop-hook-exchange-replay` (cancelled cycles). The trigger
here is a captured but never-committed cycle whose snapshot/HEAD comparison
short-circuits commit while the response body still has not been written.

## Fix

1. `agent-doc write --commit` and `agent-doc finalize` must distinguish
   "cycle was committed and the response is present in HEAD" from "cycle was
   marked committed but the captured response body is not present in HEAD".
   When the captured body's hash is not found in HEAD, refuse the
   "already-committed" short-circuit and force a fresh patch application
   instead of dropping the body as a "late fallback patch".
2. After a successful different-cycle finalize for the same prompt
   (`prompt_target` / `queue_head` match), stale captures whose response body
   shares prompt scope must be marked
   `state=superseded_by=<new_cycle_id>` so the next `agent-doc preflight`
   does not re-apply them as orphans. Provide an explicit
   `agent-doc captures prune --prompt <prompt-hash>` for manual cleanup.
3. `agent-doc preflight` step 1 (`repair`) must check whether an orphaned
   response body already appears in the document HEAD by content hash, in
   addition to by snapshot. If the body is already present, mark the capture
   `state=already_committed` and skip re-application rather than appending a
   duplicate.
4. When `agent-doc repair` cannot apply a captured body because of the
   already-committed snapshot/HEAD short-circuit, exit non-zero with a clear
   diagnostic so the agent does not believe the cycle is healthy.
5. Surface stuck-cycle state in `agent-doc preflight` JSON as a first-class
   warning code (e.g., `stuck_captured_cycle`) with the captured cycle id,
   captured body byte length, and a recovery command, so the agent can take a
   recovery path instead of retrying the same finalize.

## Tests

1. Force a finalize cycle to wedge by simulating the
   "snapshot matches HEAD but captured body is absent" state for a recorded
   capture. Assert that subsequent `agent-doc write --commit` applies the
   captured response and reaches `committed` instead of reporting "late
   fallback patch".
2. After a successful different-cycle finalize that lands the same prompt
   response, run `agent-doc preflight <FILE>` and assert no orphaned-response
   repair fires and no duplicate `### Re: ...` block is appended.
3. Verify `agent-doc preflight --json` returns a `warnings[].code =
   "stuck_captured_cycle"` entry when a captured-but-uncommitted cycle is
   present, including the captured cycle id and the
   recovery-command string.
4. `agent-doc repair` against a wedged cycle exits non-zero with a stuck-cycle
   diagnostic; once unstuck, a follow-up `agent-doc repair` exits zero.
5. `agent-doc captures prune --prompt <hash>` removes only the captures
   matching the given prompt hash and leaves other captures intact.
