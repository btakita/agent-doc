# Plan: blocked `do #id` closeout must preserve actionable next steps in backlog

## Status note (2026-05-31, #blocked-closeout-followup-capture) — LANDED

`check_blocked_closeout_followup_guard` shipped in `session_check.rs`, wired into
the post-commit guard loop. It fires (strict → fail closed, warn → advise) when a
directed `do [#id]` cycle gates its target out of `agent:backlog`
(`cycle_state.pending_gated_ids`, still present in `agent:review`) while the
response carries a tight blocked / still-needed-work phrase
(`BLOCKED_FUTURE_ACTION_PHRASES`) co-occurring with `#id` in the same paragraph.
Satisfied by: keeping the id open via `--pending-edit` (`pending_kept_open_ids`),
any `--pending-add*` (`cycle_state.pending_added_this_cycle`), an explicit
"no additional backlog follow-up is needed because …" phrase, a
`<!-- no-blocked-followup-guard -->` / `<!-- no-pending-done-guard -->` marker, or
`--done`. New cycle_state fields + recorders (`record_pending_gated_ids`,
`mark_pending_added`) feed it from `write.rs`. Tests:
`blocked_closeout_followup_guard_*` (7, incl. inspect wiring + the not-tied-to-id
false-positive guard) in `session_check.rs`. Docs: `runbooks/pending-ops.md`,
`runbooks/commit.md`, `specs/07-closeout-commands.md`. FlowCore budget updated
(session_check `guard_` 40 → 49).

## Live repro

- Session document: `src/boost-client/tasks/monsterrodholders.md`
- Directed item: `#374n-removalcleanup`
- User report: "Next steps to complete [#374n-removalcleanup] were not added to
  the backlog."
- Observed closeout shape:
  - the agent found a real blocker: Merchant Center still had 17 active
    `ELIGIBLE` legacy removal rows;
  - the response named concrete completion conditions: remove/expire the rows,
    deliberately delete them through an approved path, or get Ron approval that
    they are safe blanks;
  - `#374n-removalcleanup` was moved/gated out of the active backlog instead of
    leaving or adding an actionable backlog item for those next steps.

The result is that the document explains the blocker, but the active backlog no
longer clearly drives the work needed to complete the item.

## Distinction from related bugs

- `#next-steps-backlog-capture` enforces backlog capture when the prompt itself
  asks for `#next-steps` or follow-up backlog items. This repro happened during
  a directed `do [#id]` closeout, not a standalone `#next-steps` prompt.
- `#do-id-partial-closeout-state` detects a directed item that claims completion
  while the original id remains open. This repro is the opposite state hazard:
  the item was not complete, but the closeout path could move it out of the
  active backlog by treating "blocked" like "review-gated".
- `#backlog-priority-attribute` and queue sync work can improve ordering, but
  they do not decide whether blocked work stays represented as actionable
  backlog.

## Desired invariant

For a directed `do [#id]` cycle, if the response says the target is blocked,
deferred, waiting on approval, or otherwise still needs future action, closeout
must preserve an actionable next step in `agent:backlog` unless the response
explicitly states that the existing backlog item already remains open and names
the exact reason.

Valid closeout states:

1. `--done <id>` when the work is complete.
2. `--pending-gate <id>` only when the work is implementation-complete and
   waiting for review/external validation, with no new execution step needed by
   the agent.
3. `--pending-edit <id>=...` keeping the same id open in backlog when the item
   is blocked and the next action is still the same tracked work.
4. `--pending-add-after <id> ...` or `--pending-add ...` when the blocker splits
   into a new actionable follow-up.
5. An explicit no-follow-up statement only when there is genuinely no actionable
   backlog work to capture.

## Proposed implementation

1. Add a closeout classifier for directed prompt targets:
   - target id from `do [#id]`, queue head, or prompt target;
   - response/body signals: `blocked`, `remains blocked`, `waiting`, `requires`,
     `until`, `only after`, `no live cutover`, `approval`, `external`;
   - lifecycle mutation applied: done, gate/review, edit-kept-open, add-follow-up,
     or no mutation.
2. Teach `session-check` to fail closed when all are true:
   - the cycle targeted an active backlog id;
   - the response has blocked/future-action signals;
   - the target id was moved to review or removed from active backlog;
   - no new/edited active backlog item captures the concrete next step.
3. Add a directed repair diagnostic:
   - for same-id blocked work: use `agent-doc write --pending-edit
     "<id>=..."`;
   - for split work: use `--pending-add-after <id> "id=<new-id> ..."`;
   - for true review-only gates: include an explicit "no additional backlog
     follow-up is needed because ..." phrase.
4. Add deterministic fixtures:
   - monsterrodholders shape: blocked response + `--pending-gate
     374n-removalcleanup` + no active backlog follow-up -> session-check fails;
   - same response with `--pending-edit 374n-removalcleanup=...` keeping it in
     backlog -> passes;
   - same response with `--pending-add-after 374n-removalcleanup ...` -> passes;
   - implementation-complete review gate with explicit no-follow-up phrase ->
     passes;
   - incidental use of the word "blocked" in unrelated prose -> no false
     positive unless it is tied to the directed id.
5. Update `runbooks/pending-ops.md` and `commit.md` to distinguish
   "review-gated" from "blocked but still actionable" closeout.

## Acceptance

- The `#374n-removalcleanup` shape cannot close cleanly with the item moved out
  of the active backlog while completion steps exist only in prose.
- Agents receive a repair command that either keeps the original id open with
  narrowed next action text or adds a sibling follow-up item.
- Existing `#next-steps-backlog-capture` behavior stays intact and does not need
  to synthesize backlog items from arbitrary prose.
