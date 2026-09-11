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
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzpystructcell]: should we use pyproject extras or another project? This adapter should be generic to handle other competing libraries such as pydantic and dataclasses.~~
- ~~do [#lzpypromegress]~~
- ~~do [#lzpykeyedfold]~~
- ~~do [#lzpyworkflowdeterminism]~~
- do [#lzpyreplayproof]
- do [#lzpyprojectedchart]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzpyreplayproof] [recommended] [lazily-py] replay-equivalence harness: rebuild the graph from an ordered event log and assert cell values match a recorded fingerprint, failing loudly on divergence -- the durable-execution analogue of tsift's BLAKE3 body-hash revalidation for crux excerpts. `reliable_sync` and `latest_durable_projection` have the machinery but no stated contract. Repo: src/lazily-py. Source: src/haiven-dev/tasks/fpe.md
- [ ] [#lzpyprojectedchart] [recommended] [lazily-py] statechart mode that projects an external authority: feed it observed states, have it validate each transition was legal, and expose a derived 'wedged' cell (in a state past its deadline with no advance). FPE's workflow_run.status is owned by Postgres/Temporal, not the graph. Would express the #fpelasterrorlww wedge declaratively. Repo: src/lazily-py. Source: src/haiven-dev/tasks/fpe.md
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
