---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
agent: codex
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

active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260727-202047.md`*

Compacted content:
- Archived 16 response topic(s): review triage; queue materialization repair; controller recycle boundary; 13 more
- Prior summary/context: prior compacted content: Archived 14 response topic(s): Response; Response; Response; 11 more
Implementation is complete, green, committed, pushed, and CI passed. IDEA still runs plugin 0.2.299; plugin 0.2.300 is installed but requires activation.

Please restart IDEA once more, then send `continue` so I can commit the agent-doc closeout.

### Re: Response

Implementation is complete, green, committed, pushed, and CI passed. IDEA still runs plugin 0.2.299; plugin 0.2.300 is installed but requires activation.

### Re: do [#lzcstemporal] — gpt-5

> **Queue prompt:**
>
> do [#lzcstemporal]


Completed and shipped the full lazily-cs temporal, rate-shaping, and windowing phase.

- Added `Optional<T>` plus production logical-clock `ManualLogicalClock`, `TimerCell`, `IntervalCell`, `CronCell`, and value-preserving `DeadlineCell<T>`.
- Added `DebounceCell<T>`, leading/trailing `ThrottleCell<T>`, count/time `SampleCell<T>`, deterministic `SplitMix64Random`, and strict-threshold `ProbabilisticSampleCell<T>`.
- Added count/time tumbling, count sliding, and gap-based session windows over the production `MergePolicy<T>` algebra.
- Every emitted output is a real reactive `Source`; held, dropped, early, and empty-window operations do not touch it, so fixture invalidation assertions exercise the library rather than runner bookkeeping.
- Added `TemporalRateWindowConformanceTests.cs`, which discovers and opens all 4 temporal, 6 rate-shaping, and 4 windowing canonical fixtures, asserts exact returns/state/output/invalidation behavior, and carries positive fixture and step floors.
- Removed exactly those 14 runtime-opened fixtures from the known-uncovered ledger. Coverage advanced from 61/124 with 63 known-uncovered to 75/124 with 49 known-uncovered.
- Mutation proof: changing probabilistic admission from `draw < rate` to `draw <= rate` made the canonical rate-shaping runner fail at the exact `draw == 0.5` boundary; the restored implementation passed.

Verification and publication:

- Targeted phase suite: 5/5 passed.
- lazily-cs `make check`: 136/136 passed; coverage gate reported 75/124 opened and 49 known-uncovered.
- lazily-cs `make format-check`: passed.
- Workspace `make check`: passed, including 8,222/8,222 agent-doc tests and the remaining workspace suites.
- lazily-cs CI for `d5bba73`: completed successfully on the required one-shot inspection.
- Root repository has no workflow named `CI`.
- Pushed lazily-cs commit `d5bba73`; pushed root submodule pointer `6434c3b53`.
- Preserved pre-existing lazily-cs `AGENTS.md`, `README.md`, and untracked `build/`, plus unrelated workspace changes.

Confidence is high: the complete canonical corpora, true reactive invalidation probes, merge-policy-path counter, large logical-clock jump, mutation check, full binding gate, workspace gate, and CI are green. No escalation was required.

C# may now join the exact temporal, rate-shaping, and windowing peer groups in the single feature suite. Unsupported capabilities remain staged and skipped rather than advertised.

### Re: do [#lzcscluster] — gpt-5

Completed `#lzcscluster` and preserved capability-staged peer admission.

Implemented production C# surfaces for:

- SWIM membership with Phi-accrual failure detection and structurally compared reactive peer sets.
- Lease, leader, fenced lock, semaphore, barrier/quorum coordination.
- Typed ephemeral, presence, and awareness cells with TTL and membership eviction.
- Circuit breaker, retry backoff, bulkhead, and timeout resilience.
- Health, readiness, discovery, and durable service-registry projection/replay.

Canonical proof:

- Added `tests/Lazily.Tests/ClusterConformanceTests.cs`, which opens and replays all 17 membership/coordination/presence/resilience/service fixtures and checks every operation, return, projection, and invalidation edge (95 fixture steps across 5 tests).
- Deliberately mutated lease expiry from `<` to `<=`; `coordination/lease.json` failed at the exact `now == expiry` edge (`Expected: True`, `Actual: False`). Restored the correct boundary and reran green.
- `make check` in lazily-cs: 141/141 tests passed; runtime coverage is 92/124, with known-uncovered reduced from 49 to 32.
- `make format-check`: passed.
- Root `make check`: all 8,228 agent-doc tests passed; an unrelated dirty Corky submodule failed once in `corky-core --lib`, and the exact `cargo test -p corky-core --lib` reproduction passed immediately without changes.
- Lazily C# CI run 30304369632: completed successfully. The root repository has no workflow named `CI`.

