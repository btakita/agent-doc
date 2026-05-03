# Plan — harness-native `agent-doc` invocation bypassed commit closeout

**Pending:** `#adcmt`
**Status:** Draft

## Problem

A direct Codex prompt of the form:

```text
agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/buildparty-investor-demo/dev.md
```

produced a visible session-doc response and supporting repo edits, but the turn
still ended without the required commit-boundary closeout. The assistant
reported success while explicitly stating that it did **not** commit or push.

That concrete repro happened in Codex, but the underlying failure mode is
broader: any harness-native `agent-doc` entrypoint can drift if the agent
treats the invocation like a generic "work this document" instruction, patches
files manually, and exits without `agent-doc finalize <FILE>` or `agent-doc
write --commit <FILE>`.

This is not the older Rust commit-boundary failure family where `agent-doc`
itself wrote the response and then failed before commit. In this repro family,
the binary never owned the closeout path at all.

## Goal

Make harness-native `agent-doc` invocation unambiguous and commit-safe:

- the harness must treat it as a binary-managed response cycle, not a loose
  manual editing task
- successful closeout must cross `finalize` or `write --commit`
- if the agent cannot do that, it must fail closed and say the response is not
  complete

## Reference Repro

- User invoked direct Codex form: `agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/buildparty-investor-demo/dev.md`
- Assistant edited:
  - `src/session-share/buildparty-investor-demo/...`
  - `src/session-share/news/2026-05-02/...`
  - `src/session-share/tasks/buildparty-investor-demo/dev.md`
- Final response then said: "I did not commit or push"
- The document workflow had asked for `spec-test-news-commit-push`, so the
  closeout semantics were wrong even though the file edits themselves were
  useful

Harness note:
- Codex is the concrete reference repro because its entrypoint is a plain
  `agent-doc <FILE>` message and it has Codex-specific hook backstops.
- Claude Code uses `/agent-doc <FILE>` via the Skill tool instead, but the same
  binary-owned closeout invariant still applies there.

## Root Cause To Prove

The current skill / harness guidance is still weak at one critical point: it
explains the correct `finalize` / `write --commit` contract, but it does not
reliably force agents to treat a harness-native `agent-doc` entrypoint as
"enter the binary-owned response cycle now, and do not claim completion until
the commit boundary is crossed."

That leaves room for a harmful fallback:

1. interpret the prompt semantically instead of operationally
2. edit the session doc and repo files directly
3. skip `finalize` / `session-check`
4. report a successful outcome with an uncommitted response

## Implementation Steps

- [ ] Document this exact BuildParty repro in the agent-doc bug tracker as the
      reference case for direct-invocation closeout drift.
- [ ] Audit the bundled skill / instruction surface that Codex receives:
      and the shared harness runbooks:
  - `src/agent-doc/src/skill.rs`
  - `src/agent-doc/runbooks/harness-invocation.md`
  - `src/agent-doc/runbooks/commit.md`
- [ ] Tighten the direct-invocation / skill-entry rule so harness-native
      `agent-doc` prompts are treated as executable binary workflow entry, not
      as a generic document-editing request.
- [ ] Make the closeout rule explicit and hard to miss:
  - finish implementation work
  - run `agent-doc finalize <FILE>` for the normal path
  - run `agent-doc session-check <FILE>` immediately after
  - do not report success if either step was skipped
- [ ] Keep the harness-specific distinctions explicit:
  - Codex: plain-message `agent-doc <FILE>`, Codex hooks, direct-exec
    `session-check` backstop
  - Claude Code: `/agent-doc <FILE>` Skill entrypoint, same binary-managed
    finalize / repair rule even without the Codex-specific hook path
- [ ] Add a direct anti-pattern to the skill text:
  - do **not** manually patch the final assistant response into the document
    and stop there
  - do **not** say "not committed" after a normal `agent-doc <FILE>` turn
    unless the user explicitly asked to leave it uncommitted
- [ ] Decide whether the skill should require a more explicit recovery message
      when the agent did manual repo work first and still needs a final
      `agent-doc write --commit` / `finalize` step.
- [ ] Add regression tests for the bundled skill text so future wording changes
      cannot remove:
  - direct Codex invocation semantics
  - Claude slash-invocation semantics
  - mandatory finalize/session-check closeout
  - the "do not claim success while uncommitted" rule
- [ ] If needed, add one higher-level integration or golden-output test for the
      generated harness-specific skill content that verifies each invocation
      path still includes the commit-boundary contract.
- [ ] Update the public runbook/docs only after the skill text and tests are in
      place.

## Files Likely To Change

- `src/agent-doc/src/skill.rs`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/runbooks/commit.md`
- `src/agent-doc/README.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- A harness-native `agent-doc` prompt no longer leaves room for a "manual patch
  + no finalize" interpretation.
- The bundled skill explicitly requires `finalize` / `write --commit` plus
  `session-check` before success is reported.
- Regression tests protect that instruction contract from future drift.
