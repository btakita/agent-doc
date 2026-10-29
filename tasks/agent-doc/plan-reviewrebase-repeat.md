# Parent-scoped repeat execution for nested review and rebase presets

Status: planned. This document defines the implementation and verification work;
it intentionally does not implement the fix.

## Incident and invariant

On 2026-10-29, `haiven-dev/tasks/api.md` at commit `46525385` contained a live
`#review` child under `https://github.com/haiven-dev/haiven-api/pull/767`.
Commit `477a8d7e` removed that child while recording the review of pull request
763. The response for one parent therefore completed an equal-text leaf under a
different parent.

Required invariant (`#reviewrebase-repeat`): every executable leaf occurrence in
one prompt tree is distinct work. Equal words under different parents must each
run once. A short claim may bind one remaining occurrence, but its completion,
response echo, deferral, baseline fence, and closeout strike must stay bound to
that same occurrence. A completed leaf under parent A must not consume, suppress,
defer, or provide completion evidence for the equal leaf under parent B.

The identity is structural, not textual:

```text
queue component identity + parent lineage + executable leaf occurrence
```

For the common PR tree, the operator-visible claim/echo key remains the nest
vector, for example:

```text
https://github.com/haiven-dev/haiven-api/pull/763: #review
https://github.com/haiven-dev/haiven-api/pull/767: #review
```

Flat intentional repeats remain ordered occurrences. One answer consumes one
flat occurrence unless the caller explicitly supplies multi-head consume
authority; this plan must not regress `#ftstrikeonce`.

## Current seam and root cause

Recent queue work made selection, dispatch, and claims lineage-aware:

- `document_queue::lineage_claim_text` and `queue_claim::claim_identity_at`
  derive the parent-scoped key for a nested executable leaf.
- `ClaimQueueView::from_content`, subagent dispatch, and short shared-leaf claim
  resolution retain that key, so repeated `#review` and `#rebase` leaves can be
  claimed separately.
- submitted prompts and queue guidance already render the nest vector when the
  bare leaf is ambiguous.

Closeout still reconstructs identity from bare node text. In
`agent-doc-queue/src/queue_consume.rs`,
`answered_free_text_strike_decision`:

- iterates Markdown item nodes whose `item.text` is only the leaf text;
- calls claim, selection, answer, deferral, and baseline predicates with that
  bare text;
- tracks `struck_counts`, `already_applied`, and `struck_texts` by normalized
  leaf text across the document.

In `agent-doc-queue/src/queue_response.rs`, nest-vector lookup accepts a bare
leaf and resolves one matching occurrence through `located_prompt`. That is
adequate when a leaf is unique, but it cannot prove which occurrence a closeout
node represents when the same leaf appears below several parents. The
text-count fence is then asked to repair lost identity after normal consume and
answered-head projection have both acted. It cannot reliably distinguish
"this occurrence was already consumed" from "a different parent has the same
words."

This is the policy inversion to remove: occurrence identity must be an input to
the closeout decision, not inferred afterward from text counts.

## Target architecture

### 1. One parsed occurrence model

Add a pure queue projection that enumerates executable occurrences from the
selected queue graph. Each occurrence carries at least:

- queue component ID (including the legacy default resolution);
- exact node key/source span used by structural mutation;
- raw display text and marker-stripped work text;
- `lineage_claim_text`;
- `QueueItemIdentity` from `claim_identity_at`;
- flat-repeat ordinal or equivalent source-stable occurrence discriminator when
  equal flat rows intentionally share the same text and lineage.

Do not create a second lineage parser in closeout. Reuse the same parsed queue
entries and identity constructors used by claim and dispatch. A cosmetic runtime
marker, claim badge, pin, or answered annotation must not change the occurrence
identity.

### 2. Occurrence-scoped answer decision

Refactor `answered_free_text_strike_decision` to decide against one occurrence,
not one bare string. Answer evidence follows these rules:

