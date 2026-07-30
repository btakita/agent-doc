# plan: per-scenario replay accounting (#lzscenariocoverage)

## The defect

A fixture with multiple named scenarios can be PARTIALLY replayed and nothing
notices. `reliable-sync/liveness_orset_lww.json` carries four scenarios;
lazily-dart replays three, lazily-rs and lazily-go skip
`derived_live_doc_aggregate_converges_under_retry` entirely, lazily-cs replays
all four. Every one of those runs is green and the fixture is counted as
covered.

Both existing guards are blind to this **by construction**:

- the coverage guard (`#lzcoverageaudit`) asks only whether the FILE was
  opened — one scenario is enough;
- the key guards (`#lzassertunknownkeys`, `#lzconsumednotasserted`) only bind
  blocks a runner actually reaches, so an unreplayed scenario contributes no
  unconsumed key and no unasserted key. Skipping a whole scenario is invisible
  to a guard that only inspects the scenarios you ran.

This is the next rung:

| rung | guard | proves |
|---|---|---|
| 1 | runtime fixture manifest | the fixture was **opened** |
| 2 | unconsumed-key tracker | every key of a block you reached was **read** |
| 3 | unasserted-key tracker | every read key reached a **comparison** |
| 4 | **this item** | every **scenario** in the fixture was **replayed** |

## The surface

31 fixtures carry a `scenarios` array; **101 scenarios** total.

```
collections/mergecell_algebra.json                          n=3   NO ID KEY
collections/semtree_incremental.json                        n=3   name
collections/seqcrdt_convergence.json                        n=6   name
collections/stableid_alignment.json                         n=6   name
collections/textcrdt_convergence.json                       n=6   name
collections/textcrdt_delta_sync.json                        n=4   name
crdt-tree/algebra.json                                      n=3   name
distributed/anti_entropy_converge.json                      n=3   name
familysync/materialize_on_ingest.json                       n=3   name
lossless-tree/concurrent_conflict_preserves_text.json       n=1   name
lossless-tree/concurrent_insert_same_parent.json            n=1   name
lossless-tree/concurrent_reorder_and_leaf_edit.json         n=1   name
lossless-tree/exact_roundtrip.json                          n=1   name
lossless-tree/invalid_source_roundtrip.json                 n=2   name
lossless-tree/non_contiguous_anti_entropy.json              n=2   name
lossless-tree/one_leaf_edit_delta.json                      n=2   name
lossless-tree/split_merge.json                              n=2   name
lossless-tree/token_trivia_preservation.json                n=2   name
message-passing/cancel_preempts_nonterminal.json            n=2   name
reactive-graph/scope_teardown_equals_fold_of_disposals.json n=2   name
reliable-sync/coalesce_bounds_outbox.json                   n=3   name
reliable-sync/idempotent_redelivery.json                    n=2   name
reliable-sync/liveness_lease_eviction.json                  n=4   name
reliable-sync/liveness_orset_lww.json                       n=4   name
reliable-sync/multi_epoch_delta.json                        n=2   name
reliable-sync/outbox_replay_after_crash.json                n=2   name
reliable-sync/outbox_store_protocol.json                    n=4   name
reliable-sync/resync_gap_converge.json                      n=2   name
stdlib/revision_barrier.json                                n=9   id
stdlib/timeout.json                                         n=8   id
stdlib/timer.json                                           n=6   id
```

Note the corpus is not uniform: 28 fixtures identify a scenario by `name`, the
three `stdlib` ones by `id`, and `collections/mergecell_algebra.json` carries
**no identifier at all** (its scenarios are distinguished only by `policy`).

## The contract

### 1. Scenario id resolution

Resolve a scenario's id in this fixed order, identically in every binding:

1. `id` if present
2. else `name` if present
3. else the positional index, spelled `#<n>` (0-based)

A ledger entry that fell back to the positional index must be **reported** in
the guard's output, not silently accepted — the fallback exists so this item is
not blocked on a corpus edit, and its visibility is what makes the corpus gap
fixable later. Do NOT edit `lazily-spec` to add the missing identifiers as part
of this item; that is a shared-corpus change and belongs upstream.

### 2. A runtime ledger, not a declaration

The ledger records what the runner ACTUALLY replayed, exactly as the fixture
manifest records what it actually opened. A hand-authored list of "scenarios
this runner covers" is the thing being guarded against — it is a claim, and a
claim rots.

Record at the point of replay. Prefer instrumenting the shared scenario
iteration helper so recording is automatic and a new runner cannot forget it:

```
for scenario in fixture.scenarios():     # helper records the id as it yields
    replay(scenario)
```

Where a runner iterates by hand, add the explicit `recordScenario(fixture, id)`
call at the top of the loop body — after any `continue`, so a skipped scenario
does not record itself.

### 3. Verification, both directions

At end of run, for every fixture the manifest says was opened, compare the
ledger's scenario ids against the ids present in the fixture on disk:

- a scenario id in the fixture but not in the ledger → **failure**, unless
  excused;
- `excuseScenario(fixture, id, reason)` with a non-empty reason marks it
  satisfied;
- an excuse for a scenario the same run DID replay → **failure** (stale);
- an excuse naming an id the fixture does not carry → **failure** (stale).

Same both-directions rule as the `KNOWN_UNCOVERED` fixture allowlist and the
key excuses from `#lzconsumednotasserted`. Keep the excuse list next to the
existing coverage allowlist so there is one place to read what this binding
does not prove.

### 4. Repair what the ledger finds

**Prefer implementing the missing scenario.** lazily-rs and lazily-go must
actually replay `derived_live_doc_aggregate_converges_under_retry` rather than
excuse it — a known-skipped scenario is exactly the work this item exists to
force. Excuse only where the binding genuinely cannot express the scenario, and
say why in the reason.

## Verification (required per binding)

Mutation-check all three failure modes, restoring after each:

1. Skip one scenario a runner currently replays (`continue` past it). The run
   must fail naming that fixture and scenario id.
2. Add an `excuseScenario` for a scenario the run DOES replay. Must fail as a
   stale excuse.
3. `excuseScenario` an id the fixture does not carry. Must fail as a stale
   excuse.

Then run the binding's full `make check` and confirm green. On compiled
bindings, `touch` restored files so the build system does not serve the mutated
artifact.

## Out of scope

- Editing `lazily-spec` — no fixture, no identifier, no schema change here.
- Flipping any `coverage.json` mark. This changes no feature coverage.
- Sub-scenario `steps` accounting. A scenario's steps are already covered by
  rungs 2 and 3 once the scenario is replayed at all.
