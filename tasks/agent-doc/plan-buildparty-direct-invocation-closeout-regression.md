# Plan — fix direct `agent-doc <FILE>` closeout regression after docs hardening

**Pending:** `#adinv2`
**Status:** Draft

## Problem

On May 2, 2026, a plain Codex prompt:

```text
agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/buildparty-investor-demo/dev.md
```

did not complete as a binary-owned `agent-doc` cycle even though the document was
edited and the assistant reported work as shipped.

Concrete evidence after the turn:

- `src/session-share/tasks/buildparty-investor-demo/dev.md` was edited directly
- supporting repo files under `src/session-share/buildparty-investor-demo/` were
  edited directly
- the terminal answer described the work as done, but it was not closed out
  through the `agent-doc` binary workflow
- `agent-doc session-check .../dev.md` now reports:

```text
[session-check] INTERRUPTED: cycle `cycle-1777708207821` is `committed`
(commit_success), but the document still has unresolved prompt-bearing user
changes with no new agent-doc cycle started
```

This is a live regression after `#adcmt` already tightened the documented
harness-native closeout rule. The docs/rules changed, but the direct Codex
behavior still fell back to "treat this like a normal coding request, patch
files manually, and stop."

## Why This Is Separate From Older Closeout Bugs

- `#adcmt` covered the instruction contract and made the rule explicit.
- `#closeout-bypass` / `claudescore-3` focused on post-turn closeout bypass once
  a binary-owned cycle had already been in play.
- This repro shows a direct `agent-doc <FILE>` invocation can still miss the
  binary-owned cycle entirely in live Codex usage, even after the rule was
  documented and tested in agent-doc's bundled skill/runbook output.

## Goal

Make a direct harness-native `agent-doc <FILE>` invocation fail closed unless it
either:

1. enters a real binary-owned response cycle and finishes with
   `finalize` / `write --commit` + `session-check`, or
2. explicitly reports that the turn was not completed as an `agent-doc` cycle.

The agent must not be able to perform useful repo/document edits, emit a normal
success answer, and leave the target session doc in the
`committed previous cycle + unresolved prompt-bearing changes` state.

## Root Cause To Prove

At least one enforcement layer is still too soft in live Codex execution:

1. the plain `agent-doc <FILE>` prompt is still semantically interpreted as a
   general work request instead of an operational command to enter the binary
   workflow
2. the runtime has no hard guard that detects "you edited the target
   agent-doc document manually during this turn without reopening closeout"
3. `session-check` can diagnose the bad state after the fact, but there is no
   same-turn enforcement/backstop that prevents the false-success answer

## Implementation Steps

- [ ] Capture this BuildParty repro as the live post-`#adcmt` reference case,
      including:
  - the original direct prompt
  - the target document path
  - the `session-check` interrupted message
  - the fact that repo edits and session-doc edits both landed
- [ ] Audit the actual live Codex-side instruction/hook path used for plain
      `agent-doc <FILE>` prompts and compare it with the already-tested bundled
      skill/runbook text from `#adcmt`.
- [ ] Prove whether the miss is:
  - instruction-delivery drift
  - instruction-following drift
  - missing runtime enforcement after manual target-doc mutation
- [ ] Add a hard same-turn guard for the direct invocation path:
  - if the target `agent-doc` document changed during the turn outside a
    binary-owned cycle, the turn must not report success
  - the closeout path must reopen or fail closed before final answer
- [ ] Decide the narrowest reliable enforcement point:
  - Codex hook / stop-hook
  - direct invocation wrapper
  - session-check / finalize backstop surfaced before final success
  - explicit target-doc mutation detector in the agent-doc runtime
- [ ] Add regression coverage for the exact BuildParty shape:
  - direct `agent-doc <FILE>` invocation
  - useful manual edits land in the document/repo
  - no new binary-owned cycle is started
  - the run must fail closed or reopen closeout instead of surfacing success
- [ ] Keep the shared-harness rule explicit:
  - Codex plain-message invocation and Claude `/agent-doc` skill invocation must
    both remain binary-owned closeout paths
  - this repro is Codex-specific evidence, not a Codex-only invariant

## Files Likely To Change

- `src/agent-doc/src/skill.rs`
- `src/agent-doc/src/codex_hook.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/SPEC.md`
- possibly the Codex-facing installed skill/output generation tests added for
  `#adcmt`

## Exit Criteria

- Repeating the BuildParty direct invocation cannot end with a normal success
  answer while `session-check` still reports unresolved prompt-bearing changes
  with no new cycle.
- If the binary-owned closeout path was skipped, the run fails closed and says
  so explicitly.
- Regression coverage proves the post-`#adcmt` instruction contract is enforced
  in live direct invocation behavior, not only documented in generated skill
  text.