- an explicit nest-vector echo answers only the matching lineage occurrence;
- a bare leaf echo is accepted when the selected/claimed occurrence supplies
  unambiguous authority, and it applies to exactly that occurrence;
- without selected/claimed authority, a bare echo that matches multiple live
  parent lineages fails closed instead of choosing the first textual match;
- a deferral for one nest vector defers only that occurrence;
- release-include and operator-action holds remain occurrence-scoped;
- preset expansion evidence is evaluated for the occurrence whose preset is
  being completed and cannot fan out across equal preset words.

Replace `nest_vector_in_document(content, bare_leaf)` use at the decision seam
with an API that receives the occurrence's already-derived lineage. Compatibility
helpers for unique bare heads may delegate to this API, but production closeout
must not re-run a first-match lookup.

### 3. Occurrence-scoped fences and receipts

Key these closeout facts by occurrence identity rather than normalized text:

- baseline presence;
- current-versus-baseline struck deltas (`#ftstrikeonce`);
- preflight-selected heads;
- active/reported claim authority;
- dispatch-owned inline vetoes;
- node keys selected for the answered strike;
- claim/completion retirement after the strike.

The normal selected-head consume and the answered-head catch-up projection must
publish/observe the same occurrence receipt. If normal consume already consumed
parent A, catch-up treats parent A as applied and leaves parent B live. It must
not infer that receipt from a document-wide count of `#review` strings.

Keep this in the existing document/turn-scoped Lazily topology: authoritative
document source -> parsed queue occurrences -> selected/claimed occurrence and
materialized response evidence -> computed strike decision -> structural
mutation effect -> typed receipt -> recomputation. Do not add polling, a second
state store, or caller-owned retry/recompute steps.

### 4. Compatibility and ambiguity policy

Preserve these existing contracts:

- one unique nested leaf may still be addressed by its short text;
- a short claim over several equal leaves binds the first remaining eligible
  occurrence in queue order, and subsequent short claims bind the next ones;
- an explicit nest vector always targets that parent;
- a later identical flat repeat stays live after one answer;
- loop queues retain their pass-scoped lifecycle and remain excluded from the
  document-wide answered-head catch-up projection;
- component IDs fence equal trees in separate `agent:run` components;
- claimed, selected, reported-release, and post-baseline authority all compare
  the same occurrence key.

If a legacy persisted claim contains only a bare shared leaf and cannot be
mapped uniquely, diagnose it as ambiguous and leave every occurrence live. Do
not silently attach it to the first parent. A unique legacy claim may be
migrated to the occurrence key when the authoritative document is observed.

## Specification updates

Update the normative queue sections in `specs/02-document-format.md`,
`specs/07-orchestration-commands.md`, and `specs/pending-system.md` to state:

1. nested executable work is identified by queue component plus lineage, not
   leaf text;
2. short claims are convenience selectors for one remaining occurrence, not a
   shared completion identity;
3. response evidence and closeout strikes are occurrence-scoped;
4. equal leaves under different parents each remain runnable;
5. a bare ambiguous response without selected/claimed occurrence authority
   cannot close any sibling occurrence;
6. flat repeated rows retain consume-once semantics.

Update the module-harness comments beside the pure occurrence projection and
answer decision with the same invariant. Align every bundled `respond.md` copy
only after the binary contract is implemented; agent wording must describe the
enforced behavior rather than compensate for a text-keyed binary.

## Test plan

### Pure queue identity and response tests

Add table-driven tests in `agent-doc-queue` using the incident shape:

- PR 763 and PR 767 both contain `#review`; a response echoing
  `PR 763: #review` strikes only PR 763.
- The symmetric PR 767 response strikes only PR 767.
- Two `#rebase` leaves under separate parents run and close independently.
- A short selected/claimed `#review` closes the bound occurrence only, then the
  next short claim binds the other occurrence.
