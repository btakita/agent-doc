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
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzpystructcell]: should we use pyproject extras or another project? This adapter should be generic to handle other competing libraries such as pydantic and dataclasses.~~
- do [#lzpypromegress]
- do [#lzpykeyedfold]
- do [#lzpyworkflowdeterminism]
- do [#lzpyreplayproof]
- do [#lzpyprojectedchart]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzpypromegress] [recommended] [lazily-py] counter/gauge cell family plus a `prometheus_client` egress adapter that registers off the reactive graph. Coverage matrix marks Python Egress as partial; this would delete the 43 hand-written mirror lines in src/haiven-dev/src/fpe/core/telemetry.py `update_runtime_metrics`. Repo: src/lazily-py. Source: src/haiven-dev/tasks/fpe.md
- [ ] [#lzpykeyedfold] [recommended] [lazily-py] named `KeyedFold` cell: N independent writers each own a key and clear only their own, with the summary as a guarded `Computed` over the live set. `service.HealthCell` does this for booleans only; `merge.MergeCell` has the algebra but no documented keyed-fold surface. Generalizes the FPE `last_errors` fix (#fpelasterrorlww, fpe PR #92). Repo: src/lazily-py. Source: src/haiven-dev/tasks/fpe.md
- [ ] [#lzpyworkflowdeterminism] [recommended] [lazily-py] temporal.io workflow-safe context: bind `TimelineSource` to `workflow.now()`, route scheduled effects through Temporal timers/activity starts, and raise on any non-deterministic read instead of corrupting replay. Also rename or loudly document `lazily.temporal` (time operators) to avoid the temporal.io collision. Blocks any workflow-side adoption. Repo: src/lazily-py. Source: src/haiven-dev/tasks/fpe.md
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
