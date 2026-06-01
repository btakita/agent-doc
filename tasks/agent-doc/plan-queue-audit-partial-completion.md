# Plan: queue audit must distinguish partial completion from not complete

## Problem

Live repro: `src/boost-client/tasks/monsterrodholders.md` had six free-text
queue "next step" items. Several prerequisites were already complete:

- same-day conversion QA was clean
- HydroWeb and non-brand URL/validate-only checks were current
- feed-label and Merchant removal audits were clean
- budget ceiling checks and Rods negatives were complete

The answer recorded "none of the queue items is fully complete" because every
queue item still had at least one remaining required action. That was technically
defensible for full closeout, but it was a poor queue audit: the user asked
which queue steps were complete, and the visible answer collapsed meaningful
partial completion into "none."

## Distinction From Related Bugs

- Not `#completed-queue-residue-regression`: no completed queue row was left
  behind after a closeout.
- Not `#do-id-partial-closeout-state`: these were free-text next-step queue
  items, not a single directed `do [#id]` item whose implementation phase
  partially shipped.
- Related to `#gated-followup-split-enforcement`: multi-condition queue rows
  should be split or narrowed so their completed subconditions are not buried.

## Desired Behavior

When asked to audit `agent:queue` completion, agent-doc should classify each
queue item into one of three states:

1. **Complete**: all required conditions in the row are satisfied and the row can
   be removed or marked done.
2. **Partially complete**: some prerequisites or substeps are satisfied, but at
   least one explicit remaining action blocks closeout.
3. **Not started / blocked**: no material substep is complete, or the item is
   waiting entirely on an external/input gate.

For partially complete rows, the response should name both:

- the completed substeps, and
- the exact remaining closeout condition.

If a row contains multiple independently actionable phases, agent-doc should
recommend splitting the row into child backlog IDs or narrowing the queue row to
the next actionable phase.

## Implementation Plan

1. Add a queue-audit helper that parses queue rows for completion language,
   remaining-action language, and referenced review/backlog IDs.
2. Extend planning/session-check guidance for prompts like "which queue items are
   complete?" so the response contract requires a per-row state table, not just a
   binary all-or-none summary.
3. Add a warning when an audited queue row has both completed-substep evidence
   and remaining-action evidence but the final response says only "not complete"
   without a partial-completion explanation.
4. Add an optional split recommendation when a single row contains multiple
   gateable phases or prerequisites that deserve separate backlog IDs.
5. Add regressions using the monsterrodholders shape:
   - same-day QA complete but 3-day QA remaining -> partially complete
   - validate-only complete but Ron/create-paused approval remaining -> partially
     complete
   - margin inputs entirely missing -> not started / blocked

## Acceptance

- The monsterrodholders queue audit would answer "none are fully complete, but
  several are partially complete" and list the completed vs remaining substeps.
- New backlog IDs generated from queue rows preserve the partial state instead
  of rewriting each item as simply open.
- The guard does not auto-complete or remove queue rows; it only improves audit
  accuracy and split/narrowing recommendations.
