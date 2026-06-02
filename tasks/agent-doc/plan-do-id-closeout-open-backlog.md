# Plan: `do [#id]` closeout must not leave the target open

## Live repro

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Prompt: `do [#next-steps-2]. #spec-test-commit-push-deploy`
- Outcome recorded by the agent:
  - queue was cleared
  - status said repo/API/deploy work was complete
  - response said the gate was completed except for explicitly residual external gates
  - related review items were reconciled/reaped
- Bad state: `agent:backlog` still contained `- [ ] [#next-steps-2] ...`.

The user had to notice that the tracked `do [#id]` item was still open after a
successful closeout. That makes the backlog untrustworthy for the next cycle.

## Distinction from related bugs

- Not `plan-backlog-done-reap.md`: that plan covers items already rendered as
  `[x]` but not archived. Here the item never got marked done.
- Related to `#pending-done-guard-false-positive`: that item prevents over-eager
  done detection from prose citations. This bug is the opposite failure: a clear
  resolved `do [#id]` directive did not require a done/gate/stays-open outcome.
- Related to `plan-finalize-pending-done-cascade.md`: the eventual fix should not
  reintroduce finalize retry cascades when `--done` and capture guards interact.

## Invariant

When the active prompt target is a tracked work directive (`do #id`, `do [#id]`,
`complete #id`, or queue head resolving to an id), a successful closeout must end
with exactly one explicit lifecycle outcome for that id:

1. moved to `agent:done` / archive via `--done <id>`,
2. moved to `agent:review` via `--pending-gate <id>` when implementation is
   complete but review/external validation remains,
3. left open only when the response includes a machine-detectable, explicit
   "stays open because ..." reason and the queue is not silently treated as done.

A response saying "complete", clearing the queue, or updating status must not be
enough if the target id remains `[ ]` in `agent:backlog`.

## Proposed implementation

1. Extend preflight/plan prompt-target extraction so `do [#id]` produces an
   `expect_done_or_gate` pending mutation for the target id.
2. Extend `session-check` / finalize guard logic:
   - If a prompt-target id was resolved in the response heading/status and still
     appears open in `agent:backlog`, fail closed with a diagnostic that says to
     rerun finalize/write with `--done <id>`, `--pending-gate <id>`, or an explicit
     open-state override.
   - Do not trigger on incidental body citations; reuse the heading/topic
     resolution model planned in `#pending-done-guard-false-positive`.
3. Ensure queue consumption and backlog lifecycle are coupled:
   - if `agent:queue` consumes `do [#id]`, the matching tracked id must also reach
     a lifecycle outcome before the cycle is considered clean,
   - if the id stays open, the queue should either keep/requeue the prompt or record
     a visible gated/open reason.
4. Add deterministic tests:
   - template doc with `do [#abc]`, response says completed, queue cleared, backlog
     still `[ ]` -> session-check fails,
   - same cycle with `--done abc` -> succeeds and archives/reaps,
   - same cycle with `--pending-gate abc` -> succeeds and moves to review,
   - same cycle with an incidental `#abc` citation but unrelated heading -> no
     false positive,
   - same cycle where response explicitly says the item stays open because of an
     external blocker -> succeeds only if the queue/backlog state remains coherent.
5. Update `runbooks/pending-ops.md` and `commit.md` to make the closeout rule
   explicit for `do #id` tasks.

## Acceptance

- The monsterrodholders shape cannot commit cleanly with `#next-steps-2` left
  open while the queue/status claim the directive was handled.
- Agents get one clear repair path instead of needing the user to audit the
  backlog manually.
- Existing false-positive fixes still allow harmless references to open ids in
  explanatory prose.
