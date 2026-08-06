# Plan — queue-contamination guard fires with no available remedy

**Status:** diagnosis only. DO NOT IMPLEMENT (filed via the document's agent-doc-bug preset).
**Observed:** 2026-08-02, `src/boost-client/tasks/monsterrodholders.md`, agent-doc 0.35.118.
**Recurrence:** second occurrence; first was recorded as a `queue: stop` case, this one is `queue: go`.

## Symptom

`agent-doc session-check` fails closed on every finalize:

```
[session-check] INTERRUPTED: agent:queue contains assistant response prose copied from a
`### Re:` body, not a user prompt or `do [#id]` directive: "We are having a partnership with".
Remove the contaminating line(s) from `agent:queue` ...
(see jb-run-agent-doc-response-queue-contamination)
```

The flagged line is **operator text** — a half-typed free-text queue line the operator was still
writing. It is not copied response prose.

## Root cause (confirmed this session)

The guard's match is *self-referential and permanent*:

1. The operator leaves a half-typed queue head, e.g. `- We are having a partnership with `.
2. A later assistant response summarizes queue state and **quotes that line** in its `### Re:` body
   (here, line 287: ``...and a half-typed `We are having a partnership with `.``).
3. The guard tests "does this queue line appear in a `### Re:` body?" — which is now literally true.
   The direction is inverted: the *response quoted the queue*, not the queue copying the response.
4. `agent:exchange` is append-only, so that mention never ages out. The guard therefore fires on
   **every** subsequent finalize, permanently, until the operator's own line is removed.

Single-command diagnostic: `grep -n "<flagged text>" <FILE>` returning two hits — one inside an old
response paragraph, one in `agent:queue` — identifies the false positive.

## Why the guard is currently unescapable

Every sanctioned remedy refuses. Verified individually this session:

| Path | Result |
|---|---|
| `agent-doc queue prune-noise <FILE>` | `no predicate-proven queue heads to prune (queue inactive, empty, or all heads are drainable/live)` — refuses on an **active** (`priority go`) queue too, not just an inactive one |
| `agent-doc queue consume` | scoped to **leading** free-text heads; the flagged line was the 5th head, so reaching it would strike four legitimate work items |
| `agent-doc queue consume --id / --ack-id` | id-backed heads only; this head has no id |
| `agent-doc write --template --stream` with a `patch:queue` block | `partial or non-committing response writes are disabled for session documents` — rejected before patch parsing, so no repair-only path exists |
| suppression marker | none exists (no queue-contamination opt-out, unlike the queue-removal-guard / pending-capture markers) |

**The core defect is a predicate disagreement.** `guard_queue_response_contamination` classifies the
line as *response prose*; `prune-noise` classifies the same line as *drainable/live* and so declines
to touch it. The guard fires and its own prescribed remedy refuses, leaving the operator's only exit
as hand-deleting their own text — which the operator-text authority rule forbids the agent from doing.

Net effect: closeout is permanently held open. The write itself still commits (verified: `39429efd`
landed with the doc clean), so this blocks *clean closeout and the auto-loop*, not the work.

## Fix directions (not implemented — pick one)

1. **Fix the predicate, preferred.** The contamination test should not match when the queue line
   *predates* the `### Re:` body that mentions it. Node identity / insertion order already exists in
   the semantic diff (`node_events` carry `item_id` and `before_index`), so "queue line is older than
   the quoting response" is decidable rather than heuristic.
2. **Require a stronger signal.** Match only on a substantial contiguous span of a response body
   (e.g. a full sentence or a fenced/table fragment), not on a short fragment that a response merely
   quoted while narrating queue state.
3. **Make the remedy reachable.** Align `prune-noise` with the guard so that whatever the guard
   flags, the pruner can strike — a guard whose own remedy refuses is the actual bug regardless of
   which classifier is right.
4. **Offer an operator-authorized escape hatch.** e.g. `queue consume --line <n>` or
   `--ack-text "<exact>"` to strike a specific non-leading free-text head, so operator-approved
   resolution does not require hand-editing or striking unrelated heads.

## Regression coverage to add

- Queue head present, later response body quotes it verbatim → guard must **not** fire.
- Queue head genuinely pasted from a response body → guard **must** fire (protect the true positive).
- Whatever the guard flags, `prune-noise` must be able to strike it (assert the two predicates agree).
- Non-leading free-text head strike leaves preceding heads intact.

## Cross-references

- Operator memory: `agent-doc-queue-contamination-guard-false-positive`
- Authority rule: operator-visible document text is authoritative; never resolve by dropping operator text.
