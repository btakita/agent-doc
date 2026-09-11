---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: stop
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260911-150520.md`*

Compacted content:
- Archived 43 response topic(s): #lzzigcanonrunner; #lzzigforkhlcpeer; #adjshashprivatefield; 40 more
- Prior summary/context: prior compacted content: Archived 28 response topic(s): Phase 1; Phase 2; superproject pointers + verification; 25 more

### Re: #lzpystructcell — pyproject extras, not another project — opus-5

> **Queue prompt:** do [#lzpystructcell]: should we use pyproject extras or another project? This adapter should be generic to handle other competing libraries such as pydantic and dataclasses.

**Extras, in `lazily-py`.** A separate distribution would ship *zero dependencies* for the two backends that matter most (stdlib `dataclasses` and `NamedTuple`), pin a `lazily` version just to reach `Source`/`Computed`/`batch`, and add a tenth release lane to a family that already releases nine bindings per canonical-fixture change. The bridge is ~all kernel internals; what it needs from msgspec/pydantic/attrs is field introspection and a constructor call.

The dependency argument for a separate project dissolves once the probes are structural. `matches()` never imports its library to decide a type is *not* one of its structs — it walks `__mro__` comparing `__module__`/`__qualname__` (msgspec, pydantic), or reads the marker attribute the library stamps on the class (`__attrs_attrs__`). If the library were not already imported, no live class could carry that base. So `import lazily` still loads no third-party module, and `dependencies` in `pyproject.toml` is unchanged. A subprocess test enforces exactly that:

```python
subprocess.run([sys.executable, "-c",
    "import sys, lazily; assert not [m for m in ('msgspec','pydantic','attr','attrs') if m in sys.modules]"], check=True)
```

New extras — install ergonomics and the CI backend matrix, nothing more: `lazily[msgspec]`, `lazily[pydantic]`, `lazily[attrs]`, `lazily[structs]` (all three). `lazily[dev]` pulls `lazily[structs]`, so `uv sync --frozen` in CI exercises all five backends rather than skipping three of them.

### Re: what shipped — opus-5

`lazily.struct_cell` — `struct_source(ctx, instance)` or `struct_source(ctx, Type, **fields)` → `StructSource[T]`: one `Source` per bridged field plus one guarded `Computed[T]` that re-materializes the struct.

- **Per-field edges.** `c.read(status["queue_depth"])` depends on that field alone. A sibling write forms no edge to it. Verified: a `queue_depth` reader stays at 1 recompute across a `last_error` write.
- **Guard, both levels.** An equal field write is inert at the cell; an equal re-materialization is inert downstream. `status.update(depth=3, last_error="boom")` on identical values recomputes nothing.
- **One wave.** `update()` / `apply()` write through `batch`, so an N-field change invalidates each dependent once, not N times.
- **`apply(instance)`** diffs a whole fresh instance into the cells — the ingress for code that already returns a copy from `status()`. Only the fields that moved invalidate.
- **Bridged fields** are the `init` fields in declaration order. Non-`init`/derived fields are excluded: the constructor recomputes them, so they are not independent state.

Backends, in resolution order: msgspec `Struct` → pydantic v2 `BaseModel` → `attrs` → `dataclasses` → `NamedTuple`. `register_struct_backend(b)` prepends, and re-registering a built-in's `name` **replaces** it instead of leaving the default behind it in the chain.

Two judgment calls worth flagging:

- **pydantic re-materializes through `model_construct`** (no validation). The values came out of an already-validated instance or an explicit per-field write; re-validating on every settled wave would make a whole-struct read cost a full validation per field change — the exact cost this bridge exists to delete. It is also alias-proof, which the validating constructor is not unless the model sets `populate_by_name`. `PydanticBackend(validate=True)` is the opt-in, passed via `struct_source(..., backend=...)`.
- **`DataclassBackend` reads fields with `getattr`, not `dataclasses.asdict`** — `asdict` recurses and deep-copies, and a bridged field must hold the object the struct holds, not a copy of it.

**Verification:** 21 new tests in `tests/test_struct_cell.py`; `make check` green end to end (ty, ruff format/check, `run_readme`, 854 tests, conformance coverage 142/153, interop peer, assertion-ordering, ci-reach). Docs in README (`## Struct bridge`), SPEC (`### Struct bridge`), CHANGELOG Unreleased. Committed `lazily-py c9f9a37`, superproject `3c5270a16c`, both pushed. `lazily-hub/lazily-py` CI run 34614969844 is still queued at write time — local `make check` is the evidence this turn stands on.

