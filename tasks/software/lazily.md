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
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260726-174052.md`*

Compacted content:
- Archived 26 response topic(s): the done-archive quote bug is fixed, not worked around; Phase 0; Phase 1; 23 more
- Prior summary/context: prior compacted content: Archived 28 response topic(s): create lazily-cs (C#); complete all lazily-cs features; queue...

### Re: do [#lazilyupgradeconformance] — opus-5

Done in all three remaining bindings — rs, dart, zig — each committed, pushed, and gated green. The static grep is gone from the family except where a binding never had one.

**The upgrade earned its keep.** Every binding's mutation check ran the same probe: disable one fixture's replay while leaving its filename in the source. In all three the suite stayed green *and the old static guard stayed green* — rs at 118/124, dart at 111/124, zig at 94/124 — while the runtime guard failed and named the fixture. Same tree, opposite verdicts. That is the whole case for the change.

**What the manifests found, per binding:**

- **rs**: one real hole. `tests/crdt_tree_laws.rs` is `#![cfg(feature = "lossless-tree")]`, no `make check` target enabled that feature, and `test-lossless-tree` enumerates its binaries explicitly — so the entire file compiled to nothing while `crdt-tree/algebra.json` sat in the source where the grep counted it. Fixed by adding `--test crdt_tree_laws`; 118 named → 117 opened. The four `topiccell_*.json` I expected to fall out did not: `topic_conformance.rs` still hand-transcribes them, but the new `queue_family_conformance.rs` topic replays open all four, so the manifest sees them.
- **zig**: 22 fixtures fell out, 8 of them the real thing — inline mirror tests asserting against values typed into the `.zig` source with the fixture name only in a comment. The other 14 are vendored `@embedFile` copies, which a compile-time embed can never record; rather than excuse them, the agent added a test that opens the *canonical* file and asserts the vendored copy is byte-identical, so vendored drift now fails instead of silently replaying yesterday's corpus.
- **dart**: 111/124 unchanged — no named-but-never-opened fixture at all.

**dart's real finding was in the recorder, not the corpus.** `FileMode.append` in Dart is not POSIX `O_APPEND`: the end-of-file offset is captured when the handle opens, so concurrent isolates clobber each other. Its first manifest held severed id fragments (`sure.json`, `_and_leaf_edit.json`) that made the guard report three *intact* fixtures as never opened — the recorder manufacturing the exact drift it exists to detect. Two more attempts (fcntl locks are per-process, useless across `dart test` isolates) landed on an `O_EXCL` lockfile plus explicit seek-to-length: measured over 4 processes × 4 isolates × 3200 lines, plain append kept 2009 with 199 mangled; the final shape kept 3200 with 0. It also added a check the Go reference lacks — reject any recorded id naming no file in the corpus — which is now queued for the other three.

Two build-system traps worth recording: `zig build test` **caches Run steps**, so a second run skipped every test binary and left the freshly-truncated manifest empty (fixed with `has_side_effects` plus an explicit child env when the variable is set); and rs needed `conformance-manifest-reset` as the *first* prerequisite of `check`, which makes `make check -j` unsafe by construction.

Also closing `#lazilythreadsafetopiccel` and `#lazilythreadsafeworkqueu` — both shipped earlier today in lazily-rs 0.51.0 with all eleven canonical fixtures replayed per flavor and ten mutation probes confirmed.

**One finding I am not closing.** The rs agent hit `concurrent_push_and_read_do_not_deadlock` failing with `left: 199, right: 200` mid-run. It is not a deadlock — it is a lost or stale reactive read in `ThreadSafeQueueCell`, and it is pre-existing: 0/25 in isolation, but 2/30 on the working tree and 1/30 on stashed HEAD under saturating CPU load. That is a correctness defect in code released as 0.51.0, on the exact surface the thread-safe flavor exists to provide, so it goes to the top of the backlog rather than into this item.

### Re: do [#lazilyzigconformance] — opus-5

Done, and it was worth more than the ledger line suggests. **86/124 → 94/124 opened, `KNOWN_UNCOVERED` 38 → 30**, all eight mirrors gone. Committed and pushed as `ff00d3f`; both gating toolchains green (nightly 786/786, 0.16.0 771/771) and `make check` clean.

**Replaying them found three real wire defects.** Every mirror had been transcribed from *this implementation* rather than from the corpus, so each one certified the bug it was standing next to:

