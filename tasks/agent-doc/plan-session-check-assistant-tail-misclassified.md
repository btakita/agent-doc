# Plan: session-check should not classify assistant closeout tails as prompts

## Problem

After a successful `agent-doc finalize` response commit, `session-check` can
misclassify assistant-owned closeout/status prose at the end of a template
exchange response as an unresolved prompt-only tail. The observed closeout then
required a second artificial patchback that began with `Closeout repair:`,
polluting the transcript even though the preceding response was already an
assistant `### Re:` block.

## Evidence

- `tasks/agent-doc/agent-doc-bugs2.md` closeout on 2026-05-11 for
  `#startingdispatch`.
- The assistant response ended with status text about a push running after
  closeout.
- `finalize` wrote and committed the response, then `session-check` interrupted
  with a prompt-only-tail diagnostic for that assistant-owned sentence.
- A follow-up `agent-doc write --commit` patchback was required only to explain
  that the prior line was not a user prompt.

## Desired Behavior

- Assistant ownership inside a `### Re:` response block must continue until an
  explicit user prompt marker, component boundary, or new user-authored prompt
  run appears.
- Response-body status prose, even if imperative-looking or future-tense, must
  not be treated as an unresolved prompt target.
- Successful closeout should not require a transcript-cleanup patchback whose
  only purpose is to convince `session-check` that the prior assistant response
  was assistant-owned.

## Implementation Plan

- [ ] Reproduce the `#startingdispatch` response shape in a focused
      `session-check` or closeout regression.
- [ ] Audit the prompt-only exchange-tail scanner and identify where it loses
      assistant-block ownership near the final exchange boundary.
- [ ] Teach the scanner to preserve assistant ownership after `### Re:` /
      attributed response headings until a structural prompt boundary appears.
- [ ] Keep the existing fail-closed behavior for real prompt-only tails after a
      committed response.
- [ ] Add a regression proving no extra `Closeout repair:` patchback is needed
      for assistant-owned closeout/status prose.

## Exit Criteria

- The reproduced `#startingdispatch` closeout shape passes `session-check`
  without a second patchback.
- Real post-response user prompts still interrupt closeout.
- No transcript pollution is introduced by repair-only assistant explanations.
