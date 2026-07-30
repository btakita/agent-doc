# plan: consumed is not asserted (#lzconsumednotasserted)

## The defect

`#lzassertunknownkeys` landed a per-binding tracker that fails a conformance run
when a fixture assertion block carries a key **no runner read**. That proves
consumption. It does not prove assertion.

A runner can read a key and then do nothing with it. The tracker sees the read
and goes green. Three shapes:

1. **Named skip inside a consuming loop.** The loop iterates the block, reads
   `block[key]` (which marks it consumed), then `continue`/`break`s past it.
   Confirmed in `lazily-dart/test/collections_extra_conformance_test.dart`:

   ```dart
   for (final entry in expectInitial.entries) {
     if (entry.key == 'sibling_a_cached' ||
         entry.key == 'downstream_consumer_reran') continue;   // read, discarded
     expect(tree.nodeValue(entry.key), equals(entry.value));
   }
   ```

   and again as a `case 'downstream_consumer_reran': break;` arm.

2. **Value bound but never compared.** `final want = block['x'];` with no
   subsequent use of `want` in an assertion.

3. **Comparison against a literal rather than the fixture value.** The arm reads
   the key but asserts against a hardcoded constant, so editing the fixture
   changes nothing:

   ```dart
   if (expectAfter['downstream_consumer_reran'] == false) {
     expect(tree.isCached('root'), isTrue);   // fixture value gates, never asserts
   }
   ```

This is the third rung of the ladder:

| rung | guard | proves |
|---|---|---|
| 1 | runtime fixture manifest (`#lzcoverageaudit`) | the fixture was **opened** |
| 2 | unconsumed-key tracker (`#lzassertunknownkeys`) | every key was **read** |
| 3 | **this item** | every read key reached a **comparison against the fixture's own value** |

## The contract

Every binding extends its existing `#lzassertunknownkeys` tracker. No new
parallel mechanism, no second allowlist file.

### 1. Track `asserted` alongside `read`

The tracker already records which keys were read. Add a second set recording
which keys reached a comparison. Verification (the same teardown / `Drop` /
`defer` / fixture-scope hook that already fires) now fails on **three**
conditions:

- a key that was never read — existing `#lzassertunknownkeys` behaviour;
- a key that was read but never asserted — **new**;
- a stale excuse (below) — **new**.

Prose keys (`comment`, `description`, `note`, `notes`, `why`, plus each
binding's existing equivalent) stay exempt from all three.

### 2. One assertion entry point

Provide the helper the runners call instead of reading the raw value and
comparing by hand. Name it idiomatically per binding, but keep the shape:

```
assertKey(block, key, actual [, where])
```

It reads `block[key]` (marking read), marks the key **asserted**, and performs
the equality check against that value. A key can only become asserted by going
through this path, so an arm that compares against a literal never marks it.

Where a comparison is genuinely not an equality — a tolerance, a set
containment, a regex — provide `assertKeyWith(block, key, predicate)` or the
binding's equivalent, which marks asserted and hands the fixture value to the
caller's own check. The point is that the fixture's value must reach the
comparison, not that the comparison must be `==`.

### 3. Declared exceptions, both directions

Where a key genuinely cannot be asserted at this call site — the binding tests
it elsewhere, or the field is a discriminator that selects a code path rather
than a value to check — the runner must say so out loud:

```
excuseKey(block, key, reason)   // reason must be non-empty
```

`excuseKey` marks the key satisfied and records the reason. It is **both
directions**, exactly as the `KNOWN_UNCOVERED` allowlist is: excusing a key that
the same run also asserts is a failure, because the excuse has gone stale and is
now hiding nothing.

Prefer converting the skip into a real assertion. Excusing is the fallback for
when there is nothing to compare, and the reason text has to say why.

### 4. Repair the sites the audit finds

Sweep every conformance runner in the binding for the three shapes above. For
each hit:

- **Preferred:** implement the assertion. `sibling_a_cached` and
  `downstream_consumer_reran` are real observable facts about the library —
  assert them against the fixture value rather than excusing them.
- **Fallback:** `excuseKey` with a reason naming where the fact is proven
  instead, or why it is unprovable here.

Comparisons against a literal become comparisons against the fixture value.
Values bound but never used are either asserted or deleted.

## Verification (required per binding)

A guard nobody mutation-checks is a guard that might be asserting its own
bookkeeping. Runners assert the **library**, not the runner's counters.

For each binding, prove all three failure modes redden, then restore:

1. Convert one real assertion into a read-then-discard (read the key, drop the
   comparison). The run must fail with the read-but-not-asserted message.
2. Add an `excuseKey` for a key that IS asserted. The run must fail as a stale
   excuse.
3. Delete one assertion arm entirely. The existing unconsumed-key message must
   still fire (rung 2 must not regress).

Restore after each probe and re-run green. On compiled bindings, `touch` the
restored file so the build system does not reuse the mutated artifact.

Then run the binding's full `make check` (or equivalent) and confirm green.

## Out of scope

- Per-scenario replay accounting — that is `#lzscenariocoverage`, a different
  rung (a whole scenario never replayed contributes no unconsumed key at all).
- `lazily-cpp`'s fully-transcribed runners that open no fixture — that is
  `#lzcpptranscribedrunners`. This item covers cpp runners that DO replay.
- Flipping any `coverage.json` mark. This changes no feature coverage.
