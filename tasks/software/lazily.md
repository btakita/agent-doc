---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
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

gated: npm security-key user presence required
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260802-201633.md`*

Compacted content:
- Archived 10 response topic(s): do [#lzspecdecoderbound]; do [#lzkeynullstrict]; do [#lzreadmelinkrot]; 7 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): do [#coveragejsonscores]; do [#lzcppmsgpackwire]; do [#lzcppco...

### Re: do [#failcloseddispatch] — opus-5

Done in all six remaining bindings. One agent per repo, six repos, no overlap.

**76 fail-open dispatch chains found and closed**, every one mutation-checked by feeding a discriminator value no arm names, every one confirmed red before the mutation was reverted.

| repo | fixed | commit | suite | CI |
|---|---|---|---|---|
| go | 8 | `0cd58a9` | 129/138 fixtures, 118/118 scenarios | green |
| py | 25 | `6d924be` | 686 passed, 134/138 fixtures | green |
| kt | 24 | `6819ac6` | 423 tests, 118/118 scenarios | green |
| dart | 16 (30 sites) | `bca5750` | 657 tests, 118/118 scenarios | green |
| zig | 16 | `97afb05` | 916 tests × 3 pinned toolchains | green |
| cpp | 12 + 20 asserts | `4cdb143` | 55 tests, 99/103 scenarios | green |

Superproject pointer bump: `d952e9139`. `lazily-spec` was never edited — every mutation ran against a scratch corpus or a temporary runner-source edit, and the shared corpus is byte-identical and clean.

**The defect was bigger than the shape we named.** We went looking for a missing `default:` and found three classes:

- *Expectation-side dispatch.* The nastiest. `test_coordination_conformance.cpp:120` dispatched on the expected `role`; an unrecognised spelling didn't skip the assertion, it silently **changed which assertion ran** — the fixture said one thing and the runner checked another and passed. Same shape in cpp membership/resilience/service, and in dart's `durability` chains, which compared the observed durability against a value the fixture never named.
- *The discriminator nobody read.* Ten drivers across py, kt, zig and dart replayed every op as a single variant while a `type` field sat unread in the fixture. `windowing` tumbling/sliding counts, `rateshape` sample_count and probabilistic_sample, in four bindings each. Those runners were not dispatching at all.
- *The assumed last variant.* The `assert(policy == "Max")` shape, in every spelling: Kotlin `else -> tick(now)`, Dart `direction != 'client'` running the server codec, Zig bare `else`, Python `else: assert res.is_ignore` — where `resync_gap_converge.json` carries no `Ignore` frame at all, so that arm had never once executed.

**Two corrections to the record.**

`c264241`'s commit message says NDEBUG compiles the cpp asserts out. It doesn't: `tests/CMakeLists.txt` adds `-UNDEBUG`, and the generated flags are `-O3 -std=c++17 -UNDEBUG`. Those asserts fire today, including in the Release default. They were one build-flag edit from vanishing and they named nothing when they fired, so converting them was right — but they were not live false greens, and the cpp finds that *were* live are the silent-default class instead.

Zig's `signaling_conformance.zig:349` `else => unreachable`, the lead we carried, is **not** this defect class. It switches on a tagged union already narrowed by the enclosing prong, not on fixture data. It's now an explicit error return anyway — `unreachable` is unchecked UB under ReleaseFast and a runner should not spell a refusal in a form that only traps in Debug — but no fixture value can reach it, so that one change is unverified by execution. Stated plainly rather than counted as a mutation-checked fix.

**One real corpus bug fell out.** `nodekey_null_leniency`'s non-snapshot field is spelled `node_add`, not `delta`. The dart fail-open had been hiding which value it was actually handling.

**One method note worth keeping.** The py sweep's first pass reported five false "still green" mutation results — stale `__pycache__` let pytest reuse pre-mutation bytecode when mutate and restore landed inside the same filesystem second. Only the re-run with `PYTHONDONTWRITEBYTECODE=1` and the cache cleared per case is reported above. A mutation check that can't invalidate its own build is just another false green, one level up.

Queued the js/cs/rs sweep at the top of the backlog. Those three were marked "already have theirs" on the strength of the chains we happened to inspect, not an audit — and the six repos we did audit averaged twelve fail-opens apiece.

### Re: do [#nevergotsystematic] — opus-5

Done. The premise held: all three had been marked "already have theirs" on the strength of the chains we happened to inspect, and a systematic audit found **98 more fail-opens**.

| repo | fixed | commit | suite | CI |
|---|---|---|---|---|
| js | 40 | `c271814` | 581 tests, 118/118 scenarios | green |
| cs | 34 | `33ab72f` | 249 tests, Debug + Release | green |
| rs | 24 | `956a36a` | 6018 passed, + release profile | green |

Superproject pointer bump: `f9d39c8b8`. `lazily-spec` clean throughout — every mutation was runner-side.

**174 fail-opens across nine bindings.** The sweep is complete.

Each agent built a real AST analyzer rather than grepping — TypeScript compiler API for js, Roslyn for cs, `syn` for rs — and each ran a *separate* corpus-vocabulary cross-check for class 3, collecting every discriminator value the fixtures actually carry and reporting the ones no runner names as a literal. That second pass is what found the class the AST cannot see, and it found the same four drivers in all three repos that it found in the six before: `windowing` tumbling/sliding count, `rateshape` sample_count and probabilistic_sample. Four bindings, then three more. Seven of nine shipped the identical bug because the runners were written from the same shape.

**The "already fail-closed" record was half right, which is why it survived.** lazily-rs is the clean case: 71 `match` chains already panic on the unmatched value. Every single fail-open there was a *different* syntax — if/else tails, `==` binaries against a literal where a third value exists, and unread discriminators. Whoever checked this repo checked `match` and was right about `match`. The audit was scoped to the shape we already knew about, and the shapes we didn't know about were the ones that were broken.

**A negative test that passed for the wrong reason.** `signaling_negative_conformance.rs:54` dispatched on `direction` and returned `Err` on an unrecognised value — into a loop asserting `is_err()`. A misspelled direction passed the rejection test because the *runner* didn't recognise it, never because the codec rejected the frame. `test/signaling.test.js:37` had the same shape in js. A fail-open inside a negative test doesn't skip an assertion; it satisfies one.

**Two mutation checks reported "survived" and both were the harness, not the code.**

The rs pass reported all five stdlib sites as surviving. `cargo test` was filtered on `timer`/`timeout`/`revision_barrier`, but that file has exactly two tests with neither name — the filter matched zero tests and cargo exited 0. The cs pass first mutated a delta op the corpus doesn't carry, so the mutation never reached the arm it was aiming at; re-aimed at a value the corpus does carry, it went red. Neither was the stale-cache route we hit in py last cycle, and that's the point worth keeping: *a mutation check has at least three ways to report a false green* — stale artifact, a filter that selects nothing, and a probe aimed at a value the corpus never produces. All three exit 0 and look identical from outside. The cs agent's guard is the generalizable one: it asserted the test assembly's mtime advanced, which also caught two "RED" results that were actually compile failures from warnings-as-errors.

**The corpus corrected us again.** cs's new `field` guard went red immediately on the real fixtures: `nodekey_null_leniency.json` spells the delta case `node_add`, not `delta`. That's the second binding to learn this from a guard — dart hit it last cycle. The bare ternary could not have surfaced it in either.

Queued the library-side audit. This whole sweep looked at runners only, and cs came back with five concrete unassessed sites in `src/Lazily`. rs flagged two of its own and argued both are deliberate wire leniency — which is exactly why it needs a per-site verdict rather than a blanket conversion, and why an intentional default deserves a fixture pinning it. From the outside, a documented leniency and a forgotten `_ => {}` look the same.
<!-- agent:boundary:ebcdbd84 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- do [#failclosedsweep]
- 📌 do [#lzassertionblockschema]
- 📌 do [#lzdependencyavailability]
- 📌 do [#lzcpptranscribedrunners]
- 📌 do [#lzreactdtsgate]
- 📌 do [#lzegress]
- 📌 do [#lzqueuefamilyflavors]
- 📌 do [#lzwqfixtureconfig]
- 📌 do [#lzingressadapters]
- 📌 do [#kny1]
- 📌 Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#failclosedsweep] The fail-closed sweep audited conformance RUNNERS in all nine bindings and never looked at the LIBRARIES. lazily-cs surfaced concrete unassessed sites while closing #nevergotsystematic: IngressCore.cs:450/1053/1140, DurableOutbox.cs:301, AsyncSourceMap.cs:191, plus several `_ =>`-defaulting switch expressions. lazily-rs flagged two of its own and argued both are deliberate: ipc.rs:71 BlobBackendKind::from_str maps an unknown backend to Shm as documented wire forward-compat, and statechart.rs:215-225 defaults an unknown state kind to Atomic as documented parse leniency. That is the whole question - in a runner a silent default is always a bug, but in a library it is sometimes the wire contract, so each site needs a per-site verdict rather than a blanket conversion. Audit the library dispatch surface in all nine bindings the same way (AST pass, not grep), and for each silent default either document it as intentional leniency with the wire reason, or fail it closed. Where it IS intentional, add a conformance fixture that pins the leniency, because an undocumented default and a deliberate one are indistinguishable from the outside.
- [ ] [#lazilystatetable] Evaluate and pilot typed Lazily state tables as pure Computeds over enum product states; preserve exhaustive transition coverage without a second runtime, sidecar, or imperative ACK path. Plan: tasks/software/plan-lazily-state-tables.md
- [ ] [#reliablesyncrow] The Reliable sync row in coverage.json reads shipped in ALL NINE bindings while reliable-sync/coalesce_bounds_outbox.json and reliable-sync/liveness_lease_eviction.json are in ALL NINE KNOWN_UNCOVERED lists. The row claims resync coordinator plus at-least-once durable outbox plus OR-set/LWW liveness, and the two canonical fixtures for the liveness clause are replayed nowhere. I left the row uncited under coveragejsonscores rather than flag every column at once, because the resolution is a family-wide decision and not a JSON annotation. Work: pick one - implement the two replays in all nine bindings and then cite the row, or downgrade the row to partial everywhere and cite it so the guard holds the weaker claim. Either way it must end with fixtures on the row, because an uncited wire-bearing row is exactly the blind spot this guard was built to close.
- [ ] [#rootlevelconformance] Root-level conformance fixtures cannot be audited by the coverage-claim guard. arena_blob.json, snapshot_*.json and delta_*.json carry no area, and the two area-scoped guards (lazily-cpp, lazily-kt) restrict their audit to REQUIRED_AREAS, so a root-level fixture is neither replayed nor listed in KNOWN_UNCOVERED there - the ledger simply cannot express it. lazily-cpp says so in a comment: root-level (8) snapshot/delta/arena_blob IPC wire fixtures, test_ipc.cpp hand-builds the equivalent structures. That leaves the Shared-memory blob path row and the Snapshot/Delta half of the IPC wire row unbacked, and I cited only area-qualified fixtures rather than let the guard return a confident wrong answer. Work: give the root corpus a real area in both area-scoped guards (or move those fixtures into an ipc/ area upstream), then cite the two rows. Second, smaller item in the same ledger family: lazily-dart lists distributed/crdt_sync_frames.json in KNOWN_UNCOVERED with no reason of its own - it sits under the comment belonging to the msgpack entry above it - in a ledger that requires a stated reason per entry.
- [ ] [#lzassertionblockschema] [recommended] Extend lazily-spec's existing JSON Schema coverage from the wire field to the ASSERTION blocks. Raised by the operator as a question about whether Protobufs would help the key-validation work; the schema instinct is right but protobuf is the wrong instrument here, because codegen replaces the observable dynamic key access every tracker depends on with compiled struct fields, which is exactly the blind spot lazily-go found in #lzconsumednotasserted, and proto3 tolerates unknown fields by design so it would not even give rung 2. The instrument already exists: 14 schemas under lazily-spec schemas, using additionalProperties and required, with tests test_schema_conformance.py carrying both-directions stale-form rejection tests after a real slot_id and base64 drift incident. Two gaps. First, those schemas validate the wire field, while every defect found in #lzassertunknownkeys and #lzconsumednotasserted lived in expect, expected and assertions, which are entirely unschema'd. Second, roughly 13 of 31 fixture areas have a schema at all - collections, ingress, materialization, reactive-graph and temporal have none. Schema the assertion blocks and #lzwqfixtureconfig closes structurally, since visibility_timeout and max_deliveries stop being hardcoded out of band in nine runners.
- [ ] [#lzfixturediscrim] A fixture can be opened, have every key consumed, have every key asserted - and still be unfalsifiable, because its data cannot distinguish the claim it makes. Found in lazily-cs while closing #lzconsumednotasserted: reversing SignalingRoom's roster sort still PASSED, because the canonical anti_spoof_session.json transcript never produces a roster with more than one entry, so no ordering is observable and every emitted frame stays byte-identical. This is the fourth rung - fixture-opened, key-consumed, key-asserted, claim-discriminating - and no runner-side guard can reach it, because it is a property of the corpus data rather than of runner code. Only a mutation against the LIBRARY exposes it. Audit lazily-spec claim by claim: for each assertion key, mutate the behaviour it names and confirm at least one canonical fixture reddens; where none does, either strengthen the fixture data or record the claim as untested with a reason. lazily-cs worked its instance around locally by driving a three-peer room plus a vacuity guard that fails if the roster drops below two, but the corpus fix belongs upstream in lazily-spec so all nine bindings inherit it.
- [ ] [#lzrung2gaps] Conformance runners still outside the unconsumed-key guard entirely, all surfaced while closing #lzconsumednotasserted. lazily-cpp: nine fixture-replaying runners (command, distributed, lossless_tree, receipts, seqcrdt, statechart, stdlib, topic, reactive_graph) carry hand-rolled unknown-key guards instead of the shared AssertionKeys tracker, so they get neither rung 2 nor rung 3, and test_command_conformance.cpp has no unknown-key guard over its expect block at all. lazily-cs: StateChartConformanceTests has no FixtureAssertions tracker because its steps mix op inputs with assertions. Same class in lazily-kt, where bringing MaterializationConformanceTest under the tracker exposed two keys no runner had ever read - default_mode, and observe in deferral_not_deallocation.json. Convert each to the shared tracker so both rungs apply, and mutation-check every conversion rather than trusting the green that follows it.
- [ ] [#lzstructfieldreads] Typed bindings that decode a fixture block into a struct have a blind spot no map-based key tracker can see: an unused local will not compile, but an unused FIELD is silent, so the key counts as consumed at decode time and is then discarded. Found in lazily-go while closing #lzconsumednotasserted, where open_handles and schema_version were both decoded and never read - one of them stayed hidden precisely because its field name collided with a real method. The fix there is TestConformanceStructFieldsAreRead, a go/ast scan of the package's own test sources with a both-directions excuse allowlist. lazily-go is covered. Check whether lazily-rs (serde structs), lazily-cs, lazily-cpp and lazily-zig decode any fixture block into a typed struct rather than reading it through their key tracker, and port the equivalent scan wherever they do.
- [ ] [#lzassertwithseam] The non-equality escape hatch added under #lzconsumednotasserted marks a key asserted BEFORE it runs the caller's check, so a closure that ignores its argument marks the key without proving anything. Flagged by lazily-rs about assert_key_with, and the same seam exists in every binding's equivalent - it is the one place in the new guard held by discipline rather than by the type system, which is exactly the shape of defect this whole ladder exists to remove. Tighten it so the fixture value must actually be consumed: hand the closure a wrapper that records its own read and mark the key asserted only if that read happened, or narrow the API to a fixed set of comparison combinators. Mutation-check by writing a closure that ignores its argument and confirming the guard reddens.
- [ ] [#lzcpptranscribedrunners] lazily-cpp has fully-decoupled runners that hand-transcribe expectations and never open a fixture at all: test_reliable_sync.cpp, test_family_sync.cpp and test_outbox_store.cpp. The #lzassertunknownkeys guard cannot touch them because there is no assertion block to guard — a transcription has no unconsumed key. The cpp coverage script already reports the corresponding fixtures as uncovered rather than claiming them, so the ledger is honest, but the transcribed values can silently diverge from the corpus forever and the tests keep passing against the old numbers. This is the same defect the zig queue-family runner had before c103070 replaced its transcriptions with a real replay. Convert each to replay the canonical fixtures, then confirm the runtime manifest count rises and remove the matching KNOWN_UNCOVERED entries. Also note the root-level IPC corpus in cpp has no runner at all and is excused the same way.
- [ ] [#lzreactdtsgate] lazily-react ships src/*.d.ts and a `types` export but its `make check` is only `node --check` syntax validation plus `node --test` (18 tests), so nothing typechecks the declarations it publishes. Same hole that let lazily-js ship four TS2749 errors in transport.d.ts undetected until this cycle. Port the gate landed in lazily-js 553d840: a declaration-only tsconfig (include src/**/*.d.ts, noEmit, strict, skipLibCheck:false, allowJs:false), an npm typecheck script wired into both `make check` and CI, plus CI's non-empty-program assertion, since a gate whose include matches nothing also exits 0. Mutation-check by breaking one declaration and confirming a non-zero exit before the tests run.
- [ ] [#lzqueuefamilyflavors] Ship the thread-safe and async queue-family flavors in lazily-js, lazily-zig and lazily-cs - the 18 cells that coverage.json rows 17/18/20/21/23/24 show absent. IMPLEMENTATION HAS LANDED IN ALL THREE BINDINGS: lazily-js 67a0013, lazily-cs f056dd8, lazily-zig c103070 (the last one mutation-checked with 15 defects across the shared core and all six new shells, green on 0.15.2 787/791, 0.16.0 849/849 and master 849/849, CI 30523011815 green). Do NOT re-implement. What REMAINS is the ledger and the plumbing: flip rows 17/18/20/21/23/24 for the JS, Zig and C# columns in lazily-spec/coverage.json, regenerate the twelve README/doc coverage tables via sync-coverage.mjs (--check is the gate), fix lazily-zig README line ~92 which claims every spec row is shipped for the Zig column while the generated table still shows those six as absent, and bump the superproject gitlinks for lazily-js, lazily-cs and lazily-zig. Verify each mark against that binding's own 3x3 ledger and replayed corpus before flipping it, not against the commit message.
- [ ] [#lzwqfixtureconfig] Put the WorkQueueCell lease configuration into the canonical workqueue_*.json fixtures' initial block: visibility_timeout and max_deliveries are absent, so every binding hardcodes them out of band (10, and max_deliveries 3 for competing_delivery vs 2 for lease_deadletter) and a new binding has to read lazily-go's runner to discover them. Add the fields to the fixture and the schema, then update the nine bindings' runners to read them, so a fixture change cannot silently mean something different in each binding.
- [ ] [#lzegress] Design and implement the egress family as the mirror of ingress (#designimplementtransport): an EgressCore owning sequence assignment, the unacked window, the ack watermark, and retry budget, with sync + thread-safe + async shells exposing pending/inflight/acked_through/retry as Computeds and ONE Effect per attached transport as the sink attachment. An Effect alone cannot be the family — it has no delivery state — so the Effect attaches, the core decides. Reuse Outbox/RelayCell for conflation+backpressure and SpillStore/DurableOutbox for durability rather than reimplementing them; add a generation fence on the SEND side so a reconnected producer cannot replay under a stale incarnation. Spec + formal + conformance corpus alongside, as ingress did.
- [ ] [#lzingressadapters] Extend the shipped IngressCell family with boundary-ingress adapters learned from the agent-doc 0.35.100 build; do not create a second ingress primitive. Phase 1: specify an event-channel-first adapter contract for sync, thread-safe, and async sources, with RPC/poll adapters allowed only at external non-reactive boundaries and all ingress updating Sources. Phase 2: provide atomic snapshot-plus-event bootstrap with monotonic cursors, gap detection/cold replay, generation fencing and hot resubscription so controller or producer replacement cannot strand a graph on an old stream. Phase 3: derive readiness, authority, freshness, membership, delivery convergence and template-style validation in Computeds (extract every reactive read into the computed); keep subscription lifecycle, I/O, acknowledgements, retry and commits in Effects. Include keyed SourceMap membership and one-shot durable delivery receipts so temporarily reaching zero subscribers is not mistaken for convergence. Phase 4: map agent-doc onto the adapters as the reference integration, then add spec, formal model, cross-binding conformance fixtures and mutation probes for restart during bootstrap, stale-generation events, cursor gaps, zero-member delivery, duplicate acknowledgements, backpressure and no-torn-observation. Reuse #designimplementtransport semantics and coordinate with #lzegress rather than duplicating either core.
- [ ] [#kny1] Please help me setup trusted publishing with npm.
- [/] [#lzjspublish] After the operator completes npm security-key reauthentication in the Brave package-access tab, configure the GitHub Actions trusted publisher for lazily-hub/lazily-js using workflow release.yml with npm publish permission; rerun release job 30309980372 for v0.30.1 and verify the registry resolves 0.30.1.
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
- [ ] [#lzcoveragelayout] Restructure the coverage.json feature-table layout for a growing language axis per tasks/software/plan-lazily-coverage-table-layout.md. Measured: the feature column is 68% of row width at mean and 84% at max (feature text mean 86 / max 203 chars vs ~40 for all nine marks combined), so a 10th language adds ~4 chars to a row already 130-250 wide — the language axis is NOT the problem. Reject transposing (64 columns) and reject splitting by language: sync-coverage.mjs injects the same full matrix into all 12 targets, and per-language tables would destroy the cross-binding parity view that check-conformance-coverage.sh exists to protect. Phase 1 (land BEFORE the GDScript column, independent of the rest): add id + short label per row and move the 86-203 char normative prose to footnotes or into cell-model.md/protocol.md, taking the table from ~250 to ~60 chars wide so the layout scales past 15 languages. Phase 2 with it: split the '— absent OR not applicable' mark, since GDScript's parked thread-safe family is a different fact from unwritten work. Phases 3-4 (follow-up): author a family field — 54 distinct prefixes across 64 rows means families CANNOT be inferred from the current text — then a family x language roll-up with a stated roll-up rule. Layout and mark-value changes stay separate commits; sync-coverage.mjs --check stays the gate.
- [ ] [#lzdependencyavailability] Implement tasks/software/plan-lazily-dependency-reactive-availability.md: model every late-bound dependency as a stable per-key reactive with explicit unavailable/available state so Computeds subscribe before first publication; cover single-threaded, thread-safe, async, and binding parity, then remove Agent Doc compatibility workarounds.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzstdlibmatrixrelease] Implement Phase 5 of tasks/osoftware/plan-lazily-stdlib-conformance-rollout.md: run the capability-driven all-peer/all-channel matrix, enforce CI and mutation gates, publish coverage evidence, and release bindings inside-out only after every claimed cell is green.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
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