- **The stamp frontier was the wrong shape on the wire.** `schemas/distributed.json#/$defs/StampFrontierEntry` pins a 2-element `[peer, stamp]` tuple; lazily-rs models it as `Vec<(u64, WireStamp)>`. lazily-zig encoded *and* decoded `{peer, stamp}` — so it round-tripped against itself perfectly and its frontier advertisement was unreadable by every other binding.
- **`CrdtOp.key` was omitted when null and rejected when explicitly null**, though the schema lists it in `required` and documents the nullable-but-present rule as the deliberate difference from `NodeSnapshot`/`NodeAdd`. A keyless op could be neither produced nor read the way the corpus spells one.
- **`welcome.peers` came out of hash-map iteration order** against an explicit `roster_sorted_ascending` assertion (so a multi-peer room was not reproducible run to run), and the unknown-target error text did not match the transcript — a client matching on the documented message would never have matched.

The mirrors had also already drifted on their own terms: the idempotence mirror used stamps `(10,0,1)`/`(11,0,1)` where the fixture uses `(10,0,1)`/`(12,0,2)`; the reverse-order mirror dropped the third op, the one that makes "same winner regardless of order" non-trivial; and all three semtree scenarios shared one hand-built tree, including the removal scenario whose canonical tree is a different shape with coincidentally identical totals.

**Every new runner was mutation-checked** — break exactly the behaviour one fixture pins, require the suite to fail *and* name the right runner. 10 probes, 10/10 discriminated. Three of them initially came back `MISATTRIB` because the mutation broke the *build*; a compile error proves nothing about a runner, so the probes were rewritten to be behaviour-only and the harness now reports a build break as an invalid probe rather than a pass.

Two things outside the item that were blocking it. **CI never ran this guard at all**: it invoked bare `zig build test`, so the recorder collected no evidence and `check-conformance-coverage.sh` only ever ran under a local `make check` — the ledger was enforced by whoever remembered. It now runs in CI, and the fixture-presence assertion covers `collections`/`distributed`/`signaling` so a missing corpus cannot let the new replays skip into a green run. And **the 0.16.0 leg was already red before I touched anything**: `Step.tag` is nightly-only (`id` on 0.16.0), and pinning it made the build *script* fail to compile, so every `zig build` on that gating toolchain died before reaching a test. Fixed with the `@hasField` shim this repo already uses for `normalizeCompute`.

CI run `30219366377` is queued; recording that and not waiting.

### Re: should the JSON `type` field be an enum?

Yes — done. The union tag already *was* the enum; what was duplicated was the wire spelling. `"peer-joined"` appeared as a literal in the encoder and again in the decoder I had just written, which is exactly how a rename becomes a silently dropped frame.

`ClientMessage`/`ServerMessage` are now `union(ClientTag)` / `union(ServerTag)`, with one `wireName` table both halves read and a `fromWireName` derived from it over `std.meta.tags`. So the encoder and decoder cannot disagree about a spelling, and a variant added to the union appears in the decoder without being retyped. It follows `reactive_map.EntryKind`'s existing `wireName`/`fromWireName` convention. The mutation probe that renames `"peer-joined"` → `"peerJoined"` now fails the frames replay, so the table is load-bearing.

### Re: should `ClientTag` go to spec + formal + conformance + implementations?

