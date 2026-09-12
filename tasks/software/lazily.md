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
prompt_presets:
  '#actionable': Add actionable tasks into backlog + queue
  '#actionable-review': Add actionable review items into backlog + queue
queue: go
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

### Re: Run the backlog items in subagents — opus-5

> **Queue prompt:**
>
> do [#lzreplayrs]
>
> do [#runbacklogitems]


> **Queue prompt:**
>
> - Run the backlog items in subagents.

Eight subagents, one per binding repo, all eight landed. Phase 2 of the replay rollout is complete: `lazily-rs` `4a80ac4`+`5970853`, `lazily-kt` `cf03cdb`, `lazily-js` `0ab67de`, `lazily-dart` `98b5736`, `lazily-zig` `557ecba`, `lazily-go` `8886dc9`+`d1438af`, `lazily-cpp` `c8b8048`, `lazily-cs` `17a9655`. Every one is green locally with an explicitly captured exit code and green on its own CI run. Plus `lazily-spec` `7828886` for the piece no per-repo agent was permitted to touch.

**The corpus fixture does not prove the obligation it is aimed at.** This is the finding that outlives the cycle. Obligation 3's member-framing row is pinned by `["a","bc"]` vs `["ab","c"]` — and with a type tag in front of every member, those two already differ as byte strings. An encoder that drops the length prefix entirely still passes. lazily-js and lazily-cs each ran that exact mutation, saw it survive, and both concluded it was benign because "the tag still delimits." lazily-kt did not accept the argument — it constructed the pair that actually collides without the length, `["a","sbc"]` vs `["as","bc"]`, and the mutation goes red. lazily-rs found it independently from the other direction. So four of eight agents met this and two of them mis-ruled it. The fixture is not wrong — it asserts a real equality class — it is *insufficient*, which is worse in a corpus, because nine bindings can pass it with a genuinely under-specified encoder and each will believe the fixture cleared them. Filed as `#lzreplayframing` rather than landed here: a new canonical fixture reddens all nine bindings at once and needs a cycle scoped to ten repos, not a drive-by at the end of this one.

**Phase 1's cost estimate was wrong, and two red CIs are the evidence.** Scoring the row optional (MAY) was the right call and I would make it again, but the claim that it cost "one regenerated README line per binding" only held for the README. The per-binding corpus guards do not read `optional`. Once `lazily-spec` published a `replay/` area, `lazily-js` went red on its conformance guard and `lazily-cpp` went red in all three CI jobs on its WASM.md corpus-column guard — for an area no runner replayed. Neither agent was sent to fix a red; both discovered their repo was already broken and this work is what closed it. The optional flag protected the scorecard, not the guards. Worth carrying into the next spec-side addition: publishing a corpus area obligates every binding's guards immediately, whatever the row's MUST/MAY score says.

**The dependency finding held everywhere it was applied.** `lazily-rs` and `lazily-cpp` took the exact-canonical-bytes form and added nothing — rs keeps every dep optional, cpp has no stdlib hash worth vendoring. `lazily-dart` and `lazily-js` reached the same conclusion on their own terms: Dart's pubspec still has zero runtime deps, and JS keeps `node:crypto` off every bundle path, so `.size-limit.json` never moved. The four with a stdlib digest used it — kt `MessageDigest`, zig `std.crypto.hash.blake2`, go `crypto/sha256`, cs `SHA256`. Nine bindings, nine hashes, zero new dependencies, which is exactly what leaving the hash binding-chosen was for.

Per-language traps each got handled rather than papered over. JS parses the corpus's decimal-string integers as `BigInt` — `Number("9007199254740993")` is already its even neighbour, so the adjacent-large-integer steps would have compared a value against itself. Zig's error values carry no payload, so divergence detail travels through an out-parameter, and every label a report names is duped because `verify` frees the fingerprint and a borrowed label would dangle on precisely the failure path that has to stay readable. Go has no set type, so `ReplaySet` is the explicit stand-in keeping set order out of a value while slice order stays in. C++ was already safe from the `emcc -fignore-exceptions` trap because `tests/wasm.cmake` passes `-fexceptions`, so its throw-based refusals are genuinely observed under wasm.

Nobody's green was taken on faith. kt read the JUnit XML timestamps to confirm `:test` actually ran rather than trusting the Gradle summary. zig re-ran `zig build test` raw after `rtk proxy` printed six `failed command:` lines while exiting 0, and verified all three pinned toolchains end to end. go ran the race detector and grepped for `DATA RACE` explicitly. cpp built cold after `make clean`. Between them they ran 50-odd mutation probes, restoring from scratch copies rather than `git checkout`, and every re-pinned coverage floor was proved exact by failing at n+1.

For `lazily-spec` I flipped the `replay-equivalence-proof` marks to shipped for all nine implementing bindings — GDScript stays absent, there being no binding yet — and regenerated `docs/coverage.md`. `make check` exit 0, all thirteen guards. I then probed that the claims guard is live for this row by marking GDScript shipped: it fails, naming all three fixtures and GDScript's own `KNOWN_UNCOVERED` entries. So the nine marks are backed by each binding's ledger, not by my assertion. As predicted by every agent independently, no binding README changed — optional rows are excluded from the family roll-up. CI run 34633130674 is queued at hand-off.

Two things I did not do. `#lzreplayframing` is above. `#lzcppscenfloor` is the smaller one: five bindings re-pinned their scenario floors during phase 2 and `lazily-cpp` left its slack at 149 against a reported 151, for scope discipline — pre-existing drift, but by that script's own rule a slack floor has stopped guarding. Both are actionable backlog items, not review gates. Exchange is at 215 lines / 9 sections with the queue active; noting it rather than stalling to compact.

### Re: do [#lzreplayframing] — opus-5

> **Queue prompt:**
>
> #actionable-review


> **Queue prompt:**
>
> do [#lzreplayframing]
>
> do [#lzcppscenfloor]


Ten repos, ten green. `lazily-spec` `4010d99` published first so every binding's CI clone would replay the same corpus, then nine subagents — one per binding, never two in a repo — landed `lazily-rs` `0832acd`, `lazily-py` `829b372`, `lazily-js` `6798b78`, `lazily-kt` `75f1bd4`, `lazily-go` `2a13cab`, `lazily-zig` `a1a93f5`, `lazily-cpp` `3710cf3`, `lazily-cs` `c4f51ba`, `lazily-dart` `ddf069b`. Every one green locally with an explicitly captured exit code and green on its own CI run. `#lzcppscenfloor` closed in the same push as `lazily-cpp` `637f85c`. Then `lazily-spec` `a4548e6` for the correction the rollout itself earned, which is the interesting half.

The fixture now carries three member-framing rows instead of one. The old `["a","bc"]` vs `["ab","c"]` stays, asserting the equality class. `["a","sbc"]` vs `["as","bc"]` plus the mapping analogue `{"a":"sb"}` vs `{"as":"b"}` were lifted from `lazily-kt`'s own suite. And a new one nobody had: `[["a"],"b"]` vs `[["a","b"]]`, which collides under an unframed concatenation in *every* layout, because a nested container's boundary has no tag to hide behind.

**Not one binding's encoder was defective.** All nine were genuinely length-framed. The entire defect was in what the tests proved, which is the more expensive kind, because nine repos were each carrying a green assertion they believed covered this.

**The layout is unanimous, and that is not what the spec says.** Obligation 3 says a binding **MAY** choose its own byte layout, and reasons carefully about why demanding a shared hash would buy a dependency in five bindings for nothing. Nine bindings independently chose the identical frame: `<tag><decimal length>:<body>`. Not similar — the same netstring, down to the colon. The freedom is real and nobody used it. That means the corpus's layout-dependent rows do fire everywhere today, and it also means the "reference layout" hedge in the fixture was hedging against a thing that did not happen.

**What did happen is that a two-part length prefix fails in more than two ways.** `lazily-go` found it first: its frame is `s<digits>:`, so under a mutation that drops only the digits the content a member must spell is `s:`, not `s` — and the corpus pairs stay green. `lazily-js`, `lazily-dart`, `lazily-py`, `lazily-rs`, `lazily-cs` and `lazily-cpp` each ran that second mutation independently and each found the nested row was the only thing that killed it. Then `lazily-cpp` ran a third: remove the count from the **leaf** frame only, leave containers framed. All fourteen corpus steps pass. Nothing in the corpus reaches it, and nothing can — the corpus has to choose its colliding content in advance, and that content has to spell `s:`, a string the corpus has no standing to know. `a4548e6` writes those three shapes into a table and turns the binding-local pair from advice into **MUST**: construct the colliding pair for each *level* of frame you emit, leaf and container, spelling the whole surviving prefix rather than just the tag, and prove each by running its own mutation on a cold build.

**Every binding's runner had a slack step floor, and the new rows would have passed without running.** Eight of nine pinned `steps.len() >= 11` against a fixture that now carries 14; `lazily-cs` hard-coded `Assert.Equal(11, ...)` and was the only one that failed loudly instead of silently accepting three steps it never replayed. That is the second finding and it generalizes past this row: publishing a corpus step is invisible to a binding whose floor still describes the old fixture. `lazily-zig` said it plainly — the three new rows would have landed inside the old slack and reported green.

Pre-existing floor drift surfaced and closed on the way through, none of it caused by this work: `lazily-cpp` `MIN_SCENARIOS` 149 → 151 (that is `#lzcppscenfloor`, traced to lazily-spec `1c388a5` moving `MIN_FIXTURES` and not `MIN_SCENARIOS`), `lazily-py` `MIN_FIXTURES` 141 → 145 and `MIN_DECLARED_BLOCKS` 578 → 620, `lazily-js` `MIN_BLOCKS` 635 → 638, `lazily-cs` `MIN_BLOCKS` 740 → 743. Every re-pinned floor was proved exact by failing at n+1, and every one was pinned against the *published* corpus, not a working tree.

Four repos were carrying comments that asserted the false version of this property in prose — `lazily-py`'s `test_length_prefixes_keep_concatenations_distinct` claimed "without length framing both encode to the same member bytes", `lazily-zig`'s `frame()` doc said the same, `lazily-rs`'s `AGENTS.md` recorded "Dropping only the LENGTH stays green here" as a known-and-accepted fact. All three corrected in their own commits. `lazily-kt`'s comment was right; it was the one that found this.

On method: nobody's green was taken on faith. `lazily-kt` read JUnit XML timestamps to prove the test task advanced rather than reporting `UP-TO-DATE`; `lazily-go` cleared the test cache and grepped for `(cached)`; `lazily-zig` built into a freshly created cache dir it verified did not exist, and re-ran all three pinned toolchains raw after the known `rtk proxy` exit-code trap (the six `failed command:` lines are real, pre-existing, and are the build runner annotating steps that wrote to stderr on a `1097/1097 passed` summary); `lazily-cpp` asserted `test ! -d build` before each configure; `lazily-py` confirmed `replay.py` is not one of the five mypyc modules before trusting a source-level mutation. Every restore came from a scratch copy, never `git checkout --`, and the shared corpus at `../lazily-spec/conformance` was never written by any of the nine.

Exchange is at 244 lines / 10 sections with the queue active — noting it rather than stalling to compact.

### Re: do [#lzcorpusfloorguard] — opus-5

> **Queue prompt:**
>
> do [#lzcorpusfloorguard]


> **Queue prompt:**
>
> queue: go

Go-mode acknowledged; nothing new to execute from the flip.

Ten repos again. `lazily-spec` `76b79cf` landed the guard, then `d411b7c`, `fd25283` and `804c9d0` corrected it — **each of those three fixes came from a binding agent's report, not from me**. Nine binding commits: rs `260f41a`, py `5953ad1`, js `18c8459`, kt `169f380`, go `b8fe008`, zig `66c370a`, cpp `1e6510e`, cs `88d02d0`, dart `93dd418`. Eight are CI-green; `lazily-rs` run 34641609398 is still in progress with nothing red (Signaling, Lean and the benchmark gate already passed, the long Rust job is running) — recorded as pending, closed out on local evidence. Spec CI 34641783183 green.

**The guard, in two directions of one idea: a count another repo derives from must move deliberately.** `corpus-counts.json` pins the corpus's own array lengths, and each binding's declared `MIN_FIXTURES` / `MIN_SCENARIOS` is re-derived from the corpus plus that binding's own `KNOWN_UNCOVERED` ledger and must match *exactly* — slack fails because slack has stopped guarding, and a floor above reality fails too, naming the already-red binding in one line. The derivation reproduces every declared floor on the nose. Then the bindings retired the constant the incident was actually about: a per-fixture step count hard-coded in the runner is replaced by *executed == loaded* plus *an unknown op type is a hard failure*, neither of which has a number to re-pin.

**The same tautology was live in three bindings.** `lazily-kt`'s ingress `replay()`, `lazily-dart`'s ingress `_replay`, and all four of `lazily-rs`'s queue-family `replay` functions returned `steps.len()` — the **loaded** count — and the caller compared that against the loaded count. `expect(steps, corpusStepTotal())` could not fail. A dispatch loop that walked past a step still reported it replayed. Three agents found the identical shape independently in three languages, which makes it a family-level design slip rather than three mistakes.

**The constants were not confined to replay.** The sweep turned up roughly sixty-six hard-coded corpus counts across nine repos — `lazily-cs` twelve, `lazily-py` twelve, `lazily-rs` ten, `lazily-cpp` nine, `lazily-zig` five, `lazily-dart` five, `lazily-kt` five, `lazily-js` four, `lazily-go` four — in statechart, distributed, lossless-tree, ingress, egress, queue-family, stdlib, codec and blob runners. Every agent also drew the line correctly in the other direction and left alone the counts that describe the *runner's own* ledger (a 3×3 flavor matrix, a fixture list declared forty lines up), which are not corpus-coupled and are right to be literal.

**Nobody was silently skipping unknown ops — but two were failing with the wrong message.** All nine dispatches already ended in a throw. `lazily-js` had the fingerprint lookup *above* that throw, so a new op without a `fingerprint` field died as `no fingerprint named 'undefined'` — reads as a corrupt fixture, not as a new op. `lazily-dart` built four locals eagerly before the branches, so an unrecognized op died on a cast. Both now reach the named error. `lazily-zig`'s two replay chains returned their error mute, and now name fixture, step index and op type — emitted inside the loop while the parse arena still owns those bytes, because stashing the label for a post-`deinit` report is the borrowed slice that dangles on exactly that path.

**The guard I shipped was wrong three times, and the fan-out is what found it.** `lazily-js` reported that it *does* declare `MIN_SCENARIOS=157`, in `check-scenario-coverage.mjs` rather than the coverage guard — so my one-file read produced a false "no floor declared" *and* left a real 157-scenario floor entirely unaudited; the whole `scripts/` directory is the ledger now, and two different values for one floor is a hard failure. `lazily-kt`'s run reporting 148 fixtures / 157 scenarios against my derivation's 147 / 151 exposed that `REQUIRED_AREAS` scopes the **audit**, not the set the suite **opens**; `lazily-cpp` pairs it with `EXCUSED_AREAS`, whose own guard enforces an exact complement, and there the derivation lands exactly. Without that complement the guard now declines to state a number rather than state a wrong one — the previous revision would have called kt's honest `MIN_FIXTURES=148` *unsatisfiable* against its own bad arithmetic, and a guard that manufactures a false failure against a correct floor is worse than no guard. And `lazily-dart` pointed out the manifest pinned `steps` and `scenarios` while its stdlib runner counts `mutations`, so a shrinking mutation ledger was caught nowhere; it now pins **all** 151 array lengths across 144 fixtures, and `mutations` 6→5 and `frames` 4→3 both go red where neither was observable before.

Two artefacts of the drift clock are worth keeping. `lazily-cs`'s `MIN_BLOCKS` comment reads *"Re-pinned from 740 for lazily-spec 4010d99 (#lzreplayframing), which grew canonical_encoding_equality.json from 11 steps to 14"* — the constant recording, in its own words, being reset by my commit from the previous cycle. And `lazily-cpp`'s statechart floor was `>= 33` against a 33-step corpus: zero slack, which is exactly what "just re-pinned" looks like, and a mutation still sent 26 of 33 past it.

Method again held. Cold builds proved cold rather than asserted: `lazily-zig` verified its `--cache-dir` absent before creating it and used an isolated `ZIG_GLOBAL_CACHE_DIR`; `lazily-cpp` asserted `test ! -d build` before each configure and ran a **control** — the same cold binary against the same scratch corpus with the new step removed exits 0 — so the failure is attributable to the step and not to the scratch directory; `lazily-go` cleared the test cache and grepped for `(cached)`; `lazily-kt` read JUnit XML timestamps; `lazily-py` confirmed `replay.py` is not one of the five mypyc modules before trusting a source mutation, then ran the decisive probe — turning its `raise` into a `continue` against a fifteen-step scratch corpus, which the old `>= 14` floor would have passed. Every restore came from a scratch copy, never `git checkout --`, and not one of the nine wrote to the shared corpus.

Exchange is at 278 lines / 11 sections with the queue active — noting it rather than stalling to compact.

### Re: queue: go — opus-5

> **Queue prompt:** do[#lzblockfloorpin]

Go-mode taken. `#lzblockfloorpin` is mid-flight: the spec half is landed, pushed and CI-green, one of four bindings has landed, three are still running.

**lazily-spec — `67aa54a` (CI ✅) and `9b55539` (CI ✅).**

`corpus-counts.json` grew a `blocks` section: per fixture, per block-name bucket, the number of assertion-block **sites**. 795 across 148 fixtures.

The dimension really was unguarded, and I proved it rather than asserting it. Deleting one `expected` block from a scratch copy of `windowing/tumbling_count.json` leaves **HEAD's guard printing `corpus floors OK`** — an array pin cannot see it, because removing a key from a step that stays changes no array length. The new guard says `` `expected` SHRANK from 6 to 5``, one problem, with an unperturbed copy of the same corpus green in the same run.

Buckets keep `expect` apart from `expect[]` because the bindings genuinely disagree about what a block is. `signaling/anti_spoof_session.json` carries an eight-element array-valued `steps[].expect`; lazily-js instruments each element, lazily-cs's object-only walk sees none of them. One total would have been eight too high for one of them whichever way it went.

`--report-blocks` / `make corpus-blocks-report` is the other half — the number a binding should derive instead of type. **It reproduces three of the four typed constants to the unit: cs 743 sites, py 620 digests, js 638 digests.** That exactness is what makes the constants removable; a derivation that landed "about right" would just be a second thing to re-pin.

**lazily-cs — `362f16c`, landed.** `MIN_BLOCKS=743` and its three-generation re-pin comment are gone; the guard lists the corpus, subtracts `KNOWN_UNCOVERED`, walks the 151 opened fixtures with a python twin of `SpecCorpus.DeclareWalk`, and asserts `743 == 743`. `make check` EXIT=0. Both probe directions fail as they should — and the growth direction is the one the retired `>=` floor passed silently, which is the direction all three of its re-pins actually arrived in.

**lazily-js, lazily-py, lazily-zig — in flight.** Their derivations are already verified against the corpus (js 638 distinct digests over five names plus array elements; py 620 over five names, objects only; zig 31), so these are landings, not open questions.

**zig's 31 is not a drifted floor, and that is the finding.** I reproduced it exactly — 37 sites / 31 distinct digests — by simulating `recordDeclaredBlocks`, which reads `assertions` at the top level plus one hard-coded level inside `frames`/`scenarios`/`rejects`. Its 138 opened fixtures carry 716 sites, so rung 0 sees 5.2% of them and every `expect`/`expected` block in the corpus sits outside it. That is verbatim what lazily-py had before widening from 31 to 578. Widening zig's walk is runner work that will surface hundreds of blocks needing a binding or an excuse, so the zig turn only stops it *typing* 31 — filed as `#lzzigblockwalk` with the numbers, so the narrowness stays visible instead of becoming settled.

**One adjacent hole found and closed.** The generated, fail-closed, routed `schemas/assertion-blocks.json` routed 155 of 158 assertion-block pointers. The three it missed were the six `expect_initial`/`expect_after` blocks in `collections/semtree_incremental.json` and the eight-element array-valued `steps[].expect` — and the test that checks the route ledger is exact in both directions could not see them either, because it walks the corpus with the same three-name constant. A blind spot consistent with itself reports OK. 158 routes now, none removed, and verified constraining rather than merely present: `surprise_key` is rejected on both new routes where the unmodified block validates. Two runners were inventorying blocks no schema constrained.

Exchange is at 309 lines / 12 sections — noting it, not stopping for it.

### Re: queue: start — opus-5

Queue restarted, head drained, and `#lzblockfloorpin` closed out on evidence rather than on last cycle's "in flight".

**`#lzblockfloorpin` — all five repos landed, pushed, CI green.** `lazily-spec 9b55539`, `lazily-cs 362f16c`, `lazily-js bf2fb79`, `lazily-py 302736e`, `lazily-zig 40d4e28`, every one at its remote HEAD with a green run on that exact sha. Marked done.

### Re: do[#lzzigblockwalk] — opus-5

> **Queue prompt:**
>
> do [#lzblockfloorpin]
>
> do [#lzzigblockwalk]


Widened, and the widening is where the work was. `lazily-zig aecf1a1` (CI ✅), `lazily-spec 4718d90` (CI ✅), superproject `11505ffd4`.

**The walk: 37 sites / 31 distinct digests → 722 / 613.** Rung 0 read the top-level `assertions` key plus the `assertions` key of each element of the top-level `frames`/`scenarios`/`rejects` arrays, and nothing else — 5.2% of what the same 138 opened fixtures carry, with every `expect`/`expected`/`expect_initial`/`expect_after` block in the corpus outside it. The rule is now the whole of `BLOCK_NAMES` at every depth, object-valued only, a block emitted and not descended into, with `name`-preferring list labels: lazily-py's `iter_declared_blocks` rule, mirrored in the guard's derivation so the declaring side and the expectation cannot walk differently.

**204 digests over 239 sites had no binder. 214 now bind, across nine runners.** The largest was `reactive_graph_conformance`'s `evaluateExpect` — a hand-rolled key loop that refused an unknown key and bound nothing, so ~100 step blocks reported exactly nothing. Binding it immediately failed a rung that had never been able to fire here: `#lzsubblockkeyset`, on `read`/`readable`/`dependents_of`/`dependencies_of`/`computes_of`/`scope_owned_count`. A key-set comparison would be *wrong* for those — they are keyed by node id and a fixture names the subset it cares about, not the graph — so they descend into a child tracker and the obligation moves down instead.

**Three findings the widening produced, not a clean sweep.**

`reliable_sync` had re-typed all twelve `expect` blocks as Zig literals with the key name in a trailing comment — `try testing.expectEqualSlices(u64, &.{ 42, 43 }, retained); // retained_after_ack`. A real comparison against a constant, so upstream could move any of those numbers and the file kept agreeing with itself. `crdt_tree` never read its three `crdt-tree/algebra.json` blocks at all; `expectFrontiersEqual`/`expectSameOpIds` are now predicates so the fixture's booleans decide the verdict, and an `expect*` helper that failed on the spot could never have answered for a corpus declaring `texts_equal: false`. And `arena_blob.json`'s `payload_region` was carried by the fixture and read by **nothing** — which is the whole shape of an unbound block.

**25 remain excused, per site.** They are steps of the six `EXPECTED_SKIPS` fixtures where the replay stops on a `merge_cell` op or the `drain_exhausted` key this binding does not implement, so the later blocks are unreachable rather than unbound — the same class lazily-py excuses for the same fixtures. Per site and not per fixture so that when the op lands each entry fails as stale one at a time. The ledger also grew the two directions it lacked: an excuse naming a site this run did not inventory, and one naming a site whose digest a runner *does* bind, each now fail. Both probed.

**Proven blind, not argued blind.** Deleting one nested `expected` block from a scratch copy of the corpus fails the new guard (613 declared vs 612 derived) while the walk it replaces reports 37 sites / 31 digests either way. Unperturbed control green in the same run.

**One adjacent defect, found because it made an honest floor look broken.** lazily-spec's `check-corpus-floors.mjs` found an array by `NAME=(` and then scanned for the next line beginning `)`. For a same-line `NAME=()` that close belongs to the *next* array, so lazily-zig's 25 new `KNOWN_UNBOUND_BLOCKS` entries read as 25 scenario excuses and silently subtracted 25 from the derived count: `MIN_SCENARIOS=151` was reported "UNSATISFIABLE, above the 126 the corpus plus that binding's ledger can produce". Fixed in the parser and proven against a tree where zig still spells it on one line — fixed parser exit 0, old parser exit 1. `lazily-cpp` and `lazily-dart` ship the same shape and were one populated neighbouring array away from the same false failure.

**Verification.** `make check` exit 0 in both repos: zig's fmt on both pinned toolchains, full suite, interop peer, Lean formal model, 613/613 blocks bound, 138/156 fixtures, 151/151 scenarios, assertion ordering, ci-reach; spec's 13 guards. `assertKeyWith` callbacks needed two shapes fixed for lazily-spec's consumption guard — a multi-line signature whose `zig fmt` trailing comma reads as an empty final parameter, and callbacks resolved by name rather than through a generic factory or a local `const`.

Exchange is at 335 lines / 13 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzblocksitepin] — opus-5

> **Queue prompt:**
>
> do [#lzblocksitepin]


Done in **four** bindings, not three, and each one has a probe that discriminates between the two dimensions rather than a claim that it does.

| binding | sites | distinct digests | commit | CI |
|---|---|---|---|---|
| lazily-zig | 722 | 613 | `bf512e2` | ✅ |
| lazily-py | 729 | 620 | `48cff68` | ✅ |
| lazily-js | 747 | 638 | `ae58951` | ✅ |
| lazily-cs | 743 | 634 | `0b59e41` | ✅ |

Derived and inventoried agree on the nose in all eight cells. Every comparison is an equality, both dimensions come off the one walk the inventory itself uses, and every expectation still comes from the corpus listing minus that binding's own ledger — never from a run artifact.

**The fourth binding is the finding.** The item scoped py/js/zig because those three derive digests. `lazily-cs` was the mirror image: it asserts 743 **sites** and carries a comment arguing that sites beat digests — which is true and is only half of it. The two dimensions are blind to what the other sees, in *opposite* directions. A digest count absorbs a deletion of a block whose bytes recur elsewhere. A site count absorbs a **content edit**: rewriting one block so it is spelled exactly like another's leaves 743 sites untouched and takes the digest set from 634 to 633, and the corpus has genuinely lost a distinct claim. So cs got the digest half, with `SpecCorpus.BlockDigest` ported rule for rule — numbers folded by their raw lexical form, everything else as `z` — and verified against the run's own ledger *before* being asserted.

**Each probe had to be the one where the new dimension is the sole signal.** That took more care than expected and two of the three agents rejected their first candidate for it:

- **zig** — deleting `stdlib/timer.json` `scenarios[0].steps[0].expect`, `{"outcome": "pending", "deadline": 10}`, a shape several stdlib steps share. This is the exact perturbation that ran GREEN earlier in this session under digest-only: now 722 sites vs 721.
- **cs** — collapsing `scenarios[2].steps[0].expect` from `{"outcome": "pending", "deadline": 7}` (a unique digest) to `{… "deadline": 10}`. The digest rung fires at 634 vs 633 and the site rung stays **completely silent** — zero occurrences in the output.
- **py** — `rateshape/debounce.json` `steps[0].expected`, a shape recurring at 15 sites. Chosen over the timer block because deleting that one aborted a fixture-looping runner, which shrank the opened set and moved the digest count too: a confounded probe. Then it ran HEAD's files against the same doctored corpus and got exit 0 / `620 == 620` — green over a corpus that lost a block.
- **js** — detaching `rateshape/debounce.json` from the opened set: 7 blocks whose 4 distinct digests all recur in `sample_time.json` and `throttle_trailing.json`. Site equality red at 740 vs 747 naming all seven; the same inputs against a digest-only copy of the guard, exit 0.

**One limitation, measured rather than assumed.** js tried the probe as I specified it — deleting `coordination/lock.json` `steps[2].expected` from the corpus — and found it does **not** discriminate: both sides went 747 → 746 and the guard stayed green. Both sides of both equalities read `SPEC_DIR`, so a corpus deletion moves the expectation and the inventory together. No expectation derived from the corpus can see the corpus itself move. That applies to all four bindings — my zig probe only fired because it held a manifest from an unperturbed run against a perturbed corpus, which is not what CI does. **What is pinned here is the agreement between the run and the corpus, per site and per digest;** the corpus-against-its-own-history half is `lazily-spec/corpus-counts.json`, which `#lzblockfloorpin` already grew to pin 795 block sites across 148 fixtures. The pair is complete across the two repos, and js recorded the boundary in its source so nobody re-derives it.

js also reports its walk counts **array elements** individually, so the row to read against `make corpus-blocks-report` is 747/638 (all five names + array elements), not 735/626.

`#lzblockmagnitudeaudit` edited to say a magnitude now means both dimensions, so the six bindings it covers cannot land a digest-only pin.

**Verification.** `make check` exit 0 in all four repos, captured explicitly: zig (fmt on both pinned toolchains, full suite, interop peer, Lean model, 613+722, 151/151 scenarios), py (1017 tests, 729+620), js (665 tests, 747+747 as *sets* — two sites that move cancel in a count and cannot cancel in a set), cs (332 tests, 743+634). Every zero-guard covers either dimension. Nothing touched `lazily-spec`'s shared corpus; all perturbations were `/tmp` copies.

Exchange is at 370 lines / 15 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzblockmagnitudeaudit] — opus-5

> **Queue prompt:**
>
> do[#lzscenariofloormissing]


> **Queue prompt:**
>
> do[#lzktareapartition]


> **Queue prompt:**
>
> do[#lzorderinganchors]


> **Queue prompt:**
>
> do [#lzblockmagnitudeaudit]


Audited all six. **Five had a rung-0 ledger and a broken-or-absent magnitude; all five are fixed and CI-green. The sixth has no ledger at all — and that absence is hiding a live defect.**

| binding | magnitude before | now | commit | CI |
|---|---|---|---|---|
| lazily-rs | typed `MIN_BLOCKS=30`, `>=`, digests only | 36 sites / 30 digests | `9e85037` | ✅ |
| lazily-dart | typed `MIN_BLOCKS=722`, `>=`, sites only | 725 / 616 | `450901e` | ✅ |
| lazily-kt | **a tautology** | 18 / 18 | `bc7132a` | ✅ |
| lazily-go | absent | 725 / 616 | `f146af6` | ✅ |
| lazily-cpp | absent | 15 / 15 | `f49a759` | ✅ |
| lazily-gd | **no ledger at all** | — (nothing landed, deliberately) | — | — |

Superproject pointer `524a3d3c3`. Every one derives from the corpus listing minus that binding's own ledger, never from a run artifact; every one asserts both dimensions as **equalities**; every one has a probe where the new dimension is the sole signal; three of them (kt, cpp, rs) also cross-check the digest **sets**, so the loader's digest rule and the guard's twin cannot drift apart while agreeing on cardinality.

**Two of the five were worse than "not derived".**

`lazily-kt`'s magnitude was a *tautology*: the success line printed `$blocks_total/$blocks_total` — the same number on both sides, which cannot disagree. `lazily-dart`'s `MIN_BLOCKS=722` had **already drifted, by 3**, and its agent proved it rather than asserting it: `git archive`-ing lazily-spec at `4010d99~1` and re-walking gives exactly 722, so the constant was right when written at 13:41 and lazily-spec grew 3 sites 53 minutes later. The `>=` spent the afternoon tolerating three blocks silently detaching. Its history is the same event three times over: 672 → 674 → 722.

**I was wrong about lazily-go.** I reported it had no rung-0 ledger; it has a complete one — `conformance_unbound_block_test.go`, with a read-time inventory behind the `specReadFile` seam, a content-keyed bind ledger with two tracker seams, and a both-directions excuse ledger. The line I quoted as evidence of absence was the comment *describing the problem that file was written to solve*. Only the magnitude was missing. Its agent also corrected my table: go's own walk is three names objects-only, so its number is 725/616, not the 743/634 row I handed it — which is exactly why the rule is to derive rather than copy.

**lazily-gd: no ledger, and the gap is not theoretical.** Proven at runtime rather than by grep — after a green `make check`, gd's manifest holds 22 lines, all bare fixture ids, zero block lines. Of gd's 136 block sites, 135 are bound and **one is not**: `scope_teardown_equals_fold_of_disposals.json`'s fixture-top-level `expected`, carrying `observationally_equal`, `final_state`, `after_publish`. That is the fixture's headline claim — the thing its filename asserts. Falsifying **every value in it** exits 0 green; deleting the block entirely exits 0 green; falsifying four step-level `expect.value` in the *same fixture* exits 2 red, which is what proves the override reaches the suite and isolates the unbound block rather than a broken harness. gd's runner reads only `step.get("expect")`, so a fixture-top-level block is structurally unreachable.

Its agent also demolished the staging hypothesis I gave it. gd's 22-fixture scope is stable and exactly ledgered — `corpus ∩ IMPLEMENTED_FAMILY_PREFIXES − KNOWN_UNCOVERED = 22 → 136 sites / 127 digests`, reproduced from the committed arrays alone — which is the paired-array shape lazily-spec says makes a floor auditable. A derived magnitude moves in the same commit that adds a family prefix, so **staging is an argument for the derived form, not a reason to wait.** Nothing was landed there because building the rung means teaching gd a cross-scenario relation, which is behavioral work that deserves its own cycle; filed with the six-step shape and the GDScript digest obstacle (Godot has no per-node raw JSON access, so cs's raw-lexical rule cannot be copied).

**Three narrow walks, now measured rather than suspected.** rs 36/771 sites (4.7%), kt 18/725 (2.5%), cpp 15/704 (2.1%). cpp's is narrow in a different *kind* — one name at depth 1, where zig read several — and should be far cheaper than zig's 239-site job, because cpp's `bound` set already holds 628 digests: most nested blocks are already bound and the widening is mostly bookkeeping. kt's is the reverse: one block per fixture is a *structural* ceiling, since its inventory is a `Map<String, JsonObject>` keyed by fixture path. Each filed as its own item with its own numbers, the way `#lzzigblockwalk` was.

One finding that clears the way for the next queue item: **`#lzktareapartition` does not block kt's magnitude.** kt's own script never sizes the corpus from `REQUIRED_AREAS` — it enumerates the corpus listing and enforces `opened ∪ KNOWN_UNCOVERED` bidirectionally, giving 156 − 8 = 148 exactly. The area-partition defect only affects lazily-spec's *external* `check-corpus-floors.mjs`. Still real, just not a dependency.

Six follow-ups filed: gd's ledger (topmost — it carries the live defect), the three walk widenings, and two dart items (`MIN_BOUND_BLOCKS=697` is now the last hand-repinned floor there, already 3 slack at 700; and 12 array-valued blocks in `signaling/anti_spoof_session.json` sit outside dart's objects-only rule, one of them the anti-spoof invariant the fixture exists for).

Exchange is at 407 lines / 16 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzrstypedfloors] — opus-5

> **Queue prompt:**
>
> do [#lzrstypedfloors]


Done — `lazily-rs f50dce6` (CI ✅), superproject `6b70ba4ad`. And the honest headline is that **this is a smaller win than the item claimed, because I went looking for the detection the equality adds and it isn't there.**

`MIN_FIXTURES=150` and `MIN_SCENARIOS=166` are now derived from the corpus listing minus rs's own `KNOWN_UNCOVERED` and asserted EQUAL, with the scenario side walked through the *same* `scenario_ids` the rung above it uses. The scenario derivation is also the genuinely better witness in principle: `total` was walked from the manifest-derived `opened`, so a detached recorder shrank `total`, `replayed` and the comparison together, and the typed floor was the only thing standing outside that.

**What I could not find.** I looked for a perturbation the new equality catches and the guard's existing loops do not, and there is none. Both rungs were *already exact*: every corpus fixture must be opened or excused, every excuse must name something the run did not open, every manifest id must resolve against the corpus root, and the mirror-image pair for scenarios. Deleting a fixture from a scratch corpus is caught by the id-resolution check, not the floor; adding or removing a scenario is caught by direction 1. The floors were belt-and-braces on rungs their own neighbours already pinned. I'd rather say that than dress up a maintenance change as a new guard.

**What it does fix, both measured against the guard it replaces.**

*The treadmill was real.* Newly excusing one scenario-bearing fixture — a routine, legitimate act — made the old guard **fail**: `only 149 distinct canonical fixtures were OPENED, expected >= 150`. Both constants then have to be re-pinned by hand, which is exactly the ritual the old comments prescribed ("read the gate's own output, set this to that `<n>`, then prove it exact by `<n>+1`"). Probed by excusing `codec/blob_backend_discriminator.json` and stripping it from both evidence channels: old guard errors, new guard tracks it — 149/156 fixtures and 152/152 scenarios, both derived, both OK. That is a guard that punishes correct maintenance, and it is why `MIN_SCENARIOS` had reached 24 below reality before someone re-pinned it.

*One real hole, closed.* A **duplicated** `KNOWN_UNCOVERED` entry passes both directions — each occurrence names a real corpus file the run did not open — so the old guard exited 0 and printed `7 listed as known-uncovered` as its evidence, a number that was simply wrong. The derived arithmetic needs distinctness, so a duplicate check now comes first and refuses it. Probed both ways: old exit 0, new exit 1 naming the repeated fixture.

*And no false positive in the subtle place.* An excuse naming an **uncovered** fixture must not subtract, since that fixture contributes no scenario to the derived total. Probed: 166/166, exit 0, the excuse counted in the report and not in the expectation.

`make check` exit 0, zero `result: FAILED`, with 36 sites / 30 digests still exact.

Two things from the audit that needed doing and are done: rs's `AGENTS.md` still told readers to validate the deleted `MIN_BLOCKS` floor "when raised above the real count" — corrected in `63f1008`, pointer `fe9905788`, and it now records the narrow-walk scope the old text left implicit.

**The queue clobber left a residue.** The mirror rebuilt itself at this preflight — the binary synced 7 items into the empty body and flipped `queue: go` back on, so no operator action was needed there. But it synced **7 of 9**: `#lzorderinganchors`, `#lzktareapartition` and `#lzscenariofloormissing` are open in `agent:backlog` and absent from `agent:queue`, because the clobbered generation recorded them as consumed. Left as-is they would never drain. I'll run `agent-doc queue sync` and verify after this closeout rather than mutating the queue mid-cycle; it is recorded on `#adqueueclobber21` either way.

Exchange is at 460 lines / 17 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzgdblockledger] — opus-5

> **Queue prompt:**
>
> do [#lzgdblockledger]


Built, and the block is **bound**, not excused — `lazily-gd fc8ca63` (CI ✅), superproject `a88c147a5`. All six steps landed including the behavioral one I flagged as the part that might not fit in a single pass. `KNOWN_UNBOUND_BLOCKS` is empty; 136 sites / 127 distinct digests, all bound.

**The defect is closed and it was exactly as bad as the audit said.** Falsifying every value in `scope_teardown_equals_fold_of_disposals.json`'s top-level `expected` — `observationally_equal` naming scenarios that do not exist, `dependents_of.topic` 999, `read.outside` −1, `observed_by` `["bogus"]` — left `make check` green at exit 0. So did deleting the block. The runner now asserts `final_state`, `after_publish`, and the `observationally_equal` **relation between the two scenarios**, compared from observed records rather than from the fixture's own numbers, which is the whole point: comparing each scenario against `expected` separately would have left the relation — the thing the filename asserts — unverified.

**The digest rule was measured, and the obvious answer was wrong.** I handed over "integer-when-whole, else `%.17e`" as the likely rule since all 240 numbers in gd's blocks are whole today. It was **rejected on evidence**: GDScript's `%` does not implement `%e` at all and returns the literal string `"%.17e"`, which would have folded every non-whole number onto a single digest — a twin that agrees with itself and pins nothing. Numbers are folded by their IEEE-754 binary64 bits instead, verified byte-for-byte against `struct.pack("<d", v)`, with two explicitly-masked 32-bit FNV-1a lanes since GDScript has no unsigned 64-bit. The one surviving divergence is **guarded rather than hidden**: Godot flushes subnormals to zero, so the twin refuses such a value with a named error instead of disagreeing silently.

**Four probes, and the third dimension is new.** Delete a recurring-digest block → 135 sites / 127 digests, site fires, digest silent. Collapse a unique-digest block → 136 / 126, digest fires, site silent. Strip the block records → zero-guard fires before either equality. And then the one no cardinality can reach: **swap two unique blocks' `note` strings** → 136 sites *and* 127 digests, both counts agreeing, and only the site→digest **map identity** check fires. That is the failure a pair of counts is structurally blind to, and gd is now the fourth binding carrying such a check (rs, kt and cpp added one each during the audit).

That completes the family: every binding that carries a rung-0 ledger now asserts both dimensions, derived and equal — py, js, zig, cs, rs, dart, kt, go, cpp, gd.

**One finding filed rather than absorbed.** gd hit an undocumented corpus contract: iterating `expected` in sorted-key order runs `after_publish` before `final_state`, and `final_state.read["outside"]` then reads 105 where the corpus says 101. lazily-rs and lazily-zig already carry that same discovery as a hand-written comment in their runners — so three bindings have independently paid for a contract lazily-spec never states, and a binding that happens to iterate in insertion order passes without ever learning it. Filed as `#lzexpectedkeyorder`, with the preference for restructuring the fixtures over declaring an order: a declared order is still something every runner must remember to honour, where an order-free fixture cannot be got wrong.

**Closeout note on the retained-write defect.** It recurred twice more this cycle pair, and the second shape is new: `agent-doc queue sync` outside a cycle produced **stranded** edits — "NO cycle is open and NO response capture is retained, so nothing owns this write" — and `agent-doc commit` then refused them on purpose, correctly identifying them as an unanswered typed-component edit and instructing me to open the next cycle instead, which absorbed all three rescued items as `independent` inserts. That path worked exactly as designed. Both shapes are on `#adqueueclobber21`, and the stale JetBrains plugin (0.2.379 against the 0.2.382 this binary ships) has now warned at every preflight in this session — it is the most likely reason the editor projection never converges, and updating it is operator action.

Exchange is at 489 lines / 18 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzexpectedkeyorder] — opus-5

> **Queue prompt:**
>
> do [#lzexpectedkeyorder]


Done — `lazily-spec 97ffd0b` (CI ✅), superproject `b7dae372d`. **No fixture byte changed**, and `corpus publication OK` confirms it: `conformance/` still matches `origin/main`, so no binding's derived magnitude moves.

**I surveyed before choosing, and the survey changed the answer.** The hazard is **exactly one block in the whole corpus** — one hit across all 156 fixtures for "a sibling key of an assertion block that carries an `op`". So this is not a corpus-wide pattern needing a structural fix; it is one block that can be named exactly.

And the shape is sharper than "key order matters". `after_publish` carries an `op` — `set_cell topic=5` — so it **mutates the world**, and `final_state` describes the world before it. `final_state.read.outside` is 101, `after_publish.read.outside` is 105: the same node in two different worlds. The hazard is that **`after_publish` sorts alphabetically first**, so sorting the block's keys reverses the only correct order, and the failure surfaces as "wanted 101, got 105" with nothing naming the cause. A runner that iterates in document order is right by accident and never learns the rule.

**Nothing is broken today.** I checked the actual tail-evaluation site in all ten bindings — not first textual mention, which is misleading because rs, py and cpp declare `after_publish_*` observation fields hundreds of lines earlier — and all ten read `final_state` first. What was missing was any way for the *next* one to find out.

**So I took the documentation half, and rejected the declaration half on cost.** An `order` key in the block would change its bytes → its content digest → every binding's derived assertion-block magnitude, and all ten now assert sites *and* distinct digests as equalities; plus the new key would be unconsumed by every existing runner and fail rung 2 everywhere. That is ten reddened repositories to state something a document and a static check state for free. Recorded in the commit so the trade-off isn't relitigated.

What landed: `#lzexpectedkeyorder` in `docs/conformance.md` under the existing ordering section, naming the block, both values and the rule; plus nine `OrderedCheck` entries pinning `final_state` before `after_publish` — **anchored on the two corpus key names, not on a value or a runner literal.** That is deliberate: `#lzorderinganchors` (still queued) is the debt created by anchoring on literals the bindings then removed, and these are names the corpus owns, so this adds nothing to that pile.

**Probed in both directions** against a scratch copy of lazily-zig's runner: swapping the two tail reads fails with `pair 1 is reversed` and the item id in the message — the diagnosable failure gd didn't get; renaming the `final_state` read away fails as `missing fixture/work anchor`. Real tree green for all nine (rs 3 checks, py 6, js 6, go 3, cpp 3, dart 3, kt 3, zig 3, cs 6), and lazily-spec `make check` exit 0.

**One gap found on the way, filed rather than patched.** lazily-gd does not run `check-assertion-ordering.py` at all and has no entry in its `CHECKS` — the guard covers **9 of 10** bindings. That is precisely the failure its own design note warns about ("a new standalone target would be a guard every existing workflow could silently omit"), arrived at by gd joining the family after the guard existed. Filed as `#lzgdorderingguard` with the candidate anchors; wiring it means editing gd's Makefile and CI, which is a second repo and its own verification, and gd's `check-ci-reach` guard should itself have caught an unreached target — worth checking in that pass.

Exchange is at 512 lines / 19 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzgdorderingguard] — opus-5

> **Queue prompt:**
>
> do [#lzgdorderingguard]


Done — `lazily-spec f00f8c2` and `lazily-gd 3642b7d`, both CI ✅, superproject `5c0a67eaf`. Both repos `make check` exit 0, and the corpus is untouched.

**Two corrections to what I filed last cycle, both of which changed the work.**

First, I claimed cpp was also missing the guard. It isn't — cpp invokes it through `$(LAZILY_SPEC_DIR)`, so my literal-path grep missed it, and cpp even has the fail-closed `test -f … || refuse` preamble that turned out to be the right pattern to copy. **gd was the sole gap**: 0 Makefile mentions, 0 workflow mentions, and no `CHECKS` entry.

Second, and it matters more: I asserted gd "carries a ci-reach guard, so an unreached target should itself be caught." **That was wrong.** gd has no `check-ci-reach.sh` and no `ci-reach` target, where all nine others have both. So nothing in gd would have caught the very omission this item existed to fix — the gap was found by a cross-binding survey, not by gd's own tooling. Filed as `#lzgdcireach` with the correction stated explicitly, because a wrong claim about what a guard covers is worse than a missing guard.

**The interesting part was not the `CHECKS` entry.** The shared entry point delegates to `check-assert-with-consumption.py`, which configures nine languages and not GDScript — so registering `gd` made the delegation fail on an unknown `--binding`. Three ways out, and two of them are bad: teaching the companion GDScript is real work for a check that is currently vacuous here (gd asserts through `_check`/`_observe` and has no callback-taking API at all), and skipping the delegation silently is the `#lzvacuousrun` shape this entire ladder refuses.

So an unconfigured binding is now an **explicit, checked exemption**. It carries its reason and its proof obligation, and the guard verifies the claim against gd's real sources rather than taking it: the day an `assertKeyWith` lands there, the exemption fails and names the file, the call, and what to do about it. The success line says the exemption applied, so nobody is told the companion ran when it didn't.

**Anchor choice mattered here too.** gd lists `final_state` and `after_publish` in a `TAIL_KEYS` allowlist *in the same relative order* as the evaluation sites — so a looser anchor would have passed while proving nothing. The check anchors on `tail.has(…)`, the evaluation sites, each unique in the file.

**Four probes, each failing with a named reason:** gd's two tail reads swapped → `pair 1 is reversed`; an `assertKeyWith(` added to gd → the exemption fails naming file and call; the exemption globs pointed at an empty tree → fails as "asserted over nothing" rather than passing; a missing sibling → the sibling guards' own refusal wording, not a python traceback. The other nine are unchanged and green (rs 3, py 6, js 6, go 3, cpp 3, dart 3, kt 3, zig 3, cs 6), and `--self-test` still passes.

No new CI workflow step was needed: gd's job already clones lazily-spec as a sibling and runs `make check` as a single step, so adding the target to `check` covers CI. Convenient — and precisely why an unreached target there is easy to miss, which is the `#lzgdcireach` argument.

Exchange is at 537 lines / 20 sections with the queue active — noting it, not stopping for it.

### Re: do[#lzgdcireach] — opus-5

> **Queue prompt:**
>
> do [#lzgdcireach]


Done — `lazily-gd 913edc8` (CI ✅), superproject `6a071d51a`, `make check` exit 0. And a verbatim port would have been wrong, which is the substance of this one.

**Measured before changing anything: the unmodified guard reports all six of gd's gates unreachable.** `test`, `load-graph`, `conformance-coverage`, `assertion-ordering-check`, `import`, `gdunit4` — every one MISSING. CI runs all of them; make runs them. That is a false red, not a finding. The assumption I flagged when filing this turned out to be the whole job.

The cause: gd's CI is a single `make check` step, and `make_invokes` only credited a CI command naming the target **exactly**. So `make check` reached `check` — which carries no commands of its own and is reported "no gate" — and nothing else. Fixed by making the relation transitive: a CI step running `make <ancestor>` reaches every target in that ancestor's prerequisite closure, walked with the same `prereqs_of` the main closure is built from, so there is one notion of "prerequisite" in the script rather than two that can drift. The anchor path is untouched.

**Proven a no-op for the other nine rather than argued.** Their copies are byte-identical (`da1444b38e93`, 570 lines), and running the *new* script in each of the nine produced output byte-identical to the old — exit 0 in all eighteen runs. The reason is measurable too: **none of the nine invokes `make check` from a `run:` step.** The only real `run:` body in any of them containing that phrase is this guard's own refusal text — precisely the comment-vs-command trap its own header warns about, which is why I checked the `run:` bodies rather than grepping.

**What this guard can and cannot do for gd, stated rather than left to be found.** With CI as one aggregate step every gate is reached transitively, so it cannot catch a missing per-gate CI step — there are none to miss. What it pins is the *invocation*. Three probes:

- narrow the single step to `make test` → `load-graph`, `conformance-coverage`, `assertion-ordering-check` go unreached (exit 1), while `test`/`import`/`gdunit4` stay reached because they are inside `test`'s closure;
- remove the make step entirely → every gate unreached, exit 1;
- add a throwaway gate to `check` that no workflow step names → reported **reached**, exit 0. That probe *cannot* fire here, and I ran it to confirm that rather than assume it. It is the honest verdict — `make check` does run the throwaway gate — and it is the cost of the aggregate-step shape.

**And the correction stands in the code.** The item I filed claimed gd already had a ci-reach guard "so an unreached target should itself be caught". It didn't, and nothing here would have caught it; that is written into the commit rather than only into this exchange.

Two follow-ups, both **appended** rather than prepended: `#lzcireachsync` (ten copies of the script are no longer byte-identical — the no-op proof is already done, so it is a port plus verification, with the open question of whether hand-synced copies should keep being hand-synced), and `#lzfollowupqueueorder`. That second one is about my own process: four cycles running, the item I filed became the next head, so the drain went depth-first into gd tooling while `#lzcppblockwalk` and the three older items sat still. Prepending is the right default; it was the wrong choice for adjacent-but-not-urgent work, and I should have used `append` three cycles ago. `#lzcppblockwalk` is next.

Exchange is at 564 lines / 21 sections with the queue active — noting it, not stopping for it.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzgdcireach]~~
- ~~do [#lzgdorderingguard]~~
- ~~do [#lzexpectedkeyorder]~~
- ~~do [#lzrstypedfloors]~~
- ~~do [#lzgdblockledger]~~
- do [#lzcppblockwalk]
- do [#lzktblockwalk]
- do [#lzrsblockwalk]
- do [#lzdartboundfloor]
- do [#lzdartarrayblocks]
- do [#lzorderinganchors]
- do [#lzktareapartition]
- do [#lzscenariofloormissing]
- do [#lzcireachsync]
- do [#lzfollowupqueueorder]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzcireachsync] [lazily-rs, lazily-py, lazily-js, lazily-kt, lazily-go, lazily-cpp, lazily-cs, lazily-dart, lazily-zig] The ten copies of scripts/check-ci-reach.sh are no longer byte-identical. Before #lzgdcireach the nine all carried md5 da1444b38e93 / 570 lines — an invariant nothing enforces but which is plainly deliberate, since the script has no shared home (unlike check-assertion-ordering.py, which lives in lazily-spec and is invoked across the sibling boundary). lazily-gd 913edc8 added `make_invokes_ancestor`: a CI step running `make <ancestor>` reaches every target in that ancestor`s prerequisite closure, walked with the same `prereqs_of` the main closure uses. Without it a verbatim copy reported all six of gd`s gates unreachable, because gd`s CI is a single `make check` step. THE NO-OP PROOF IS ALREADY DONE, so this item is a port plus verification rather than a design question: the new script was run in each of the nine and produced output BYTE-IDENTICAL to the old one, exit 0 in all eighteen runs — because none of the nine invokes `make check` from a `run:` step (each runs the underlying commands per gate; the only real `run:` body in any of them containing the phrase "make check" is this guard`s own refusal text, which is the comment-vs-command trap its header warns about). So: copy gd`s version into the nine, re-run each binding`s `ci-reach` target and confirm output is unchanged, then `make check` per repo and CI. Worth deciding in the same pass whether nine-plus-one hand-synced copies should keep being hand-synced at all — the alternatives are a shared home in lazily-spec (which would make every binding`s own guard depend on the sibling checkout, a cost the current design deliberately avoids) or a guard that asserts the copies are identical. Note for whoever takes it: the file is 641 lines in gd now, and the ONLY behavioural difference is the two new functions plus one call site and the header sentence, so a diff against gd is the cheapest review. Repos: the nine above
- [ ] [#lzfollowupqueueorder] [process] Follow-up items filed by a drain cycle keep preempting the queue they were filed from, because `--backlog-add` prepends by default and the queue mirror honours that. Three consecutive cycles filed an item that became the very next head — #lzblocksitepin, then #lzexpectedkeyorder, then #lzgdorderingguard, then #lzgdcireach — each pushing #lzcppblockwalk one place further back, so the drain went depth-first into gd tooling while the three walk-widening items (cpp/kt/rs) and the three older items (#lzorderinganchors, #lzktareapartition, #lzscenariofloormissing) sat untouched. Prepending is the right DEFAULT — a follow-up from the turn that just ran is usually the most task-relevant work, which is exactly why the flag exists — but it is the wrong choice for a follow-up that is merely ADJACENT rather than more urgent, and nothing currently prompts the agent to make that judgement. Concretely: prefer `--backlog-queue-placement append` for a follow-up that is not more urgent than the existing head, and consider whether the agent-doc skill should say so at the point where it tells the agent to capture follow-up work (runbooks/respond.md, the backlog-capture rule) rather than leaving the default to decide. Worth checking whether the same asymmetry applies to `--backlog-add-after`/`--backlog-add-before`, which the queue mirror also honours. This item deliberately uses append on itself. Repo: src/agent-doc (skill/runbook text), or file against tasks/agent-doc/ if it is treated as a product change
- [ ] [#lzcppblockwalk] [lazily-cpp] cpp`s rung-0 walk reads the TOP-LEVEL `assertions` object only, no descent, no other block name: 15 sites / 15 distinct digests of the 704/601 the same 143 opened fixtures carry — 2.1%. 128 of those 143 fixtures carry no top-level `assertions` object at all, so rung 0 reports nothing whatsoever about them; corpus-wide only 18 of 156 fixtures have one. This is narrower in KIND than lazily-zig`s was (zig read several names at depth 1; cpp reads one name at depth 1), and it should be much cheaper than zig`s 239-site job: cpp`s `bound` set already holds 628 digests, so most nested blocks ARE bound by a tracker already and the widening is mostly bookkeeping rather than new runner work. Widen tests/test_assertion_keys.hpp`s declare_assertion_block to the whole block-name set at every depth, keep the guard`s python twin in lock step (it already compares digests as SETS, so a divergent rendering fails rather than passing), then bind or excuse whatever it surfaces. Verify against `make corpus-blocks-report` in lazily-spec. Repo: src/lazily-cpp
- [ ] [#lzktblockwalk] [lazily-kt] kt`s rung-0 walk is the narrowest in the family: top-level `assertions`, object-valued, ONE candidate per fixture, no recursion, no expect/expected/expect_initial/expect_after — 18 sites / 18 distinct digests of the 725/616 the same 148 opened fixtures carry, 2.5%. The one-block-per-fixture ceiling is STRUCTURAL, not incidental: declaredBlocks in src/test/kotlin/io/github/lazily/ConformanceFixtures.kt is a Map<String, JsonObject> keyed by fixture path, so widening means changing the inventory`s data structure before changing its walk. Expect roughly 600 unbound digests to surface (lazily-zig`s comparable widening surfaced 204 over 239 sites and took a full cycle across nine runners). The magnitude guard at scripts/check-conformance-coverage.sh already states the 18-vs-725 gap and forbids narrowing the walk to make a red go away; the comment at line ~958 is where this id belongs once it exists. Verify against `make corpus-blocks-report` in lazily-spec. Repo: src/lazily-kt
- [ ] [#lzrsblockwalk] [lazily-rs] rs`s rung-0 walk is the same narrow shape lazily-zig retired in #lzzigblockwalk: the top-level `assertions` object plus the `assertions` object of each element of the top-level frames/scenarios/rejects arrays, and nothing else — 36 sites / 30 distinct digests of the 771/661 the same 150 opened fixtures carry under every block name at every depth, so 4.7% of sites and 4.5% of digests. The entire remainder is the four names this walk never looks at: `expected` 434 sites/359 digests, `expect` 295/266, `expect_initial` 3/3, `expect_after` 3/3 — and ZERO `assertions` blocks are missed at any depth, so the gap is purely the name set. Widen record_declared_blocks() in tests/common/mod.rs to the whole block-name set at every depth (objects only, emit-and-do-not-descend, name-preferring list labels), mirror it in the guard`s derivation so the one-walk-two-callers property holds, then bind or excuse every block it surfaces — expect runner work, not a guard edit. In lazily-zig the same widening surfaced 204 unbound digests over 239 sites across nine runner families and took a full cycle. Verify against `make corpus-blocks-report` in lazily-spec. Repo: src/lazily-rs
- [ ] [#lzdartboundfloor] [lazily-dart] scripts/check-unbound-blocks.py still carries two typed `>=` floors that #lzblockmagnitudeaudit deliberately left alone because they are different rungs: MIN_BOUND_BLOCKS=697 (blocks BOUND) and MIN_BLOCK_FIXTURES=144 (fixtures opened). MIN_BOUND_BLOCKS is now the last number in this repo a person re-pins by hand — it read 697 when pinned and the run reports 700 today, so it is already 3 slack and guarding nothing in that range, the exact drift the block magnitude just stopped. MIN_BLOCK_FIXTURES is now REDUNDANT: the site-set equality subsumes it, since the derived site set is keyed by fixture and a missing fixture takes its sites with it. Derive the bound count the same way (declared minus the excuse ledger, asserted EQUAL) or retire it in favour of the unbound-set check that already exists, and delete MIN_BLOCK_FIXTURES. Repo: src/lazily-dart
- [ ] [#lzdartarrayblocks] [lazily-dart] dart`s walk is object-valued only, so array-valued tracked keys contribute no site: the same 144 opened fixtures give 725 sites / 616 digests under dart`s rule and 737/628 when array ELEMENTS count. The gap is exactly 12 blocks and they are all in one fixture — `signaling/anti_spoof_session.json`, whose eight array-valued steps[].expect keys carry 1+2+3+1+1+1+1+2 elements. lazily-js instruments each element (which is why js derives 747 where dart derives 725); dart`s objects-only rule sees none of them, so whatever those 12 blocks assert — one of them is the anti-spoof invariant the fixture exists for — is outside rung 0 in this binding. Decide deliberately: either instrument array elements as js does, or record why an array-valued block has no bindable site here. Related and worth settling in the same pass: dart reads its block-name set out of the tracker, so a name the CORPUS grows is invisible on both the inventory and the derivation at once (a NAME-OPEN `^(assert|expect)` scan finds nothing beyond the five names today — `expected` 405, `expect` 294, `assertions` 32, `expect_initial` 3, `expect_after` 3 — but that is today`s corpus, not a guarantee). Repo: src/lazily-dart
- [ ] [#lzorderinganchors] [lazily-spec] scripts/check-assertion-ordering.py ORDERED_CHECKS anchors its ordering contract on the EXACT floor literals the bindings are now removing - regexes like 'assert accepted == 6', 'assert replayed == 12', 'assert.equal(accepted, 10)' for py/js/cs. lazily-py could not delete three of its floors this cycle and shipped them as DEAD ANCHORS, kept only to satisfy this guard, with the live constant-free assertion sitting above them. That is debt the guard created. Re-anchor on something stable that is not a value - a named helper call, or a marker the runner declares - so removing a constant does not break an unrelated ordering contract. Note the anchor must also be UNIQUE in the file (py tripped 'anchors are not unique' by quoting the literal in an explanatory comment). Repo: src/lazily-spec, then src/lazily-py to drop the dead anchors.
- [ ] [#lzktareapartition] [lazily-kt] check-conformance-coverage.sh declares REQUIRED_AREAS (27 areas) with no EXCUSED_AREAS complement, so the area list scopes the AUDIT and not the set the suite OPENS: kt opens 148 fixtures / replays 157 scenarios where deriving from the areas alone gives 147 / 151. lazily-spec's check-corpus-floors.mjs therefore declines to derive any floor for kt (it would otherwise have called an honest MIN_FIXTURES=148 unsatisfiable). lazily-cpp pairs the two arrays and its own guard enforces the exact complement, and there the derivation is exact. Add the EXCUSED_AREAS complement (or the equivalent partition assertion) so kt's floors become auditable from the corpus side. Repo: src/lazily-kt
- [ ] [#lzscenariofloormissing] [lazily-py, lazily-kt, lazily-gd] No MIN_SCENARIOS floor is declared at all - lazily-spec's check-corpus-floors.mjs reports this rather than failing, because lazily-kt deliberately retired MIN_FIXTURES for an area-partition assertion and that is a stronger invariant. But a binding with NEITHER is relying on nothing: py replays 151 scenarios and kt 157 with no positive-evidence floor under either, so a run that silently stopped replaying scenarios would report green. Derive each floor from the gate's own output, add it, and prove it exact by failing at n+1. lazily-gd is staged and may legitimately have nothing to floor yet - check before adding. Repos: src/lazily-py, src/lazily-kt, src/lazily-gd
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