- A bare `#review` response with no occurrence authority and two live matches
  strikes neither.
- A deferral for `PR 763: #review` does not defer or complete PR 767.
- One parent already struck since baseline does not make the other parent's
  equal leaf "already applied."
- Existing flat-repeat and loop-queue cases remain unchanged.

Exercise `lineage_claim_text`, `claim_identity_at`, baseline membership,
`answered_free_text_strike_decision`, answer evidence, deferral evidence, and
the structural node-key projection in the same fixture family so no adapter can
fall back to leaf text unnoticed.

### I/O and lifecycle tests

Add `agent-doc-queue-io` coverage that uses a temporary session document and the
real claim ledger:

1. claim short `#review` with both parents live;
2. bind/report/close the first occurrence;
3. assert the sibling remains live and claimable;
4. claim short `#review` again and assert it binds the sibling;
5. repeat for `#rebase` and for an explicit nest-vector claim.

Cover live claim, reported-release, post-baseline selected-head authority, and
claim retirement. Assert the stored attempt identity, visible lifecycle marker,
completion feedback key, and struck node all name the same lineage.

### Closeout integration regression

Add a closeout-level test with the relevant excerpt of
`haiven-dev/tasks/api.md`:

- baseline contains both PR parents and their equal preset leaves;
- normal consume records PR 763's review response;
- answered-head catch-up runs in the same closeout;
- final authoritative content has PR 763 struck and PR 767 still live;
- a second cycle can claim and complete PR 767;
- snapshot and document projections agree, and no residue guard reports PR 767
  as already answered.

Use the real response echo format and both ordering directions (763 before 767,
767 before 763) so a first-match implementation cannot pass accidentally.

### Property coverage

Generate small prompt trees with two or more parents and repeated leaves. For
every chosen occurrence, assert:

- exactly one occurrence transitions per completion receipt;
- all non-chosen occurrence identities preserve state;
- cosmetic markers do not change identity;
- permuting parent order changes selection order but never target lineage;
- completing every occurrence requires the same number of receipts as live
  occurrences.

## Implementation sequence

1. Introduce and test the pure parsed occurrence projection without changing
   closeout behavior.
2. Migrate claim, selection, dispatch, and response helpers to consume that
   projection, deleting duplicate first-match lineage lookup where possible.
3. Change answered-head strike, baseline/consume-once fences, deferral, and
   retirement to occurrence identity in one atomic behavior change.
4. Add the I/O and closeout incident regression before removing the old
   normalized-text maps.
5. Delete or narrow text-only compatibility helpers so new production call
   sites cannot bypass lineage.
6. Land the spec/module-harness/runbook alignment.

Do not split step 3 across releases: a mixed state where answer matching is
lineage-aware but baseline or retirement remains text-keyed can still lose or
mis-retire a sibling occurrence.

## Verification and acceptance

During implementation, run focused tests for `agent-doc-queue`,
`agent-doc-queue-io`, closeout/write, session-check residue, and controller
structural projection. Then run `make check-fast`, `make tmux-ci` if the latest
CI tmux leg is red for this change, and the authoritative `make check` once on
the integrated bytes.

Acceptance requires all of the following:

- the 46525385 -> 477a8d7e incident fixture cannot remove PR 767 while closing
  PR 763;
- every equal nested `#review`/`#rebase` occurrence is independently claimable
  and completable;
- one response changes exactly one occurrence unless explicit multi-head
  consume authority is supplied;
- `#ftstrikeonce`, loop queues, flat repeats, claims, reported releases,
  snapshot sync, and residue checks remain green;
- no production closeout path keys sibling suppression or applied receipts only
  by normalized leaf text.

## Out of scope

- changing operator-visible queue syntax;
- assigning permanent IDs to every Markdown row;
- changing preset expansion semantics;
- auto-completing several sibling leaves from one response;
- release/version/tag/publication work;
- repairing historical session commits beyond using the incident as a fixture.