> **Queue prompt:**
>
> do [#lazilyzigconformance]


Split it in two, because the answer differs:

**The tag set — already spec-level, nothing to add.** `schemas/signaling.json` is a `oneOf` over 14 branches, each with `"type": {"const": ...}` and `additionalProperties: false`. The closed set is pinned, and `frames.json` enumerates it with a `variant` field. `ClientTag` is just the Zig-idiomatic realization of something the spec already owns.

**`ClientTag` as a named type — no, don't propagate it.** That is implementation shape, not wire contract. Kotlin reaches for a sealed class, Rust for `#[serde(tag = "type")]`, Python for a `Literal` union; mandating a named tag enum in nine bindings buys no interop and costs nine deprecation surfaces on public types. The spec deliberately does not prescribe this elsewhere — `EntryKind.wireName` is a zig-ism too.

**Formal: no.** A finite tag set already pinned by a schema has no theorem in it. Lean earns its keep on confluence, memo suppression, move-minimization — not on an enum.

**Conformance: yes, and this is the real gap.** What `ClientTag` actually buys is *rejection* — an unknown `type` is an error, never a silent default. Nothing in the corpus tests that, in any binding. Filed as `#lzsignalingnegative`: a `rejects` block covering unknown `type`, `to` and `from` both present, a client frame carrying `from`, and a `welcome` roster containing the joining peer. That is the portable version of your question, and unlike a type it applies to all nine.

Also filed `#lzwirefrontier` — the two wire defects above are exactly the kind a hand-transcribed frames test cannot see, so the other seven bindings need the same encoder/decoder audit. rs is already correct on both. Both items are at the top of the backlog.

### Re: do [#lzwirefrontier] — gpt-5

Completed the cross-binding `CrdtSync` wire audit. C++ was the only implemented binding with a live mismatch: its standard keyed MessagePack codec encoded and decoded `StampFrontierEntry` as `{peer, stamp}`. It now requires the canonical two-element `[peer, WireStamp]` tuple in both directions. I also added an independent canonical-frame test that separately inspects the encoder shape and decodes a hand-built frame, so a symmetric encoder/decoder mistake cannot pass as a self-roundtrip.

`CrdtOp.key` was already emitted as a required nullable field in C++, Dart, Go, JavaScript, Kotlin, Python, and Rust; their frontier entries were otherwise canonical. C# does not yet expose the IPC/CRDT surfaces, so this audit becomes applicable there after `#lzcsipc` and `#lzcssync`.

The C++ change is committed and pushed as `bc10df2` (`fix CrdtSync frontier wire tuple shape`). `make test`, full `make check`, and the latest C++ CI run `30215769834` are green.

### Re: one network suite across all implementations and data channels — gpt-5

> **Queue prompt:**
>
> do [#lzwirefrontier]


Created a coordinated plan set for one cross-language network suite:

- `src/lazily-interop/docs/peer-contract.md` defines the shared peer protocol, semantic plane, transport plane, five channel names, and no-vacuous-channel rule.
- `tasks/software/plan-lazily-interop-network-suite.md` is the master phased plan for an independent Rust orchestrator, nine peers, mutation proof, rollout, and CI.
- `tasks/software/plan-lazily-interop-peer-rollout.md` defines thin production-library adapters and the binding-by-binding acceptance checklist.
- `tasks/software/plan-lazily-interop-channel-matrix.md` separates frame, codec, and channel dimensions and specifies in-process FFI, shared memory, IPC socket, WebSocket, and WebRTC DataChannel profiles plus fault injection and CI tiers.

The contract and plans are committed locally as `784719be3` (`lazily: plan cross-binding network interop suite`). All four Markdown files parse successfully and the staged diff passed whitespace validation. The root `make check` reached an unrelated pre-existing Corky parallel-test port-allocation race: four loopback tests failed with `AddrInUse`, while that exact test group passed in isolation. I left the unrelated public subrepo unchanged; the in-scope C++ full suite is green.

Queued implementation in four explicit phases: `#lzinteropcore`, `#lzinteropeers`, `#lzinteropchannels`, then dependency-bound `#lzinteropcs`.

### Re: do [#lzinteropcore] — gpt-5

Implemented Phase 0-1 as a standalone Rust workspace in `src/lazily-interop`, with no `lazily-rs` dependency. The deliverable includes:

- frozen Draft 2020-12 control, scenario, and result schemas plus local validation of `lazily-spec/schemas/distributed.json`;
- a versioned NDJSON peer supervisor with command and shutdown deadlines, stderr capture, fail-closed capability validation, and a `peers.toml` registry;
- a deterministic semantic scenario engine that validates each emitted frame, relays explicit delivery order/duplicates, checks `applied`, and compares canonical expected snapshots;
- JSON, JUnit, and concise console reports with pinned categories, culprit/plane diagnostics, staged-peer visibility, and shell-safe replay commands;
- an intentionally independent Rust reference peer with canonical LWW semantics and a real Unix length-prefixed IPC link;
- a mutation runner proving all eight required defect classes go red with the expected culprit: wrong frontier object, omitted null key, reversed stamp comparison, false `applied`, truncated frame, merged frames, swallowed unsupported channel, and non-flushed control output.

Changed paths are the new crate under `src/lazily-interop` (excluding the preserved pre-existing untracked `src/lazily-interop/src/lazily_interop/peer.py`) and the three plan files under `tasks/software/plan-lazily-interop-*.md`. The implementation and plan-status update are committed locally as `ff7d4e17b` (`lazily: add interop orchestrator mutation gate`).

Verification:

- `src/lazily-interop/make check` passed: formatting, Clippy with `-D warnings`, 8 unit tests, 2 CLI integration tests, doc tests, schema checks, and the mutation command.
- The normal strict run passed with two eligible reference peers, 2 emitted frames, 3 deliveries, and C# explicitly reported as staged.
- `cargo run --quiet -- mutation-check` caught 8/8 required mutations with the expected categories and culprits.
- `cargo tree --edges normal` confirms the orchestrator imports no Lazily implementation.
- Scoped `git diff --check` passed.

Confidence: high for the Phase 0-1 gate and its feature/staging semantics. Escalation: the workspace umbrella `make check` remains red outside this change in concurrent `src/agent-doc` work because `ClassifiedEvent.event_name` is unused under `-D warnings`; I preserved that other session's dirty subrepo. The earlier umbrella run also exposed Corky's pre-existing parallel loopback-port race, whose exact test group passes in isolation. The interop crate's own complete gate is green.

### Re: stage unsupported peers by feature — gpt-5

> **Queue prompt:**
>
> do [#lzinteropcore]


Implemented capability-driven peer groups. `hello.features` advertises only production-backed surfaces. A scenario includes peers that support its required feature; unsupported implementations remain visible in `staged_peers`, receive no green credit, and are excluded from the active group. Required codecs and channels still fail closed within that eligible group, and fewer than two eligible peers is red, so feature filtering cannot produce a vacuous pass.

`lazily-cs` is checked into the registry as staged with no executable command. It will join CRDT feature scenarios only after `#lzcsipc` and `#lzcssync` provide the real library surfaces; the plans and peer contract now state this explicitly.

### Re: Response

> **Queue prompt:**
>
> do [#lzinteropeers]


Implemented `#lzinteropeers` as a capability-driven production peer rollout.

- Added thin production-runtime adapters for Rust, Zig, Python, Kotlin, JavaScript, Dart, Go, and C++; each adapter is exercised by its binding’s normal local gate.
- Added progressive 2–8 peer convergence scenarios. The eight-peer feature scenario requires `distributed_crdt` but no state-plane codec, so all implementations that support the feature participate. Codec/channel scenarios retain explicit capability requirements and therefore select narrower peer groups.
- Kept `lazily-cs` registered with `staged = true`; reports show it as staged, it is never started, and it receives no pass credit until its production CRDT/IPC feature surfaces exist.
- C++ advertises only production MessagePack support, normalizes frames through its keyed MessagePack codec, self-checks keyed and positional MessagePack, and runs under ASan/UBSan. It does not falsely advertise JSON state-plane support.
- Corrected Zig’s conformance-evidence invocation so freshly truncated manifests force real test execution rather than accepting cached runs.

Proof:
- changed_paths: `src/lazily-interop/{peers.toml,README.md,schemas/scenario.schema.json,scenarios/*.json,src/main.rs,src/reference.rs}` and the peer adapter/build files in `src/lazily-{rs,zig,py,kt,js,dart,go,cpp}`.
- commands: every binding’s `make check`; `src/lazily-interop/make check`; `cargo run --quiet -- run --scenario scenarios/eight-way-convergence.json --format json --require-channels --timeout-ms 10000`; workspace-root `make check`.
- verification: eight-peer convergence passed with 8 peers started, 8 frames emitted, 11 deliveries, zero failures, and `lazily-cs` reported as staged; mutation gate caught 8/8 defect classes; all binding gates passed; workspace gate passed 8,176 agent-doc tests plus remaining workspace checks.
- commits: Rust `6403b3a`, Zig `c0e8b58`, Python `0a2d6f5`, Kotlin `aea8b58`, JavaScript `5ed7c47`, Dart `0198fed`, Go `f82fd09`, C++ `3d83a61` (all pushed); workspace supervisor commit `00c63466b`.
- confidence: high.
- escalation: none.

### Re: #lzinteropchannels — codex

> **Queue prompt:**
>
> do [#lzinteropchannels]


Implemented and committed the capability-driven five-channel network suite as `cb0558b02` (`interop: add capability-driven channel matrix`).

- **Behavior:** `transport_matrix` expands every channel over every ordered pair of eligible peers. Peers join automatically only after advertising `transport_links`, the required codec, every requested channel, and a concrete variant. Unsupported or incomplete bindings remain visible as staged and receive no pass credit. `lazily-cs` remains registry-staged.
- **Changed paths:** `.github/workflows/lazily-network-suite.yml`; `src/lazily-interop/{Makefile,README.md,.gitignore}`; channel scenario and result/scenario schemas; supervisor types/schema/scenario/reference/mutation modules; peer-contract documentation; CLI tests and feature-gate fixtures.
- **Commands:** `make check` and `make network-suite-pr` in `src/lazily-interop`; JSON/schema parsing; report assertions with `jq`; root `make check`; staged diff validation with `git diff --cached --check` and `tsift diff-digest --cached`.
- **Verification:** focused crate gate passes (8 unit tests, 3 CLI tests, schema validation, and 11/11 required mutations caught). The PR-tier suite passes: semantic plane runs all eight completed production bindings with only C# staged; transport plane runs all five profiles bidirectionally with 10/10 cells passing and exact sent/received byte parity. A regression fixture with three supported peers proves automatic admission as sender and receiver: 30/30 cells (3×2 ordered pairs×5 channels), with C# staged.
- **Workspace gate:** root `make check` reached an unrelated pre-existing `src/agent-doc` test compile error: two `CurrentText::Current` initializers omit the newly required `delivery_version` field (`agent-doc-preflight-io/src/lib.rs:5044` and `:5346`). Those out-of-scope edits were preserved.
- **Confidence:** high for the Lazily interop scope; all local suite, schema, regression, and mutation gates pass.
- **Escalation:** none. The unrelated workspace compile break is recorded rather than modified.

### Re: do [#lzinteropcs] — codex

> **Queue prompt:**
>
> do [#lzinteropcs]


Kept `lazily-cs` staged; activating it now would grant interop credit to an incomplete binding.

- **Dependency state:** both `#lzcsipc` (schema-validated Snapshot/Delta/CrdtSync plus shared-memory/blob transport) and `#lzcssync` (CRDT runtime, resync/outbox, IPC source/sink, full-duplex sync) remain open.
- **Current suite behavior:** C# is present in `peers.toml` with `staged = true`, appears in `staged_peers`, is excluded from active semantic/transport peer groups, and receives no pass credit.
- **Future admission:** after those two prerequisites ship, the C# peer must advertise only its real production features/codecs/channels/variants. The capability-driven matrix from `cb0558b02` will then include it automatically as sender and receiver without a binding-specific scenario edit.
- **Changed paths:** no production paths changed in this cycle; the existing backlog item was narrowed to preserve this activation gate.
- **Commands:** read the live agent-doc backlog and inspected the already-verified PR-tier suite state from `#lzinteropchannels`.
- **Verification:** `#lzcsipc` and `#lzcssync` are open; the latest passing network report lists `lazily-cs` as `registry entry is staged` while completed bindings run.
- **Confidence:** high that staging is the only honest state today.
- **Escalation:** none. The remaining work is explicitly driven by the two C# prerequisite backlog items.
<!-- agent:boundary:b1e180bf -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzinteropcore]~~
- ~~do [#lzinteropeers]~~
- ~~do [#lzinteropchannels]~~
- ~~do [#lzinteropcs]~~
- ~~do [#lzwirefrontier]~~
- do [#lzsignalingnegative]
- ~~do [#lazilyzigconformance]~~
- do [#lazilyzigport]
- do [#lazilyconformancecoverag]
- do [#lazilybenchmarkregressio]
- do [#lzqfjs]
- do [#lzqfports]
- do [#lzqfzig]
- do [#maphandlecleardependents]
- do [#lazilycellfamily]
- do [#lazilyspecdisagrees]
- do [#lzjsnpmtrustedpub]
- do [#lzcsqueues]
- do [#lzcscrdt]
- do [#lzcsipc]
- do [#lzcssync]
- do [#lzcstemporal]
- do [#lzcscluster]
- do [#lzcsplane]
- do [#lzcspackage]
- do [#asyncplanenever]
- do [#lazilyrenameinverted]
- do [#readmeexamplesreference]
- do [#signalgetsignallive]
- do [#recommendeddecidewhether]
- do [#lazilycppconformance]
- do [#lazilycppcollections]
- do [#trackedcopieslazily]
- do [#firstunificationwire]
- do [#prototypeunifiedreactive]
- do [#lazilyphaseextract]
- do [#unifythreemap]
- do [#lazilymapunification]
- do [#zigdefectsfound]
- do [#lazilyasynccomputedmapde]
- do [#lazilytwoimplementations]
- do [#lazilydartcomputedmap]
- do [#recommendedspecdecision]
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzinteropcs] Keep lazily-cs visible but registry-staged in the single network suite. After #lzcsipc and #lzcssync ship real schema-validated IPC/CRDT and full-duplex sync surfaces, implement its production interop peer, advertise only the features/codecs/channels/variants it actually supports, then let the capability-driven matrix admit it automatically; stays open until both prerequisites and the C# peer are complete.
- [ ] [#lzsignalingnegative] lazily-spec + all bindings: signaling/frames.json and anti_spoof_session.json have no NEGATIVE cases, so nothing anywhere pins that a malformed frame is REJECTED rather than silently defaulted. The schema already closes the tag set (oneOf over 14 branches, each type: {const}, additionalProperties: false), but no fixture drives it. Add a rejects block covering: unknown type value; to and from both present on one frame (the description says they never are); a client frame carrying from (the anti-spoof surface itself); welcome.peers containing the joining peer. This is the portable form of the tag-enum question — the wire contract is the closed tag set plus rejection, not any binding's type shape
- [ ] [#lazilyzigport] lazily go/rs/zig: port lazily-dart's corrupt-manifest-id check into the other runtime conformance guards. dart found that a recorder can mangle its own evidence (severed ids like sure.json) and make the guard report INTACT fixtures as never opened — a guard that cries wolf gets switched off. Its guard now rejects any recorded id that names no file in the corpus. go, rs and zig all lack that check. Cheap: one loop over the manifest before the coverage comparison
- [ ] [#lazilyconformancecoverag] lazily-cs: 87 of 124 canonical fixtures unreplayed (cs opens 37). MEASURED via the runtime manifest 2026-07-26. CORRECTION — an earlier note on this item claimed the 7 statechart fixtures were a cheap "runner gap"; that was derived from the STATIC grep and is WRONG. cs enumerates its corpus, so no fixture filename appears in its sources, and the static count under-reported every area. Runtime truth: cs FULLY replays reactive-graph 21/21, statechart 7/7, materialization 3/3, familysync 1/1. There is NO runner-gap win anywhere in cs — every one of the 87 is a genuinely missing surface, each already declared in a per-runner Unsupported ledger with the blocking surface named. Unreplayed by area: collections 15, lossless-tree 9, root 8, message-passing 8, reliable-sync 8, rateshape 6, coordination 5, resilience/service/temporal/windowing 4 each, presence 3, agent-doc/distributed/signaling 2 each, crdt-tree/membership/receipts 1 each. Work it down against tasks/software/lazily-cs-parity-plan.md by SHIPPING SURFACES, not by wiring runners
- [ ] [#lazilybenchmarkregressio] lazily-rs: the benchmark regression budgets have NO CI enforcement. ci.yml enumerates individual cargo test invocations and never runs `make check` or `benchmark-check`; regressions.yml runs only the loom model. So every budget in BENCHMARKS.md is enforced solely by whoever remembers to run `make benchmark-check` locally with a populated target/criterion — which, until #lazilyfailstarget, was nobody, because the gate hard-failed on a pruned target dir. Add a scheduled job (regressions.yml already has a nightly cron and is the natural home) that runs `cargo bench` then `make benchmark-check-strict`, so a real regression fails somewhere other than one developer machine. Note the numbers are machine-specific, so the job needs a stable runner class or relative-delta budgets rather than absolute ones
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#maphandlecleardependents] MapHandle::clear_dependents exists only on the SYNC handle trait (lazily-rs cell_family.rs:134); the ThreadSafe and Async handle traits have no disposal hook. PARTIALLY RESOLVED 2026-07-26 — the blocker claim was wrong and is now disproven in practice. lazily-cpp shipped it: both thread-safe traits dispose through ctx.context() (BasicThreadSafeContext and AsyncContext each project onto a real Context, so no new primitive was needed), and AsyncSlotHandle gained a clear_dependents that drops the cached value since it has no graph dependents to clear. lazily-cs shipped it too, and found the reason it matters: AsyncComputedMap.Remove never disposed the removed node, so a reader holding the handle kept being served the removed entry's last resolved value. REMAINING: rs's own ThreadSafe/Async handle traits still have no hook; py has none on ANY flavor, so its removes drop the orphaned node without teardown consistently across all three. Note cs also needed a TryGetHandle before the disposal was observable enough to test at all
- [ ] [#lazilycellfamily] lazily-rs cell_family.rs:344-345 documents the present set as "only grows (deferral, not de-allocation)" ten lines above a `remove` that shrinks it (:325-334). Pre-existing doc bug, independent of any refactor — fix the comment or scope it to the materialization plane.
- [ ] [#lazilyspecdisagrees] lazily-spec disagrees with itself on the Dart thread-safe row: coverage.json marks the ThreadSafeComputedMap row shipped for Dart, while docs/coverage.md:85-87 declares the thread-safe context "not applicable" for Dart (single isolate, no shared mutable heap). Resolve in the spec — this is the same question as whether a thread-safe flavor is meaningful on a single-threaded runtime.
- [ ] [#lzqfzig] lazily-zig: the queue family is NOT graph-backed — QueueCell holds a *Context but mints no node; reader-kinds are five u64 version counters callers must poll. It scores green on the coverage row anyway. Make it graph-backed before adding flavors, or the flavor work lands green against nothing. Same finding as zig's sync ReactiveMap; consider one pass for both — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfjs] lazily-js: QueueCell/TopicCell/WorkQueueCell are not reactive at all — src/queue.js takes no context, contains ZERO ctx references, and reports invalidation by diffing a private snapshot. It scores green on the coverage row. Bind the family to a context and mint real reader-kinds, and check whether the collections fixtures are being satisfied by a non-reactive model here the way test/collections.test.js drives src/collections.js instead of the reactive families — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfports] lazily: Phase 5 — port the queue-family flavors to cpp, dart, py, go, kt (and build cs flavor-complete from the start under #lzcsqueues). One subagent per binding, NEVER two against one repo. Dispatch only after #lzqfspec + #lzqffx + #lzqfrs are committed so each agent has a settled spec, a red per-flavor gate, and a reference diff. cpp additionally needs its Source<uint64_t> version cells migrated if Phase 0 picks memoized-derive. KEPT OPEN 2026-07-26: #lzqfspec and #lzqffx are now committed, but #lzqfrs (the rs reference implementation) is not, so the stated precondition is unmet — dispatching now would be six independent guesses at an unpinned shape. Unblocks the moment #lzqfrs lands — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#firstunificationwire] FIRST, before any unification: wire the collections atomic-move fixtures (cellmap_atomic_move.json, cellmap_independence.json) into every binding replay harness against ALL THREE flavors. Today zig never replays them at all (only doc-comment references at reactive_map.zig:32,:39) and js replays them only against its fourth, non-reactive collections.js model — so the reactive moveTo has no fixture backing anywhere. Without this the unification lands green against nothing. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#prototypeunifiedreactive] Prototype the unified ReactiveMap shape in lazily-zig and lazily-cpp — NOT lazily-cs. cs ships only 13 of 48 coverage rows (35 absent) and has NO thread-safe or async SourceMap at all; its conformance substitutes a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336). Prototyping a unification against two families that do not exist would invent the surface rather than converge one, and cannot surface what breaks. zig and cpp are feature-complete, can unify all three families, and neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilyphaseextract] lazily-rs Phase A: extract the closure-free KeyedOrder<K,H> core (entries + order + membership + present-set + position/keys/len/contains_key + move_to/move_before/move_after) shared by all three families; thin per-model shells keep materialize/observe. Do NOT route through SyncReactiveGraph — its Send+Sync bound is stricter than the concrete Context API and would break Rc-capturing callers. `remove` stays out of the core (monotone present set is fixture- and Lean-backed). Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#unifythreemap] Unify all three map families in lazily-zig and lazily-cpp — both can fold async in, unlike the rest. zig: fn ReactiveMap(comptime Ctx,K,V,kind) with Ctx.Handle/Ctx.Observed/Ctx.Lock projections; the proof already exists in-tree as Engine(comptime Model) at reactive_graph_conformance.zig:1206 over SyncModel/TsModel/AsyncModel. cpp: traits class + existing duck-typed Cx param; only Source::clear_dependents(Context&) and Computed::clear(Context&) block it, and AsyncSlotHandle needs one added clear_dependents. Neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilymapunification] lazily-go map unification BEFORE it hits v1 — still v0.22.0 on an unversioned module path, so a breaking change costs nothing now; after v1 it costs a /v2 module path plus every import rewrite. Mechanism: closure fields (already proven in-repo at reactive_map.go:92-96) or one extra type param. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#zigdefectsfound] zig defects found while surveying, independent of unification: (1) the sync ReactiveMap is NOT graph-backed — stores plain V and fakes reactivity with u64 counters callers must poll, so keys()/len()/contains() register no edge (reactive_map.zig:140-419 touches ctx only for .allocator); (2) ThreadSafeComputedMap mints the WRONG node kind — mintLocked always calls tsctx.cell ignoring entry_kind (thread_safe_reactive_map.zig:98), so its computed entries are cells; (3) moveTo desyncs on allocator failure — orderedRemove then insert-catch-return-false leaves the key in materialized but absent from order (reactive_map.zig:288-303).
- [ ] [#lazilyasynccomputedmapde] lazily-cs AsyncComputedMap defects: Len() is documented as reading through the membership plane but returns _order.Count, and _membership is written at :117 and never read anywhere (dead code, so Len is non-reactive); Remove (:92-98) never disposes the removed AsyncComputed, unlike both sync clear delegates (ReactiveMap.cs:299,:405), leaving readers on a stale cache. Also: no thread-safe or async SourceMap exists at all — conformance papers over it with a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336).
- [ ] [#lazilytwoimplementations] lazily-js: two implementations disagree on moveTo with a negative index — reactive-family.js:270 uses Math.min(index, Math.max(len-1,0)) with no lower clamp, collections.js:196 uses Math.max(0, Math.min(index, len-1)). moveTo(key,-1) splices before the last element in one and at 0 in the other. Untested in both; pick one before any unification silently changes a published surface.
- [ ] [#lazilydartcomputedmap] lazily-dart: ComputedMap entries are Slot, not Computed (collections.dart:430). Post-v2 Slot is the UNGUARDED pass-through while Computed carries the == memo guard — so ComputedMap does not have the guard its name implies, and this leaks publicly through handle(). Decide whether the handle becomes Computed.
- [ ] [#recommendedspecdecision] [recommended] Spec decision for lazily-spec, blocking the family unification: (1) does the atomic-move contract in cell-model.md:275 bind EVERY flavor or only the source map? All nine bindings fail it on thread-safe and async, yet coverage.json marks all three rows green for every binding. (2) Is a thread-safe flavor meaningful on single-threaded runtimes? dart ships a 182-line duplicate family the spec marks N/A, and js README:633-640 argues the opposite of its own source comment at thread-safe.js:134-137.
- [ ] [#lazilycppconformance] lazily-cpp: 10 conformance runners are MARKER-ONLY — they open the canonical fixture but only assert its `"model"` string, then replay hand-transcribed steps (rateshape 6, coordination 5, temporal/windowing/service/resilience 4 each, presence 3, membership/crdt-tree 1 each = 33 fixtures). They pass a did-you-load-fixtures check while being blind to any change in the corpus steps or expectations. Convert to real replays; the coverage guard counts them as covered today.
- [ ] [#lazilycppcollections] lazily-cpp: 15 collections fixtures still uncovered — 9 with no coverage at all (cellmap_atomic_move, cellmap_independence, keyed_reconciliation_lis, semtree_incremental, stableid_alignment, textcrdt_convergence, textcrdt_delta_sync, workqueue_competing_delivery, workqueue_lease_deadletter) and 6 hand-transcribed (mergecell_algebra, 5x queuecell_*). Largest single body of remaining conformance work; spans five separate primitives. Tracked in the KNOWN_UNCOVERED ledger.
- [ ] [#trackedcopieslazily] The tracked copies of lazily-spec conformance fixtures inside lazily-py (tests/conformance), lazily-dart (test/conformance), lazily-rs (tests/conformance) and lazily-go (test/conformance) are kept byte-identical to canonical BY HAND with no sync script or drift check. This cycle had to flip the same fixture in three places. Add a sync/verify script (or make the runners read the sibling checkout) so a canonical edit cannot silently miss a vendored copy.
- [ ] [#asyncplanenever] Async plane never migrated to the v2 kernel: AsyncCellHandle / AsyncSlotHandle / AsyncSlotState survive in cpp, go, js, kt, py, zig, cs. Sharpest case is lazily-cpp `AsyncContext::memo()` (async_context.hpp:144), still live and undeprecated while the sync `Context::memo` is DELETED. Migrate to Async{Source,Computed} with deprecated aliases.
- [ ] [#lazilyrenameinverted] [recommended] lazily-py keeps `Cell`/`CellSlot` as the native class names with `Source = Cell` / `SourceSlot = CellSlot` aliases (cell.py:35,191,219-220). This is NOT drift — it is documented and motivated: cell.py is inside the mypyc compilation unit (setup.py compiles slot/cell/signal/effect/batch as one unit for cross-file native-class inheritance BaseSlot -> CellSlot), and the docstring states Cell "stays the native class name (for mypyc native-struct reads and isinstance stability across the family)". Flipping would rename the native class and change isinstance identity. Do NOT flip without deciding whether mypyc native-struct naming or family-wide vocabulary consistency wins; that is a spec/ownership question, not a cleanup.
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts. RECOMMENDATION 2026-07-26 (agent): KEEP *Cell. The kernel renamed the two value-node KINDS (Source/Computed) and kept Cell as the concept for a value-bearing reactive node. A QueueCell is not a kind — it composes them (five Source<u64> reader-version cells plus derived readers), so QueueSource would imply "written from outside, folds under a merge policy", which is false for a queue. Same for TimerCell/LeaseCell/CircuitBreakerCell/PresenceCell: each is a composite of a pure core plus projecting cells. KEPT OPEN: naming must converge with the operator before editing (9 bindings in lockstep + deprecation surface on every public type). Unblocks on a yes/no. If YES, scope is: spec cell-model.md first, then 9 bindings in lockstep with deprecated aliases, plus a decision on whether MergeCell collapses entirely (under the kernel it is already Source<T,M> with a non-default policy)
- [ ] [#lzjsnpmtrustedpub] lazily-js: register @lazily-hub/lazily-js as a trusted publisher on npmjs.com so release.yml can publish unattended. The workflow is correct — its OIDC exchange succeeds and provenance is signed and logged to Sigstore — but the upload is rejected with E404 PUT https://registry.npmjs.org/@lazily-hub%2flazily-js, meaning npm does not associate the package with that publisher (or the token lacks scope permission). This has now E404'd on two consecutive releases and each time required a manual 'npm publish --access public --otp=<code>' from the operator. Fix is npmjs.com-side configuration, not a code change
- [ ] [#lzcsqueues] lazily-cs: QueueCell + QueueStorage, TopicCell, WorkQueueCell, RelayCell with BackpressurePolicy/SpillStore/Transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16)
- [ ] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [ ] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [ ] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [ ] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