Changed paths: `src/Lazily/{ClusterCollections,Membership,Coordination,Presence,Resilience,Service}.cs`, `tests/Lazily.Tests/ClusterConformanceTests.cs`, and `scripts/check-conformance-coverage.sh`. Pre-existing `AGENTS.md`, `README.md`, and `build/` changes were left untouched.

Committed and pushed lazily-cs `6afe762` (`feat: add cluster coordination primitives`) and root pointer `b5a7dc9c8` (`chore: advance lazily-cs cluster features`).

Confidence: high. Escalation: none. C# remains excluded from unsupported peer groups; only these now-proven cluster capabilities are eligible.

### Re: do [#lzcspackage] — gpt-5

> **Queue prompt:**
>
> do [#lzcspackage]


Completed the package-ready implementation of `#lzcspackage`; credentialed first publication is captured separately as `#lzcsnugetpublish`.

Shipped in `lazily-cs`:

- Multi-targeting for `netstandard2.1`, `net8.0`, and `net10.0`, including compatibility shims for the older target.
- A package gate that inspects the built `.nupkg` for all three framework assemblies and the README.
- A tag/manual GitHub Release workflow that restores, builds, tests, packs, uploads the package artifact, validates `NUGET_API_KEY`, and pushes to NuGet.
- The generic numeric floor implementation now avoids `INumber<T>`, preserving the API on `netstandard2.1`.

Proof:

- `make check`: green; 141/141 tests, 92/124 conformance capabilities exercised, 32 explicitly staged as known-uncovered.
- `make format-check`: green.
- Package archive verified with all three TFM DLLs/XML docs plus README.
- Mutation proof: removing `netstandard2.1` made `make package-check` fail on the missing archive entry; restoring it returned the gate to green.
- lazily-cs CI run `30305847353`: success.
- lazily-cs commit `813983e` (`feat: multi-target and automate NuGet releases`) pushed.
- workspace pointer commit `f7431f0a5` (`chore: advance lazily-cs packaging`) pushed.

Publication was intentionally not triggered: the repository has no `NUGET_API_KEY`, `v0.2.0` already exists as a GitHub tag/release, and the NuGet flat-container lookup returned 404 for package ID `Lazily`. The new `#lzcsnugetpublish` follow-up owns adding the secret, authorizing `0.2.0` or a newer version, running the workflow, and verifying the first public package.

The root workspace `make check` was also attempted. It is blocked only by two reproducible, unrelated tests in the already-dirty `src/agent-doc` submodule (`managed_capability_proof_status_tracks_pending_and_failed` and `managed_capability_proof_status_requires_post_restart_proof`); lazily-cs checks and CI are green.

Confidence: high for package construction and release automation. Escalation: an operator must provide the NuGet credential and version authorization for `#lzcsnugetpublish`.

### Re: do [#lzcsnugetpublish] — gpt-5

> **Queue prompt:**
>
> do [#lzcsnugetpublish]


Gated `#lzcsnugetpublish` on the missing external publication credential. No public registry mutation was attempted.

Evidence:

- `gh secret list -R lazily-hub/lazily-cs` returned no repository Actions secrets.
- The configured password-store entry names contain no `nuget`, `lazily`, or `dotnet` credential.
- The current environment exposes no NuGet credential variable.
- The NuGet flat-container endpoint for package ID `Lazily` still returns HTTP 404, so the first publication remains outstanding.
- The package-ready implementation remains pushed at lazily-cs commit `813983e`, with workspace pointer commit `f7431f0a5`; CI run `30305847353` is green.

Changed paths: none in this cycle. The prior package and workflow changes remain the release candidate.

Required external signal to ungate: add repository secret `NUGET_API_KEY` to `lazily-hub/lazily-cs` and authorize the immutable public version (`0.2.0`, currently package-ready and unregistered, or a newer version). Once supplied, rerun this item to dispatch `.github/workflows/release.yml` and verify the NuGet listing.

Confidence: high that credential absence is the only immediate publication blocker. Escalation: repository/NuGet owner action is required because the secret cannot be derived safely from repository state.

### Re: do [#asyncplanenever] — gpt-5

> **Queue prompt:**
>
> do [#asyncplanenever]


Implemented and shipped the async-v2 kernel naming migration for C++, Go, Python, and Zig, with the shared contract enforced from lazily-spec.

- Canonical public value kinds are now `AsyncSource<T>` and `AsyncComputed<T>` in all four bindings. Published legacy `AsyncCellHandle` / `AsyncSlotHandle` names remain deprecated compatibility aliases or forwarding surfaces.
- Canonical constructors, maps, queues, disposal paths, and conformance models now use source/computed vocabulary. Deprecated memo constructors forward to guarded computed behavior rather than creating another node kind.
- Public state projection is documented and exposed as `AsyncComputedState`; formal-model vocabulary remains `AsyncSlotState` and is not mechanically renamed.
- Zig now uses generation-checked canonical handles and rejects stale/disposed handles; legacy raw-id methods remain compatibility coverage.
- `scripts/check_async_v2_names.py` verifies the canonical pair across the eight available bindings while staging absent sibling checkouts. It explicitly requires deprecated/compatibility markers on legacy declarations and does not affect network-suite capability admission.
- Mutation proof: temporarily removing C++'s canonical `AsyncSource` declaration made the checker fail at lazily-cpp; restoring it returned the complete suite to green.
- Synced the generated coverage tables required by the aggregate spec gate. This produced documentation-only commits in Kotlin and Dart.

Verification:

- `make check` passed in lazily-spec, lazily-cpp, lazily-go, lazily-py, lazily-zig, lazily-kt, and lazily-dart.
- The spec gate includes 151 pytest cases, the Lean build, coverage synchronization, and the eight-binding async-v2 naming check.
- Latest CI after push: spec `30308404104`, C++ `30308403755`, Go `30308403752`, Zig `30308404543`, Kotlin `30308403858`, and Dart `30308403712` were in progress. lazily-py has no workflow named `CI`.

Pushed commits:

- lazily-spec `07edb9a`
- lazily-cpp `994fdf8`
- lazily-go `7337a6c`
- lazily-py `d63c4a2`
- lazily-zig `c767164`
- lazily-kt `8ea153a`
- lazily-dart `82f58cf`
- root pointer advance `acedeef05`

Unrelated Python worktree state and Zig's pre-existing `AGENTS.md` edit were preserved and excluded from commits.

### Re: do [#lazilyconformancecoverag] — gpt-5

> **Queue prompt:**
>
> do [#lazilyconformancecoverag]


Kept the C# conformance-coverage ledger open, as required by the operator staging policy. This is an umbrella admission ledger, not a parity-complete implementation task.

Fresh verification at lazily-cs `813983e` confirms:

- `make check` passes with 141/141 tests and zero skips.
- The runtime manifest proves 92/124 canonical fixtures were actually opened.
- The remaining 32 fixtures are explicitly listed as known-uncovered; the guard rejects both newly unaccounted fixtures and stale allowlist entries.
- Multi-target package validation passes for `netstandard2.1`, `net8.0`, and `net10.0`.

C# therefore remains eligible only for the exact peer/feature groups whose production surface and canonical replay have shipped. Full peer admission stays blocked by the actionable implementation items `#lzcsqueues`, `#lzcscrdt`, `#lzcsipc`, `#lzcssync`, and `#lzcsplane`; the ledger text now names those dependencies explicitly.

No source paths changed in this cycle. Existing unrelated lazily-cs `AGENTS.md`, `README.md`, and build-tree state were preserved. Confidence is high because the measurement came from the runtime-opened manifest rather than static source references. No escalation is needed.

### Re: /goal Add missing lazily-cs features — gpt-5

> **Queue prompt:**
>
> /goal Add missing lazily-cs features


Goal created: **Add missing lazily-cs features**.

The active implementation scope remains the five staged parity items already tracked in this document: queue metering, CRDT families, shared-memory/blob IPC, reliable/distributed sync, and the remaining plane surfaces. C# will enter each peer/feature group only after that production surface and its canonical fixture replay ship.

No repository paths changed for this command. Goal creation was confirmed by the goal controller, confidence is high, and no escalation is needed.

### Re: release + publish all lazily packages — gpt-5

> **Queue prompt:**
>
> release + publish all lazily packages


Released the eligible lazily suite from the capability-gated conformance baseline:

- `lazily-spec` **v0.37.0** and `lazily-formal` **v0.37.0**: tags and GitHub releases are live.
- `lazily-rs` **0.52.0**: full all-features/formal/benchmark gate passed; the versioned benchmark report was regenerated; `cargo publish --dry-run --locked` passed; **0.52.0 is live on crates.io**; tag and GitHub release are live (`89db3a0`).
- `lazily-cpp` **v0.26.0**, `lazily-go` **v0.24.0**, and `lazily-zig` **v0.32.0**: release commits, tags, and GitHub releases are live (`e13334b`, `367b7e0`, `84af776`).
- `lazily-py` **0.38.0**: package build and full gate passed; tagged wheel workflow completed successfully; **0.38.0 is live on PyPI** (`7266c55`).
- `lazily-kt` **0.39.0**: full gate and Maven-local publication passed; tagged release workflow completed successfully and published the package (`37f89e5`).
- `lazily-dart` **0.28.0**: full gate and zero-warning publish dry-run passed; **0.28.0 is live on pub.dev**; tag and GitHub release are live (`1fdc9da`).
- `lazily-js`: v0.30.0 exposed an npm 12 packaging defect before registry publication. The interop peer executable mode and normalized package metadata were fixed in **v0.30.1**; the full gate and clean npm publish dry-run now pass with `bin/interop-peer.mjs` retained as executable; v0.30.1 is tagged and released on GitHub (`50c8914`). npm itself remains at 0.29.1 because CI's OIDC publish still gets the known E404 and local npm auth is expired. Existing operator action `#lzjsnpmtrustedpub` remains exact: configure `@lazily-hub/lazily-js` Trusted Publisher for repository `lazily-hub/lazily-js`, workflow `release.yml`, no environment, allowed action `npm publish`, then rerun the v0.30.1 workflow; alternatively reauthenticate as the package owner and publish v0.30.1 with the required OTP.
- `lazily-cs` was deliberately **not** tagged or published. It remains feature-staged at 141/141 passing tests and 92/124 runtime-opened canonical fixtures, with incomplete surfaces tracked by `#lzcsqueues`, `#lzcscrdt`, `#lzcsipc`, `#lzcssync`, and `#lzcsplane`; NuGet publication also lacks the repository credential. C# stays out of any peer group unless that exact production capability and canonical replay are present.

Every eligible language candidate passed its repository `make check` plus its applicable package dry-run. The superproject aggregate check encountered five agent-doc controller-socket timeouts under the 8,240-test parallel load; the representative controller test and the complete related preflight group both passed serially, isolating this as live-controller load contention rather than a lazily regression. The nine released submodule pointers were committed and pushed in root commit `9d6ee7946`; unrelated JavaScript README/build output, Zig AGENTS changes, Python worktree state, and session-document edits were excluded from that pointer commit.

The release policy now matches the requested network-suite model: peers join tests per exact supported capability, and incomplete bindings—especially C#—remain staged/skipped instead of being treated as suite-wide implementations.

### Re: complete [#lzcsqueues] — gpt-5

Completed the remaining C# queue/relay parity work and admitted C# to the full RelayCell feature group.

Production surface:

- Added value-aware `RelayMeter<T>` support for encoded `Bytes`, distinct pending `Keys`, and reactive logical-clock `Age`, while preserving `Count` and maintaining all configured measurements across live dimension retuning.
- Added tracked `Bytes`, `PendingKeys`, `Age`, and active-policy `Measure` reads; exact high-water gating; drain/drop/spill reset semantics; and fail-closed missing-meter or cross-context clock validation.
- Added the shared-core `Outbox<T>` and credit-metered `Inbox<T>` roles, token-bucket `RatePolicy`, operation `WindowPolicy`, TTL `ExpiryPolicy`, stable FIFO-tie `PriorityStorage<T>`, and sound commutative `KeyedRelay<TKey,T>` sharding.
- Kept durable spill replay constraints and transport-independent convergence on the existing `RelayCell<T>` core rather than duplicating relay behavior in the role facades.

Normative proof:

- Added 13 new relay metering/role/policy integration tests; the focused Relay suite passes 28/28.
- Mutation proof changed exact bound admission from `measure >= high_water` to `measure > high_water`; Bytes, Keys, and Age tests all failed at equality, then passed after restoration.
- lazily-cs `make check`: 154/154 passed with zero skips; `netstandard2.1`, `net8.0`, and `net10.0` builds and package checks pass; the runtime conformance ledger remains exact at 92/124 opened plus 32 known-uncovered.
- lazily-cs `make format-check`: passed.
- Canonical coverage now marks C# shipped for the complete RelayCell row. The spec gate passed 151 tests, the Lean build, coverage synchronization, fixture synchronization, and async-v2 naming checks.
- Every generated sibling coverage-table update passed its repository `make check`, including the full Rust gate.
- Workspace `make check` passed on rerun with 8,241/8,241 agent-doc tests. Its first pass ended only at an unrelated `corky-core --lib` invocation; that exact target passed immediately and the complete aggregate rerun was green.

Commits are complete and exact-path staged history is clean: lazily-cs `e3bed9d` + `e89fce4`, lazily-spec `ae03868`, generated one-line coverage commits in the other bindings, and root pointer commit `a9a275f4f`. The required pushes follow this binary-owned document closeout. Pre-existing C# `AGENTS.md`, queue-flavor README hunks, JavaScript size-report hunks, build trees, and unrelated workspace changes remain uncommitted and unstaged.

Confidence is high. No escalation was required. The umbrella C# staging ledger now depends only on `#lzcscrdt`, `#lzcsipc`, `#lzcssync`, and `#lzcsplane`.

### Re: complete [#lzcscrdt] — gpt-5

Completed and shipped the remaining lazily-cs CRDT parity phase.

Production support now includes:

- `TextCrdt` with Unicode-scalar elements, Fugue/RGA ordering, sticky tombstones, deterministic merge, version vectors, delta extraction/application, identity-preserving replay, and conservative stable-leaf GC.
- The generic `ICrdtTree` contract plus canonical CrdtTree algebra coverage.
- Move-aware `SeqCrdt<TKey,TValue>` with fractional positions, independently merged value/position/deletion registers, deterministic HLC behavior, reordering, merge, and GC.
- `LosslessTreeCrdt` with exact byte rendering, element/leaf structure, edit/split/merge/reorder/tombstone operations, UTF-8 boundary checks, dotted version frontiers with sparse holes, dependency buffering, anti-entropy deltas, fork/copy, and deterministic concurrent convergence.

Canonical proof is strict and runtime-backed: the C# suite now replays 10 TextCrdt scenarios (46 steps), 6 SeqCrdt scenarios (33 steps), 3 CrdtTree algebra scenarios, all 14 LosslessTree scenarios (41 steps), plus dedicated GC, Unicode-boundary, and dotted-hole-repair regressions. A mutation replacing dotted membership with per-peer maximum semantics made both the canonical repair fixture and the dedicated hole regression fail, then both passed after restoring sparse-dot membership.

Validation is green: focused CRDT tests 6/6; lazily-cs `make check` 160/160 with zero skips across netstandard2.1, net8, and net10 plus package checks; format verification; lazily-spec `make check`; every regenerated sibling coverage-table gate; and root `make check` (8,241/8,241 agent-doc tests plus the remaining workspace gates). Runtime conformance advanced from 92/124 opened with 32 known-uncovered to 105/124 opened with 19 known-uncovered. All eight C# CRDT coverage rows are now admitted.

Shipped commits are lazily-cs `f25c632`, lazily-spec `c24151e`, generated sibling coverage commits `9f3026f`, `2df2a19`, `c0e50b6`, `bc10922`, `85e0b9a`, `ed99582`, `d3acd6b`, `cc9db9e`, and `a344ddc`, with root pointer commit `b66c1bff2`; all were pushed. The required one-shot CI inspection found the spec and JavaScript runs successful, C#/Rust/Kotlin/Dart/Go/C++ in progress, Zig queued, and no CI workflow in Python/formal. It was not polled further.

Pre-existing worktree changes, including the staged queue-family README work and build directories, were preserved and excluded from these commits. C# may now join all eight exact CRDT feature groups; incomplete IPC, sync, and plane features remain staged.
<!-- agent:boundary:69ac7bfa -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzcsipc] lazily-cs: PARTIAL/STAGED — schema-defined Snapshot/Delta/CrdtSync wire types, exact externally tagged codec, all 10 DeltaOp variants, WireStamp/frontier/CRDT frames, blob descriptors, and Draft 2020-12 validation shipped at 13e8d67. Runtime conformance coverage is now 56/124 with 68 known-uncovered. Admit C# only to Snapshot/Delta/CrdtSync schema and wire peer groups. Still implement ShmBlobArena and executable BlobBackend zero-copy resolution/transport for shm, Arrow, and in-process backends before rows 26-27 admission. Plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27). (stays open: shared-memory arena and zero-copy backend behavior/tests remain.)
- [ ] [#lzcssync] lazily-cs: PARTIAL/STAGED — storage-independent IOutboxStore/DurableOutbox protocol, InMemoryOutboxStore, fsync append-only FileOutboxStore, monotonic stale-handle cursor recovery, append-before-send crash replay, and canonical outbox_store_protocol/outbox_replay_after_crash replay shipped at b103f80. Runtime coverage is now 58/124 with 66 known-uncovered. Admit C# only to the durable-outbox peer group. Still implement CrdtPlaneRuntime + anti-entropy, resync coordinator, IpcSink/IpcSource, and full-duplex SyncDriver; replay remaining reliable-sync, distributed, and message-passing corpora. Plan rows 28-32. (stays open: CRDT plane runtime, anti-entropy/resync, transports, sync driver, remaining corpora.)
- [ ] [#lzcsplane] lazily-cs: PARTIAL/STAGED — CausalReceipt/CausalReceipts schema/projection shipped at 967c44e; all 17 typed signaling frames, strict exact codec, authoritative SignalingRoom, anti-spoof server-stamped sender, sorted self-excluding rosters, unknown-target handling, and leave semantics shipped at d663074 with canonical signaling client/server fixture replay. Runtime coverage is now 61/124 with 63 known-uncovered. Admit C# only to the exact causal-receipts and signaling-wire/open-room peer groups. Still implement general state projection/mirror, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay service, agent-doc, and remaining message-passing corpora. Plan rows 33-39. (stays open: state mirror, command plane, FFI, permissions, handshake, instrumentation/benchmarks, and remaining corpora.)
- [ ] [#lazilyconformancecoverag] lazily-cs: STAGED per operator — incomplete C# surfaces stay out of peer/feature test groups until implemented. Reverified 2026-07-27 at lazily-cs f25c632: 105/124 canonical fixtures are runtime-opened and 19 are explicitly known-uncovered; 160/160 tests, all three target frameworks, package checks, and the conformance guard pass. All eight CRDT rows are now admitted (registers, TextCrdt + delta, CrdtTree, SeqCrdt, LosslessTreeCrdt core, dotted-frontier anti-entropy, and concurrent convergence), alongside the previously shipped exact slices. (stays open: close only after #lzcsipc, #lzcssync, and #lzcsplane ship their remaining production surfaces and canonical replays; until then admit C# only to exact feature groups whose implementation and fixture replay are present.)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzcsnugetpublish] lazily-cs: add repository secret NUGET_API_KEY, choose and authorize the NuGet version (0.2.0 is package-ready and currently unregistered, or create a newer tag), run the Release workflow, and verify the first Lazily publication on NuGet.
- [/] [#lzinteropcs] Keep lazily-cs visible but registry-staged in the single network suite. Current state: #lzcsipc is partial (schema-validated wire codec shipped; ShmBlobArena/executable zero-copy backends remain) and #lzcssync is partial (durable outbox shipped; CrdtPlaneRuntime, anti-entropy/resync, IpcSink/IpcSource, full-duplex SyncDriver, and remaining corpora remain). After both prerequisites ship their complete production surfaces, implement the C# interop peer, advertise only the exact features/codecs/channels/variants it supports, and let the capability-driven matrix admit it automatically. (gated: prerequisites #lzcsipc and #lzcssync remain open.)
- [/] [#lazilyrenameinverted] [operator-decision] Keep lazily-py `Cell`/`CellSlot` as mypyc-native implementation class names with canonical `Source`/`SourceSlot` aliases. NEXT: accept this documented exception, or explicitly authorize a breaking native-class rename with mypyc and identity migration tests.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