The FPE gate this item existed to answer is **open**: the eight `*State` structs are a swap, not a rewrite — `struct_source` seeds from an existing instance, `apply()` accepts the struct `status()` already builds, and `msgspec.structs.asdict` stays the one C call on that path. That adoption belongs in `src/haiven-dev/tasks/fpe.md`, not here.

### Re: #lzpypromegress — metrics as graph reads, not mirrors — opus-5

> **Queue prompt:** do [#lzpypromegress]

Two modules. `lazily.metrics` is the family; `lazily.prometheus_egress` is the adapter. The split follows `ingress` / `ingress_core`, and it matters here because the family has **no dependency at all** — `render_text()` emits the Prometheus text exposition format directly.

**The design decision that deletes the 43 mirror lines.** A `MetricFamily` child at a label set is either *writable* (`CounterCell` / `GaugeCell` over a `Source`) or *derived*:

```python
up = metrics.gauge("service_up", "1 when the component is connected.", ("component",))
up.derive(("intake",), lambda c: 1.0 if connected(c).value else 0.0)
```

`derive` binds a `Computed` that reads the graph. There is no push step, so there is no interval during which the exported number and the state it describes disagree — which is the actual defect in a mirror, not the line count. FPE's `update_runtime_metrics` is 43 `self._X.labels(...).set(...)` lines that run only when someone remembers to call them; a derived child runs when the scrape happens.

`lazily.prometheus_egress.register_metrics(registry, prom)` registers the whole registry as a `prometheus_client` **custom collector** — a pull, for the same reason. Its output is asserted byte-equal to `render_text()`, so the dependency-free path and the exporter path cannot drift.

**Four semantics worth stating, each with a test:**

- **Counter monotonicity in both directions of use.** `inc` rejects a negative delta; `set` — the form that mirrors an externally-owned absolute total, which is what `_set_component_counter` does — rejects a total below the current one. A scraper can never read a spurious counter reset out of this family.
- **Lazy derive by default.** The expression runs on `collect`, not on every upstream change, so a graph that churns between scrapes costs nothing. The honest trade-off: a lazy child holds no settled value, so a reactive consumer of `observe()` *is* invalidated by an upstream change that leaves the number alone. `eager=True` buys the `Computed` guard at the cost of computing on every upstream change. Both behaviours are asserted, not described — `test_lazy_derived_child_has_no_settled_value_to_guard_with` exists specifically so the doc claim cannot rot.
- **One kind per label set.** A writable and a derived child disagree about who owns the value, so asking for either where the other lives raises rather than silently shadowing.
- **Collection is untracked.** A scrape is not a graph node; subscribing the exporter to every metric would make scraping invalidate the graph it reads. `observe(compute)` is the explicit opt-in.

Name validation is enforced up front (Prometheus metric/label grammar, reserved `__` prefix, duplicate labels), and the text renderer handles `+Inf` / `-Inf` / `NaN` plus documentation and label-value escaping.

**On the coverage matrix.** The backlog item cites Python **Egress** as `~`. That cell is generated from `lazily-spec`'s `coverage.json`, and this is a Python-local surface — no new node kind, no invalidation rule, no canonical fixture. It does not flip the cell, and deliberately so: a new canonical fixture reddens all nine bindings, which is not a price worth paying for an exporter adapter.

**Verification:** 39 new tests in `tests/test_metrics.py`; `make check` green (ty, ruff, `run_readme`, 893 tests, conformance coverage, interop peer, assertion-ordering, ci-reach). New `lazily[prometheus]` extra, pulled into `lazily[dev]` so CI exercises the adapter rather than skipping it. Docs in README, SPEC, CHANGELOG. Committed `lazily-py c0f04dd`, superproject `aa0a66a423`, both pushed.

Also: the previous item's CI run (34614969844) finished **green**, so `#lzpystructcell` is confirmed beyond the local `make check` it was reported on. Run 34616209898 for this commit is queued.

### Re: #lzpykeyedfold — a key has one owner — opus-5

> **Queue prompt:** do [#lzpykeyedfold]

`lazily.keyed_fold`. A `KeyedFoldWriter`'s entire surface names its own key, so "clear only your own" is not a rule to follow — there is no argument that reaches another key.

```python
errors = KeyedFold[str, str, dict[str, str]](ctx)
intake = errors.claim("intake")          # exclusive; a second claim raises
dispatcher = errors.claim("dispatcher")

intake.set("connection refused")
dispatcher.set("timeout")
intake.clear()                           # dispatcher's entry survives
errors.value                             # {'dispatcher': 'timeout'}
```

`claim` being exclusive is the part that catches the FPE `last_errors` bug *before* it happens: two components writing one key is the defect, not a sharing mechanism, so it raises. `writer(key)` is there when a shared handle really is intended.

Entries are independent cells in a `SourceMap`, so a key reader is never invalidated by a sibling write. The summary depends on membership **and** every live value and on nothing else, so a `summarize` that counts entries is untouched when a value moves. `policy=` selects the `MergePolicy` `merge` folds under; a first write seeds the entry with the operand, because a `MergePolicy` is an associative merge, not a monoid — there is no identity to start from. `set` bypasses the policy. Eager by default, matching `HealthCell`, which is what makes the summary guard observable.

### Re: the kernel defect this uncovered — opus-5

Building it surfaced a real bug, so it is fixed here rather than filed.

**An eager `Computed` froze permanently when an upstream node was disposed.** Two individually-correct rules in `_dirty_disposed_dependents` combine badly: the walk clears each dirtied node's dependent set on the way through, and it deliberately schedules none of the effects it reaches (a disposal publishes nothing). For a user effect both are right. For an eager computed's puller they are fatal together — the memo edge that would rerun it is exactly the edge the walk removed. The computed then served the value it held at disposal time **forever**, and stopped tracking every *other* dependency too, so no later write could repair it:

```python
m = SourceMap(ctx); m.set("a", 1); m.set("b", 2)
v = computed(ctx, lambda c: {k: m.get(k, c) for k in m.keys(c)}).eager()
m.remove("a");  v.value   # {'a': 1, 'b': 2}  — stale, and permanently deaf
m.set("c", 3);  v.value   # {'a': 1, 'b': 2}  — still
```

That is `KeyedFold.clear()` exactly, which is how it showed up. `_EagerPuller._drop_cached` now re-registers the memo edge and marks the owner stale; `Computed.value` re-materializes through the puller on the next read — the first moment it can recompute without running an effect inside `dispose`. The eager form now agrees with the lazy one after an upstream disposal (both recompute; both raise `DisposedError` if the body still reads the disposed cell).

**A trap worth naming.** My first attempt at this fix appeared to do nothing. `lazily-py` keeps in-place mypyc `.so` files beside the sources, so `import lazily.signal` loads `signal.cpython-314-*.so`, not `signal.py` — every kernel edit is inert until `make compile`. The `.so` on disk was about six weeks older than the checked-out sources, which means the suite had been validating a stale kernel. Both regression tests were then verified against a neutered hook with a recompile on **each** leg, so neither is vacuous. Recorded in operator memory.

**Verification:** 18 new `KeyedFold` tests plus 2 kernel regression tests; `make check` green (913 tests, ty, ruff, `run_readme`, conformance coverage, interop peer, assertion-ordering, ci-reach). Docs in README, SPEC, CHANGELOG (`Added` + `Fixed`). Committed `lazily-py 6d54f3b`, superproject `484dcae3e1`, both pushed.

`#lzpypromegress`'s CI run (34616209898) came back green.

### Re: #lzpyworkflowdeterminism — make divergence loud — opus-5

> **Queue prompt:** do [#lzpyworkflowdeterminism]

`lazily.workflow`. A reactive graph is already a pure function of its sources, which is most of what durable replay wants — until something in it reads the wall clock. Then replay diverges *silently* and surfaces much later as a workflow that will not complete. The module's whole job is to move that failure to the line that caused it.

**No dependency on `temporalio`.** The engine is injected as a clock accessor plus a two-method scheduler, so the same code runs against temporal.io, a test double, or any other durable engine. Adding the dependency would have bought nothing and pinned lazily to one engine's release cadence.

```python
wf = workflow_context(ctx, workflow.now, scheduler=TemporalScheduler())
timer = wf.register(TimerCore(fire_at=wf.clock.tick() + 5_000))

with deterministic_scope():
    while not timer.fired():
        await wf.schedule_next()   # a durable engine timer, not a sleep
        wf.advance()               # one clock reading drives every source
```

**The clock raises instead of clamping.** `ManualClock.advance` clamps a backwards move — correct for a game loop, wrong here. A replayed clock cannot regress, so a backwards reading means something is feeding it wall time, and clamping would hide precisely the bug this module exists to surface. That asymmetry is the one design decision I'd flag for review.

**One reading per `advance()`.** The context owns the registered `TimelineSource` set and ticks all of them from a single clock read, so two sources can never disagree about "now" — the shape that makes a replay diverge from the original run. `next_wakeup()` deliberately distinguishes `0` (a source is already due; a real instruction to the engine) from `None` (nothing pending). `activity()` is the only sanctioned side effect: a reactive effect reruns on replay, and its I/O would rerun with it.

**The guard, stated at its real strength.** `deterministic_scope()` rebinds `time.time`/`time_ns`/`monotonic`/`monotonic_ns`/`perf_counter`/`perf_counter_ns`, the module-level `random` functions, `os.urandom`, and `uuid.uuid1`/`uuid4` to raise `NonDeterminismError`, restoring every one on exit — including on an exception, and nesting safely. It is **not a sandbox**, and I did not let the docs imply otherwise: it cannot intercept `datetime.datetime.now()` (a method on a C type whose attributes cannot be rebound), a reference bound before the scope opened, or anything a C extension does internally. Module re-import isolation is the correct layer for those — which is what temporal.io's own workflow sandbox does. Both limits are **asserted**, not just described: `test_a_reference_bound_before_the_scope_escapes_the_guard` exists so the claim cannot quietly become false.

**On the naming collision.** `lazily.temporal` now opens with a warning block stating plainly that it is not temporal.io — it is the time-operator family, "temporal" in the tense/logical-clock sense, and older than the engine — and pointing at `lazily.workflow`. I did **not** rename it: it is public API in nine bindings, and a breaking change across all of them buys exactly what that paragraph buys. A test asserts the note is present, so it is API rather than a comment someone can delete.

**Verification:** 42 new tests; `make check` green (955 tests, ty, ruff, `run_readme`, conformance coverage, interop peer, assertion-ordering, ci-reach). Docs in README, SPEC, CHANGELOG. Committed `lazily-py 5dd631c`, superproject `d942241a31`, both pushed.

`#lzpykeyedfold`'s CI run (34617355723) came back green, which also confirms the eager-`Computed` kernel fix passes CI, not just the local rebuild. Run 34618133123 for this commit is queued. Exchange is large (118 lines, 7 cycles in 30 min) — noting it rather than pausing the queue to compact.

### Re: do [#lzpyreplayproof] — opus-5

> **Queue prompt:**
>
> do [#lzpyreplayproof]


Shipped `lazily.replay` — the provability half of the gate `lazily.workflow` opened last cycle. 1461 lines across module + suite + docs; `make check` green (991 tests, ruff/ty/conformance-coverage/interop-peer/assertion-ordering/ci-reach all clean), committed and pushed as `6c3c111`.

**The tsift rule is the whole design.** A tsift excerpt is trustworthy because it records a body hash and *revalidates against the source bytes* before it is returned, so a stale body deterministically suppresses the cached answer. Here the event log is the source bytes: `ReplayFingerprint` carries the digest of the log that produced it, and `verify`/`check` compare that binding **before** any value. A fingerprint recorded against a different log raises `ReplayLogMismatchError` and is never compared — so it cannot pass by coincidence (two logs, same final total is a test in the suite) and is never misreported as a graph defect. `check()` is the non-raising form for value divergence and still raises here: a stale fingerprint is an unanswerable question, not a report. `stride` is recorded for the same reason.

**Divergence is located, not just detected.** Every event is a checkpoint (`stride=N` samples sparsely; the initial and final states are always captured), so the error names the *first* event where the values parted and the exact cell label. The suite pins that: a graph that takes one value from outside the log at `seq=1` fails at `seq=1`, not at the final state where it is still visible. `prove(log)` needs no recorded fingerprint at all — it records, re-replays, and compares, because a graph that is not a pure function of its log already disagrees with itself. `build` is called once per replay (asserted), so a harness cannot prove a graph against its own leftover state.

**Canonical or it raises.** `canonical_bytes` is type-tagged and length-framed: mapping and set members order by their own encoded bytes so iteration order is not part of a value, but `1` / `"1"` / `1.0` / `True` / `b"1"` / `[1]` / `{1}` are seven distinct digests, `["a","bc"]` ≠ `["ab","c"]`, floats encode by `float.hex()` so `-0.0` ≠ `0.0`, and enums/dataclasses carry their type name (a `StrEnum` member is not its string). An unencodable value raises `ReplayEncodingError` naming the path it was reached through — no `repr` fallback, because an address-bearing `repr` would report a false divergence on every run, which is the exact failure this module exists to prevent.

**The stated contract, on the machinery that already had it.** `replay_log_from_outbox` turns `reliable_sync`'s `DurableOutbox.replay_from` into a fingerprinted log, using outbox epochs as event seqs so an ack-truncated prefix changes the log digest instead of silently shifting every event (pinned: `ack_through(3)` invalidates a fingerprint taken at `ack_through(2)`). `LatestDurableProjectionCore` is proven under the contract over a ten-event upsert/claim/fail/reclaim/ack/reconnect trace, including a wire round-trip of the fingerprint and refusal against an edited log. The contract in one line, now in SPEC.md and the module docstring: *given the same `ReplayLog`, a rebuilt graph observes the same values at every checkpoint — any deviation is a defect, not a tolerance.*

`deterministic=True` runs each replay inside `deterministic_scope()`, so a wall-clock read raises at the line that reached it rather than surfacing as a divergence one checkpoint later; the suite asserts both modes. Hashing is BLAKE2b-256 from `hashlib`, not BLAKE3 — lazily-py has no runtime dependencies and BLAKE3 is not in the stdlib. Digests are not wire-compatible with tsift's and are not meant to be.

CI run 34619335884 was still queued at hand-off; 34618133123 (the previous commit) came back green. Local evidence stands on its own. Filed the cross-binding port as a recommended follow-up at the queue tail rather than preempting `#lzpyprojectedchart`.

### Re: do [#lzpyprojectedchart] — opus-5

> **Queue prompt:**
>
> do [#lzpyprojectedchart]


Shipped `lazily.projected_chart` — `2c6fda2`, pushed. `make check` green (1011 tests, ruff/ty/conformance-coverage/interop-peer/assertion-ordering/ci-reach clean).

**The design decision the item turns on: the projection never refuses the authority.** `lazily.statechart` owns its transitions — you send it an event and it decides. That is the wrong shape when `workflow_run.status` lives in Postgres and is advanced by Temporal. So this chart decides nothing; you feed it observed states and it answers the two questions the authority cannot. Every non-stale observation is **adopted**, including a step the declared lifecycle forbids and a state the chart has never heard of, and the violation is recorded as an `IllegalTransition` counted on a reactive cell. Refusing the row would make this side silently disagree with the database it is projecting — worse than not modelling it. Adopting it quietly would say nothing. It adopts and says so. An undeclared state cannot wedge, because no deadline is knowable for it: the violation is the signal, not a fabricated deadline.

**The wedge is where the FPE bug lives, so it is where the semantics are strictest.** `wedged` is a derived cell over `now - entered_at > deadline`, strictly — at the deadline is not past it — so an effect or health cell on it runs exactly on the edge and not once per poll. The part that matters: **a repeated observation does not reset the deadline.** Re-observing the current state advances the authority timestamp but *not* `entered_at`, because a poller re-reading the same `started` row is evidence the run is wedged, not evidence it just advanced. Resetting there makes the wedge unreachable, which is precisely the failure `#fpelasterrorlww` was patching by hand. Terminal and deadline-less states never wedge — a finished run is not late.

**Two clocks, kept apart on purpose.** `at` is the authority's timestamp for the row and staleness is last-writer-wins on it, so out-of-order delivery cannot walk the state backwards — but an `at` older than the observer clock is *not* stale, which is the ordinary case of a row written five seconds before the poll that reads it. Dropping those would lose real transitions. `tick(now)` is the observer clock and raises on a backwards reading rather than clamping (`ManualClock` clamps, which is right for a game loop; a deadline computed off a regressing clock is not a deadline). Both must share a time base, which epoch-ms DB timestamps and a local reading naturally do.

`tick`/`next_fire` match `TimelineSource`, so the wedge registers with a `WorkflowContext` and becomes a durable engine timer instead of a polling loop. And because both clocks are inputs, `ProjectedChartCore` is a pure function of its observation log — the suite proves one under last cycle's `ReplayHarness` with wedge and violations in the fingerprint, so `#lzpyreplayproof` paid for itself one item later.

Scoped as a Python-side addition, not a spec family: `statechart` stays the conformant Harel/SCXML chart that owns its transitions, and the subject here is one external row per chart, so no thread-safe/async flavours and no cross-binding port implied. Filed the FPE adoption (drop the hand-written branch for the derived cell) into `src/haiven-dev/tasks/fpe.md`, where that work actually lives.

`#lzpyreplayproof`'s CI run 34619335884 came back green; 34620191066 for this commit is pending. Advisory `stale_install` and `stale_plugin` warnings noted — neither is owned by this cycle. Exchange is at 169 lines / 7 sections; noting it rather than stalling the queue to compact.

### Re: do [#lazilyspecbindings] — opus-5

> **Queue prompt:**
>
> do [#lazilyspecbindings]


Phase 1 landed across **12 repos**, all committed and pushed. `lazily-spec` `make check` exit 0 (all 13 guards), `lazily-py` `make check` exit 0 (1014 tests), and each of the 9 other bindings' conformance-coverage guard run individually to exit 0.

**The contract** — `lazily-spec/docs/replay-equivalence.md`, scored as an **optional (MAY)** row. That classification is the load-bearing decision: a replay proof is a verification facility, not a runtime feature, and optional rows are excluded from the family roll-up — so adding it cost **one regenerated line** per binding README instead of flipping `Reactive graph` from ✅ to `~` in nine of them. Three obligations, one fixture each:

1. **The fingerprint is bound to its log**, revalidated before any value compare. `fingerprint_log_binding.json` pins why both halves matter: logs `[+1,+2,+3]` and `[+3,+2,+1]` settle to the **same** sum 6 with **different** digests, so a value-only comparison passes and certifies nothing about the log in front of it — while a harness that compared anyway would blame the graph for a stale test artifact. The non-raising `check` form refuses it too.
2. **Divergence is localized.** `divergence_localization.json` drifts at seq 1 and requires seq 1 plus the cell label, not the final state where the defect is merely still visible. Stride is part of the fingerprint and a cross-stride compare is refused.
3. **The encoding is canonical or it fails.** `canonical_encoding_equality.json` names **no hex digest** — it asserts equality *classes* as same/different pairs, so each binding's hash and byte layout stay free. That was the right call for a reason phase 2 will feel: BLAKE2b is stdlib in Python and Zig and a third-party dependency in Kotlin, JS, Dart, C++ and C#. Demanding a shared digest would have bought five dependencies for something no two bindings ever exchange. Values are type-tagged because JSON cannot separate int `1` from float `1.0`, and integers carry decimal **strings** so 2^53+1 stays exact.

**lazily-py is now the reference runner, not just an implementation.** Its coverage guard went 142/153 → **145/156 fixtures opened** with the known-uncovered list unchanged at 11, so the ✅ on the new row describes bytes that were really read. On a `record` step the runner cross-checks that the recorded `sum` digest **is** the digest of the subject's final sum — without that the fixture would accept a harness that observed some other value entirely. Also added `ReplayStrideMismatchError`: the stride refusal was a bare `ReplayProofError`, so a driver could only distinguish it by matching on a message.

**Two corpus-convention corrections I had to make rather than work around.** My per-step claim key was named `property`, which is not in the corpus-wide `PROSE_KEYS` set — every binding's consumption guard would have demanded an assertion for a key carrying nothing comparable, and nine bindings would each have decided how to excuse it, which is precisely the divergence the prose-key convention exists to stop. Renamed to `note`, which is already exempt everywhere and therefore needs *no* declaration; a fixture-level `assertions.prose` is in fact refused, because prose is scoped to the block that carries it. And `lazily-cpp` has an area-level ledger above its fixture ledger, so `replay` had to be excused as an *area* there, not just fixture-by-fixture.

The publish order was forced and followed: `lazily-spec` first (its `corpus-published` guard refuses to let a binding verify against a working tree CI's clone will not see), then the bindings.

**Phase 2 is 8 per-binding harnesses**, filed as one actionable id each rather than re-gating a parent — `#lzreplayrs` through `#lzreplaycs`, in that order at the queue head. `#lzreplayrs` carries a design finding from this cycle worth not rediscovering: lazily-rs has no hash crate and keeps every dependency optional, so it should store the **exact canonical bytes** per label rather than take on blake2/sha2 for an optional facility — the spec deliberately leaves both the hash and the byte layout binding-chosen, and exact bytes are the degenerate strongest choice at test sizes. The same applies to C++.

CI: `lazily-spec` and `lazily-py` green; `lazily-rs` and `lazily-cpp` runs were still in progress at hand-off, and the earlier `lazily-cpp` red I checked was a **cancelled** run from 2026-09-03, not a failure. Advisory `stale_install` / `stale_plugin` warnings noted — neither is owned by this cycle. Exchange is at 190 lines / 8 sections; noting it rather than stalling the queue.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzreplayrs] [lazily-rs] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from scripts/check-conformance-coverage.sh KNOWN_UNCOVERED. DESIGN FINDING from phase 1: lazily-rs has no hash crate and every dep is optional, so do NOT add blake2/sha2 for an optional verification facility — the spec leaves hash AND byte layout binding-chosen, so store the exact canonical bytes per label instead (test-sized, zero collision risk, zero dependency). Mirror ReplayStrideMismatchError as a distinct error variant so a driver routes on the type, not a message. Repo: src/lazily-rs
- [ ] [#lzreplaykt] [lazily-kt] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. Kotlin/JVM has MessageDigest SHA-256 in the stdlib, so no Bouncy Castle dependency is needed. Repo: src/lazily-kt
- [ ] [#lzreplayjs] [lazily-js] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. node:crypto covers the digest; keep it out of any browser bundle path or use the exact-bytes form. Repo: src/lazily-js
- [ ] [#lzreplaydart] [lazily-dart] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. Repo: src/lazily-dart
- [ ] [#lzreplayzig] [lazily-zig] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. std.crypto.hash.blake2 is in the stdlib. Verify all three pinned toolchains. Repo: src/lazily-zig
- [ ] [#lzreplaygo] [lazily-go] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. crypto/sha256 is stdlib; golang.org/x/crypto/blake2b would be a new module dependency. Repo: src/lazily-go
- [ ] [#lzreplaycpp] [lazily-cpp] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures, drop them from KNOWN_UNCOVERED and move 'replay' from EXCUSED_AREAS to REQUIRED_AREAS. No stdlib hash: use the exact-canonical-bytes form rather than vendoring one. Keep the core wasm-clean. Repo: src/lazily-cpp
- [ ] [#lzreplaycs] [lazily-cs] ReplayHarness per docs/replay-equivalence.md; open the three conformance/replay/ fixtures and drop them from KNOWN_UNCOVERED. System.Security.Cryptography.SHA256 is in the BCL. Repo: src/lazily-cs
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzagentloopcibilling] [operator-action] GitHub Actions is blocked on btakita/agent-loop by a spending limit, and has been since 2026-08-09. Every run of both workflows (Agent Doc, Lazily network suite) reports "The job was not started because recent account payments have failed or your spending limit needs to be increased" — no job starts, so there are no logs and nothing to debug in the workflow itself. Diagnosed precisely: btakita/agent-loop is PRIVATE, so it consumes paid Actions minutes; btakita/tsift, btakita/agent-doc and every lazily-hub/* binding are PUBLIC and run on free minutes, which is why binding CI has been green throughout while the superproject has been dark. Last successful run 31293624224 at 2026-08-09T04:00:25Z; first billing failure 31297858292 at 2026-08-09T05:58:50Z. COVERAGE LOST for that whole window: the `check` job (make check under tmux) and the `tmux-ci` job (make tmux-ci) in agent-doc.yml, plus the tiered `network-suite-*` targets in lazily-network-suite.yml — none of which is covered by the per-binding CI that has been green. OPERATOR ACTION, and nothing else unblocks this: open Billing & plans for the account that owns btakita/agent-loop, settle the failed payment or raise the Actions spending limit, then re-run one workflow and confirm a job actually starts. Until then treat superproject CI as ABSENT rather than passing, and do not read a green binding CI as coverage for it. Once jobs start again, re-run make check and make tmux-ci on HEAD before trusting the backlog of submodule-pointer commits pushed during the outage.
- [/] [#lzstdlibmatrixrelease] Implement Phase 5 of tasks/osoftware/plan-lazily-stdlib-conformance-rollout.md: run the capability-driven all-peer/all-channel matrix, enforce CI and mutation gates, publish coverage evidence, and release bindings inside-out only after every claimed cell is green.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
- [/] [#lzjspublish] After the operator completes npm security-key reauthentication in the Brave package-access tab, configure the GitHub Actions trusted publisher for lazily-hub/lazily-js using workflow release.yml with npm publish permission; rerun release job 30309980372 for v0.30.1 and verify the registry resolves 0.30.1.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
eturn typed satisfied/timed-out/cancelled/disposed-or-unavailable outcomes; optionally prove keyed side-effect receipts while leaving receipt storage and transport policy outside the primitive. Test lost wakeups, already-satisfied state, revision races, cancellation, deadlines, disposal, and keyed receipts; document using the stdlib Timer as the deadline budget.
cate orphan line remains outside all agent-doc components at line 158. Please delete only the stray `- [ ] [#lzstdlibbarrier] ...` line in IDEA, save, and send `continue`. The completed item is already archived, so removing this duplicate is the only remaining closeout step.
ebc036
