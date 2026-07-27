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
> do [#lazilyrenameinverted]


> **Queue prompt:**
>
> do [#asyncplanenever]


> **Queue prompt:**
>
> do [#lzcscrdt]
>
> do [#lzcsipc]
>
> do [#lzcssync]
>
> do [#lzcstemporal]
>
> do [#lzcscluster]
>
> do [#lzcsplane]
>
> do [#lzcspackage]


> **Queue prompt:**
>
> do [#lzcsqueues]


> **Queue prompt:**
>
> do [#lzjsnpmtrustedpub]


> **Queue prompt:**
>
> do [#lazilyspecdisagrees]


> **Queue prompt:**
>
> do [#lazilycellfamily]


> **Queue prompt:**
>
> do [#maphandlecleardependents]


> **Queue prompt:**
>
> do [#lzqfzig]


> **Queue prompt:**
>
> do [#lzqfports]


> **Queue prompt:**
>
> do [#lzqfjs]


> **Queue prompt:**
>
> do [#lazilybenchmarkregressio]


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

### Re: #lzsignalingnegative — gpt-5

> **Queue prompt:**
>
> do [#lzsignalingnegative]


Implemented and published capability-gated negative signaling conformance.

The canonical fixtures now require rejection of all four requested malformed cases: an unknown tag, a directed client frame carrying both `to` and `from`, the anti-spoof client-supplied `from`, and a `welcome` roster containing the joining peer. The spec tests select the direction-specific schema branch, so a client frame cannot accidentally pass by matching the corresponding server variant; the welcome self-roster rule is checked semantically.

Participating production decoders are Rust (including the TypeScript Worker/client), Python, Kotlin, JavaScript, Go, Dart, and Zig. Each now rejects unknown/extra per-variant fields and the welcome self-roster condition, and each runner replays the canonical `rejects` arrays rather than hand-transcribing them.

Feature staging is explicit rather than credited as a pass:

- C++ remains staged for this decoder feature because its existing conformance ledger states that it has typed signaling-room messages but no signaling wire serde; a test-only JSON parser would test the runner instead of production behavior.
- C# remains staged because its signaling plane is not implemented; `#lzcsplane` and the parity ledger already own that work. No C# files or capability claims were changed.

Changed paths:

- `lazily-spec`: `conformance/signaling/{frames,anti_spoof_session}.json`, `tests/test_schema_conformance.py`
- `lazily-{py,js,kt,go,dart,zig}`: production signaling decoder plus canonical conformance runner in each binding
- `lazily-rs`: `src/signaling_client.rs`, new `tests/signaling_negative_conformance.rs`, and `signaling/{src,test,tsconfig}` for the TypeScript surface
- Supervisor: only the eight participating submodule pointers

Verification:

- `make check` passed in lazily-spec, py, js, kt, go, dart, zig, and rs.
- Rust additionally passed `npm --prefix signaling run check`: 32/32 TypeScript signaling tests; the Rust feature replay ran 2/2 new canonical negative tests and the runtime manifest covered 119/124 canonical fixtures.
- Spec: 148/148 tests plus Lean build and coverage sync check.
- Python: 551/551 tests; Dart: 541/541; Kotlin, Go, JavaScript, and Zig full gates and conformance coverage passed.
- Zig printed its already-documented AsyncContext `dispose_signal_reverts_to_lazy` divergence from `EXPECTED_DIVERGENCES`; its signaling decoder/replay and overall gate completed successfully.
- CI checked exactly once after push: lazily-spec completed green; js/kt/dart/zig/rs were queued and go was in progress. lazily-py has no workflow named `CI`.

Published commits:

- spec `17df13b`; py `730edea`; js `0281eab`; kt `66bc92a`; go `945d78c`; dart `605228a`; zig `6bdae44`; rs `10362af`
- supervisor pointer commit `2bad00f9e`

Confidence: high. The rejects are canonical shared bytes and every admitted binding exercises its production decoder. Escalation: none for this feature. The workspace-root `make check` remains separately red in the pre-existing `src/agent-doc` worktree on missing `delivery_version` fields at `agent-doc-preflight-io/src/lib.rs:5044` and `:5346`; it was not modified or represented as part of this signaling result.

### Re: #lazilyzigport — gpt-5

> **Queue prompt:**
>
> do [#lazilyzigport]


Ported Dart’s fail-closed manifest-ID validation to the Go, Rust, and Zig runtime conformance guards.

Each guard now resolves every distinct recorded ID against the canonical corpus and fails with an explicit recorder-corruption diagnostic when an ID names no file. This happens independently of the ordinary “fixture was not opened” comparison, so a severed or interleaved manifest line can no longer masquerade as upstream coverage drift.

The port exposed and fixed one real Zig recorder defect: `specReadFile` recorded the requested path before the read succeeded. Zig’s intentional missing-corpus loader test therefore wrote `definitely-not-a-corpus-directory/nope.json` into the real manifest. Recording now occurs only after bytes are successfully opened, so failed probes cannot manufacture evidence.

Changed paths:

- `lazily-go/scripts/check-conformance-coverage.sh`
- `lazily-rs/scripts/check-conformance-coverage.sh`
- `lazily-zig/scripts/check-conformance-coverage.sh`
- `lazily-zig/src/lazily/conformance_manifest.zig`

Rust’s two signaling fixtures were also removed from `KNOWN_UNCOVERED`: the preceding cycle now genuinely opens both, so retaining those excuses would make the ledger overstate the remaining gap.

Verification:

- Corrupt-copy probes for all three bindings appended `definitely-not-a-corpus-directory/nope.json`; every guard exited nonzero and emitted `manifest records ... which names no file`.
- `make check` passed in lazily-go, lazily-rs, and lazily-zig after the fixes.
- Clean coverage results: Go 109/124 opened (15 known-uncovered), Rust 119/124 (5 known-uncovered), Zig 94/124 (30 known-uncovered).
- Rust’s full matrix, Loom model, Lean builds, signaling feature tests, and conformance guard passed; its benchmark gate again reported the existing no-evidence skip because `target/criterion` is absent.
- Zig’s existing documented AsyncContext divergence remains ledgered; the full gate and the newly clean manifest passed.
- CI checked once after push: Go and Zig were in progress; Rust was queued.

Published commits: Go `f40f2b6`, Rust `2f61c63`, Zig `81070f7`; supervisor pointer commit `93d8dd08c`.

Confidence: high. Both valid-manifest acceptance and corrupt-manifest rejection were exercised in every target binding, and Zig’s newly exposed false record was fixed at the recorder seam. Escalation: none for these repositories. The workspace-root `make check` remains separately red on the pre-existing `src/agent-doc` `delivery_version` compile errors and was not represented as part of this result.

### Re: #lazilyconformancecoverag — gpt-5

> **Queue prompt:**
>
> do [#lazilyconformancecoverag]


Kept this item open and explicitly staged, per the operator’s rule that incomplete bindings do not enter a feature’s peer/test group.

The prior 37/124 measurement was stale. A fresh runtime-manifest gate reports C# opens 42/124 canonical fixtures and lists 82 as known-uncovered. The opened corpus is limited to reactive-graph 21, collections 10, statechart 7, materialization 3, and familysync 1. The remaining distributed, IPC, reliable-sync, signaling, and other planes are real missing product surfaces, not runner-wiring gaps.

No implementation or coverage claim was added: doing so at this umbrella item would either duplicate the phase backlog or manufacture conformance against unsupported types. C# remains excluded from feature groups until the corresponding `#lzcs*` phase ships its production surface and replay, at which point the capability-driven suite can admit it.

Changed paths: none. Existing dirty `AGENTS.md`, `README.md`, and `build/` in lazily-cs were preserved.

Verification: `make check` passed — build clean, 77/77 tests passed, and runtime coverage reported 42/124 opened with 82 explicitly known-uncovered.

Confidence: high. The staging decision is backed by the runtime manifest and the phase plan, not static source mentions. Escalation: none; this is deliberate product staging, and the item stays actionable through the existing `#lzcscollections`, `#lzcsqueues`, `#lzcscrdt`, `#lzcsipc`, `#lzcssync`, `#lzcstemporal`, `#lzcscluster`, and `#lzcsplane` phases.

### Re: Response

Implemented and shipped `#lazilybenchmarkregressio` in lazily-rs.

The scheduled `Regressions` workflow now has a `benchmark-budgets` job on pinned `ubuntu-24.04`: it generates fresh Criterion evidence with `cargo +stable bench --locked --features "instrumentation,async,tokio,thread-safe"`, then runs `make benchmark-check-strict` with a 60-minute timeout.

The strict target now requires evidence, refreshes the deterministic instrumentation profile, and uses a new `--budgets-only` mode. That mode enforces required p50/p95 row presence and deterministic lock-acquisition budgets, but deliberately does not compare machine-dependent wall-clock values in `BENCHMARKS.md`. The ordinary `benchmark-check` still owns generated-report freshness. Spec-compliance coverage asserts the workflow command, pinned runner, strict target flags, and scheduled gate wiring.

Verification passed:
- workflow YAML parse
- focused `benchmark_report_harness` test
- full fresh `cargo bench --locked --features "instrumentation,async,tokio,thread-safe"`
- `make benchmark-check-strict` against the fresh evidence
- full `make check` on the normal clean-checkout evidence path
- `git diff --check`

Pushed lazily-rs commit `d5d3b47` (`ci: enforce benchmark regression budgets`). The latest CI run was queried exactly once and was `in_progress` (run `30225781945`). Committed the root submodule pointer as `b52d6207b` (`lazily: enforce benchmark regression budgets`).

### Re: Response

Implemented and shipped `#lzqfjs` in lazily-js.

The defect was real across `QueueCell`, `TopicCell`, and `WorkQueueCell`: their fixture runners trusted returned `invalidates` booleans, but the objects owned no context, minted no graph nodes, and could not register a dependency edge.

Changed paths:
- `src/reactive.js` / `src/reactive.d.ts`: added `Context.clearComputeds(handles)`, which force-invalidates dependency-free derived roots together in one frontier walk and schedules downstream effects through the existing batch semantics.
- `src/queue.js` / `src/queue.d.ts`: all three queue-family constructors now require a `Context`; Queue and WorkQueue mint guarded computed reader kinds, Topic mints per-subscriber pass-through computed readers, and reads accept the value-threaded `Compute` view so they actually subscribe. Successful mutations clear exactly the reader handles named by the canonical invalidation matrix.
- `test/queue.test.js`, `test/topic.test.js`, `test/work-queue.test.js`: canonical fixtures now prime dependent computeds and assert recomputation deltas for every declared invalidation, rather than accepting runner bookkeeping as proof. Queue MPSC fixture steps also execute inside `Context.batch`.

Verification:
- targeted queue/topic/work-queue/ledger suite: 25/25 passed
- full `make check`: passed
- scoped `git diff --check`: passed

One runtime test exposed and fixed the Topic empty-stream edge case: connect/disconnect can be contractually invalidating while the stream value remains `[]`, so Topic readers use `computedRippleWhen(..., () => true)` to propagate the cursor/connection transition instead of equality-suppressing it.

Pushed lazily-js commit `186c8c9` (`fix: make queue family reader kinds reactive`). Latest CI was queried exactly once and was `queued` (run `30226088533`). Committed the root submodule pointer as `2105c1842` (`lazily: make JS queue readers reactive`). Pre-existing lazily-js `README.md` and untracked `build/` changes were preserved and not staged.

Confidence: high. No escalation was required.

### Re: Response

Completed `#lzqfports` for the five admitted target bindings. The canonical corpus is 11 fixtures (Queue 5, Topic 4, WorkQueue 2), and each binding now replays every fixture across sync, thread-safe, and async flavors with an enforced 9-pair capability ledger, exact `expected.invalidates` checks, real state/return assertions, concurrency or atomicity probes, and mutation-discriminating validation.

Binding revisions:
- C++ `be4b5ac8c31ec0c6791cc99154d5b435e91c32f0`; `make check` green; CI `30226730599` queued.
- Dart `39d6e862728eaa5ced9e791b32ab9f59bc10dedc`; `make check` green; CI `30226694786` queued.
- Python `b2f5f449bdc793471ebad02e2683311af2ca7a98`; `make check` green (583 tests, coverage 120/124); no named CI workflow was present.
- Go `4c8f11081b50b1a92f6df8fdd181ddc3b82006c1`; focused race tests and `make check` green; CI `30227440955` queued.
- Kotlin `86b047d9a0417e23ef3a67b6aee1d4bbbde96c6a`; focused and full `make check` green; CI `30227524633` queued.

The root repository records exactly those five submodule revisions in `363452b0d` (`lazily: port queue family flavors`). All five target worktrees are clean except Python's preserved pre-existing untracked `.claude/worktrees` content.

C# was intentionally not admitted to this peer group. It remains explicitly staged under `#lzcsqueues` until its queue-family implementations and the same admission gates exist; no unsupported C# behavior is presented as conformance.

Confidence: high. The implementations, canonical replay, exact invalidation behavior, concurrency/atomicity probes, mutation checks, repository gates, commits, pushes, and one-time CI queries were all verified. No escalation is required for this item.

### Re: Response

Completed `#lzqfzig` as the graph-reactivity prerequisite, without prematurely admitting Zig's thread-safe or async flavors.

Zig revision `90c0241a5cf81aed8029f100de5ddfb8433cb5c7` (`queue: bind sync readers to graph`) is pushed to `origin/main`; the root records it in `806d70eef` (`lazily: graph-back Zig queue readers`). Changed paths are `README.md`, `src/lazily/context.zig`, `src/lazily/cell.zig`, new `src/lazily/reader_kind.zig`, `src/lazily/queue.zig`, and `src/lazily/work_queue.zig`.

The previous plain polling counters are replaced by runtime-minted graph node identities. QueueCell exposes demand-derived `head`/`len`/`is_empty`/`is_full`/`closed` handles; TopicCell gives every subscriber its own graph-backed reader; WorkQueueCell exposes independent pending/empty/in-flight/dead-letter handles. `Compute.get(...)` now registers real dependency edges, while direct handle `.get()` is explicitly untracked. Multi-root changes publish under one graph lock and eager dependents drain only after the complete changed-kind set is visible. Runtime cache keys are monotonic per Context so multiple collection instances cannot alias merely because they share a comptime initializer.

Verification: `zig build test` and final `make check` passed. Dedicated graph tests prove downstream computed nodes rerun for Queue, Topic, and WorkQueue changes and remain warm for independent reader kinds. Mutation checks deliberately removed Queue `len`, Topic publish, and WorkQueue pending invalidation; the respective conformance/graph gates failed, then each mutation was restored and the baseline gate passed. Latest CI was queried once after push and is queued as run `30228405960`.

The canonical coverage matrix remains partial for Zig's three synchronous queue-family rows because this item removes the false-reactivity prerequisite only; it does not yet ship the all-flavor 11-fixture runner. Thread-safe and async Zig remain staged for their later flavor item, and C# remains staged under `#lzcsqueues`. The pre-existing uncommitted `AGENTS.md` change in lazily-zig was preserved and excluded from the commit.

Confidence: high. The implementation, graph-edge behavior, independence, atomic changed-kind publication, mutation discrimination, repository gate, commit, push, and one-time CI lookup were verified. No escalation is required.

### Re: Response

Completed `#maphandlecleardependents`.

The Rust remainder was stale rather than actionable: `MapHandle`, `ThreadSafeMapHandle`, and `AsyncMapHandle` already define `clear_dependents`; source/computed implementations dispose the appropriate graph node; and every flavor's `remove` invokes the hook after releasing collection state. `cargo test --test collections_family_conformance` passes, so no Rust edit was made.

Python now tears down removed map handles on every supported flavor and entry kind:

- sync/thread-safe source entries call terminal `Cell.dispose()`;
- sync/thread-safe computed entries call terminal `Slot.dispose(ctx)`;
- async source entries call terminal `Cell.dispose()`;
- legacy `AsyncSlot` entries call `hard_clear()`, matching that non-graph handle's supported lifecycle by dropping the resolved cache and invalidating any in-flight revision.

Held-handle tests cover all six flavor/kind combinations. Mutation proof removed the three teardown calls, made all six tests fail, restored the calls, and returned the tests to green.

Verification:

- lazily-rs: `cargo test --test collections_family_conformance` — pass.
- lazily-py: focused three-family suite — 47 passed.
- lazily-py: six removal tests after restoration — pass.
- lazily-py: `make check` — pass.
- Workspace `make check` reached an unrelated `corky-core` OAuth-loopback test race: four parallel tests failed binding the same fixed port with `AddrInUse`. The isolated group passes with `--test-threads=1`; no Corky files were touched.
- Python CI query: repository has no workflow named `CI`.

Durable commits:

- lazily-py `2df765f` (`reactive-map: dispose removed entry handles`), pushed to `origin/main`.
- workspace pointer `c56438eaf` (`lazily-py: dispose removed map handles`).

The pre-existing untracked `.claude/worktrees/agent-af5350b120eaf83f3` was preserved. C# remains staged according to the operator rule; this item does not broaden its peer-suite admission.

### Re: Response

Completed `#lazilycellfamily`.

Corrected the false grow-only invariant on `present_keys` in both affected Rust map flavors:

- `src/cell_family.rs`
- `src/thread_safe_reactive_family.rs`

The docs now state the precise contract: materialization only grows the present set, while explicit `remove` shrinks it. The async flavor already used neutral wording and needed no edit.

Verification:

- `git diff --check` — pass.
- Rust `cargo fmt --all --check`, clippy, build, tests, and all doctests — pass through the repository `make check` run.
- Aggregate `make check` exits only at `benchmark-check`: `BENCHMARKS.md benchmark results are stale`. This is unrelated to the documentation-only change, and the benchmark file was preserved.
- Latest GitHub Actions CI run `30229190987` is queued; queried once as required.

Durable commits:

- lazily-rs `707533b` (`docs: scope reactive map growth invariant`), pushed to `origin/main`.
- workspace pointer `0f5aa0bab` (`lazily-rs: clarify map presence docs`).

Confidence: high. Escalation: none.

### Re: Response

Resolved `#lazilyspecdisagrees` as already completed in the current repositories; no new edit was warranted.

The spec now follows the operator's capability rule consistently:

- `coverage.json` scores Dart's `ThreadSafeComputedMap`, thread-safe context, and thread-safe ReactiveMap Core rows as shipped.
- Generated `docs/coverage.md` carries the same shipped marks.
- Note ᵈ explicitly withdraws the former N/A carve-out: Dart and JS ship and conform, so they are admitted today. The separate design question—whether those layers should exist on single-threaded runtimes—remains distinct from status scoring.
- This resolution entered lazily-spec in existing commit `63e07b4`.

Implementation evidence in lazily-dart:

- `lib/src/thread_safe_reactive_family.dart` defines the run-to-completion flavor, documents the single-isolate premise, uses a reentrant guard, graph-backed membership/order signals, and the SourceMap/ComputedMap specializations.
- `test/thread_safe_reactive_family_test.dart` proves eager/lazy behavior, transparency, present-set behavior, confluence, and source writes.
- `dart test test/thread_safe_reactive_family_test.dart` — pass.

This is exactly the staged-capability policy: a shipped, tested implementation joins the feature group; runtime philosophy alone does not stage it. No files changed, no commit or CI query was needed.

### Re: Response

Continuing. `#lazilyspecdisagrees` is already resolved by the current spec and Dart implementation: the shipped thread-safe flavor remains admitted, and its focused conformance test passed. Advancing to the next queued item.

### Re: Response

`#lzjsnpmtrustedpub` is correctly an operator gate, not a repository change.

- Changed paths: none. Existing local `README.md` and `build/` changes were left untouched.
- Repository proof: `.github/workflows/release.yml` already uses a GitHub-hosted runner, `id-token: write`, Node 24, the public registry, and `npm publish --provenance --access public`. `package.json` identifies `@lazily-hub/lazily-js` and the exact `https://github.com/lazily-hub/lazily-js.git` repository.
- Runtime proof: `npm view` reports published version/latest `0.29.1`. Release run `30215783298` passed checkout, install, and tests, signed provenance, then npm rejected the PUT with the permission-masked E404. The local npm login is valid (`btakita`), but even `npm trust list` requires one-time browser authentication, so the account change cannot be completed unattended.
- Exact unblock: authenticate the npm CLI/browser challenge, then create the GitHub Actions trust with organization `lazily-hub`, repository `lazily-js`, workflow filename `release.yml`, no environment, and allowed action `npm publish` (equivalently: `npm trust github @lazily-hub/lazily-js --file release.yml --repo lazily-hub/lazily-js --allow-publish -y`). npm’s official trusted-publisher documentation confirms those fields and that they are case-sensitive.
- Verification after unblock: `npm trust list @lazily-hub/lazily-js`, then publish the next version tag; do not retry the already manually published `0.29.1` as proof.
- Confidence: high. Escalation: npm account owner/browser authentication required.

### Re: Response

`#lzcsqueues` remains open and staged under the operator’s capability rule.

- Changed paths: no product files. Existing local `AGENTS.md`, `README.md`, and `build/` changes were preserved.
- Measured support: current lazily-cs HEAD `01d2cf3` ships `QueueCell` and replays all five canonical `queuecell_*.json` fixtures. Focused `QueueCellConformanceTests` passed.
- Still unsupported: `QueueStorage`, `TopicCell`, `WorkQueueCell`, `RelayCell`, `BackpressurePolicy`, `SpillStore`, and `Transport`; the C# conformance ledger explicitly names the topic/work-queue fixtures as unsupported.
- Suite admission: C# may advertise only the narrow QueueCell capability if the network suite defines that exact feature. It must remain absent from topic, competing-work-queue, relay/backpressure, persistence, and transport peer groups until each production surface and canonical replay ships.
- Confidence: high. Escalation: none; this is intentional feature staging, not a blocker to supported peers.

### Re: Response

Staged `#lzcsqueues` as requested. The item remains tracked with its measured partial support and exact unstage condition; the active queue will now skip it. Reopen it only when implementing the remaining C# queue-family surfaces, and admit C# per feature only after the corresponding canonical replay is green.

### Re: Response

Applied the operator’s staged-peer policy across the remaining incomplete C# phases.

- Staged: CRDT, IPC/schema, full-duplex sync, temporal/windowing, cluster/presence/resilience, control-plane/FFI, packaging, the aggregate C# coverage item, and the C# network-peer admission item.
- Each item remains tracked with its production-surface and canonical-replay unblock condition; none is marked complete.
- C# therefore stays visible to the suite registry but is excluded from every unsupported feature/channel/codec/variant group. Supported C# features can be admitted independently as their production implementation and conformance replay land.
- Changed paths: session tracking only. Verification: current C# coverage ledger and parity plan enumerate the missing surfaces; focused QueueCell replay already passed for the one measured partial slice. Confidence: high. Escalation: NuGet packaging retains its explicit operator-key prerequisite.

### Re: Response

Created and committed the staged implementation plan for `#asyncplanenever`: `tasks/plan-lazily-async-v2-migration.md` (`a9ce770a5`, pushed).

- Corrected the stale baseline: Rust, JavaScript, Kotlin, and C# already expose canonical `AsyncSource`/`AsyncComputed`; the remaining handle migration is C++, Go, Python, and Zig.
- Separated the formal `AsyncSlotState` theorem/module name from the public handle rename so the formal model is not mechanically renamed by accident.
- Defined per-binding compatibility aliases, canonical factory migration, `memo` collapse into guarded computed, proof tests, commit sequencing, and a suite rule that API naming never implies transport/CRDT/sync capability.
- Verification: root `make check` passed; `git diff --check` passed. Latest CI query was attempted once; this repository has no workflow named `CI`.
- Changed path: `tasks/plan-lazily-async-v2-migration.md`. Confidence: high. Escalation: none.

The implementation item remains staged and tracked; unstage it when executing the four remaining binding phases.

### Re: Response

Verified `#lazilyrenameinverted` and staged it as an ownership decision.

- Evidence: `cell.py` deliberately defines the mypyc-native `Cell`/`CellSlot` classes and binds `Source = Cell`, `SourceSlot = CellSlot`; the docstring explicitly preserves native-struct reads and `isinstance` identity. The compiled reactive core spans `slot`/`cell`/`signal`/`effect`/`batch`, and multiple production paths dispatch on `isinstance(..., Cell)`.
- Recommendation: KEEP `Cell`/`CellSlot` as the native implementation class names and treat `Source`/`SourceSlot` as the canonical public handle aliases. This preserves runtime identity while presenting v2 vocabulary to callers.
- Do not flip the class declaration merely for textual consistency; that is a breaking runtime/type-identity change with no semantic gain.
- Changed paths: none. Verification: source and mypyc configuration audit. Confidence: high. Escalation: operator/spec-owner yes/no decision required before changing this policy.
<!-- agent:boundary:cb9636af -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzjsnpmtrustedpub]~~
- ~~do [#lzcsqueues]~~
- ~~do [#lzcscrdt]~~
- ~~do [#lzcsipc]~~
- ~~do [#lzcssync]~~
- ~~do [#lzcstemporal]~~
- ~~do [#lzcscluster]~~
- ~~do [#lzcsplane]~~
- ~~do [#lzcspackage]~~
- ~~do [#asyncplanenever]~~
- ~~do [#lazilyrenameinverted]~~
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
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
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
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts. RECOMMENDATION 2026-07-26 (agent): KEEP *Cell. The kernel renamed the two value-node KINDS (Source/Computed) and kept Cell as the concept for a value-bearing reactive node. A QueueCell is not a kind — it composes them (five Source<u64> reader-version cells plus derived readers), so QueueSource would imply "written from outside, folds under a merge policy", which is false for a queue. Same for TimerCell/LeaseCell/CircuitBreakerCell/PresenceCell: each is a composite of a pure core plus projecting cells. KEPT OPEN: naming must converge with the operator before editing (9 bindings in lockstep + deprecation surface on every public type). Unblocks on a yes/no. If YES, scope is: spec cell-model.md first, then 9 bindings in lockstep with deprecated aliases, plus a decision on whether MergeCell collapses entirely (under the kernel it is already Source<T,M> with a non-default policy)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lazilyrenameinverted] OPERATOR DECISION — recommendation: KEEP lazily-py `Cell`/`CellSlot` as the mypyc-native implementation class names and keep `Source = Cell` / `SourceSlot = CellSlot` as the canonical public aliases. Flipping changes native-struct and `isinstance` identity across the compiled core. Resolve by accepting this documented exception, or explicitly authorizing a breaking native-class rename with mypyc and identity migration tests.
- [/] [#asyncplanenever] STAGED implementation — finish the async v2 kernel naming migration in C++, Go, Python, and Zig per tasks/plan-lazily-async-v2-migration.md. Rust, JavaScript, Kotlin, and C# already expose AsyncSource/AsyncComputed. Preserve deprecated aliases in published bindings, collapse live memo constructors into guarded computed, and settle the public AsyncComputedState vs formal AsyncSlotState distinction without renaming the formal model mechanically.
- [/] [#lazilyconformancecoverag] lazily-cs: STAGED per operator — incomplete C# surfaces stay out of peer/feature test groups until implemented. Fresh runtime measurement 2026-07-26: 42/124 canonical fixtures opened, 82 known-uncovered (reactive-graph 21, collections 10, statechart 7, materialization 3, familysync 1 are covered). The remaining fixtures are genuinely missing product surfaces, not runner-wiring gaps. Keep open and work it down only by shipping the explicit tasks in tasks/software/lazily-cs-parity-plan.md (#lzcscollections, #lzcsqueues, #lzcscrdt, #lzcsipc, #lzcssync, #lzcstemporal, #lzcscluster, #lzcsplane); admit C# to each capability group only when that production surface and canonical replay ship.
- [/] [#lzinteropcs] Keep lazily-cs visible but registry-staged in the single network suite. After #lzcsipc and #lzcssync ship real schema-validated IPC/CRDT and full-duplex sync surfaces, implement its production interop peer, advertise only the features/codecs/channels/variants it actually supports, then let the capability-driven matrix admit it automatically; stays open until both prerequisites and the C# peer are complete.
- [/] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
- [/] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [/] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [/] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [/] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [/] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [/] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [/] [#lzcsqueues] lazily-cs: PARTIAL/STAGED — QueueCell shipped at 01d2cf3 and replays all five canonical queuecell fixtures; keep C# eligible only for an exact QueueCell capability. Still implement QueueStorage, TopicCell, WorkQueueCell, and RelayCell with BackpressurePolicy/SpillStore/Transport, then replay their canonical corpora before admitting C# to those peer/feature groups — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16).
- [/] [#lzjsnpmtrustedpub] lazily-js: OPERATOR GATE — authenticate the npm CLI/browser challenge and register @lazily-hub/lazily-js as a GitHub Actions trusted publisher with organization lazily-hub, repository lazily-js, workflow release.yml, no environment, and allowed action npm publish; verify with `npm trust list @lazily-hub/lazily-js`, then prove on the next version tag. Repository workflow/OIDC/provenance is already correct; release run 30215783298 passes tests and signs provenance before npm rejects the upload with permission-masked E404.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
