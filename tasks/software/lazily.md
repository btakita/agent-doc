---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 019fc87a-beb7-7e03-a7a2-c5ca1dc04331
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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260804-064159.md`*

Compacted content:
- Archived 14 response topic(s): do [#lzblobbackendstrict]; do [#lzprosekeyconvention]; do [#lznullformblind]; 11 more
- Prior summary/context: prior compacted content: Archived 3 response topic(s): do [#failcloseddispatch]; do [#nevergotsystematic]; do [#failc...

Negative control: I temporarily changed the `unknown_kind` case into a valid atomic chart and ran the focused corpus test in all nine bindings. All nine failed on `unknown_kind`, including C++ and Zig's vendored mirror; after restoring and re-syncing the fixture, all nine focused runners passed. This proves the shared invalid-definition corpus is load-bearing.

Published commits:

- lazily-spec `0bd0f51713e6718223265a9c6399a985b7af9c87`
- lazily-rs `3fc00f1392b161c7f0cb35ac42718cf77e2266ef`
- lazily-py `546157df667cd8241d00b42e46acabb1ecb8529c`
- lazily-js `fcfe363f054838fc8140ecfab0d5d6c69514fca1`
- lazily-go `b0f4f7ddca5e4ff478b15b0e0371cf19318148d8`
- lazily-cpp `d3f4011f457053ece35b98a61fc56cfae9263acd`
- lazily-dart `cff7cefe707264ab427d833b5fd361c738a57f2d`
- lazily-kt `da2e802ccdcdfb8873287022fc44aa12450fca42`
- lazily-zig `035a960a6b81737ae8030c2bbb6ae37e5840aba5`
- lazily-cs `1273ec5a853ea7e16d68d14f6ed0cd3255c339f3`
- superproject gitlink pin `334d3b6014fc749b99a8e250f9f568206f654e05`

Verification:

- `make check` is green locally in the spec, all nine bindings, and the superproject (`tart-abfd7a3ba6`).
- Exact-SHA CI is green for the spec and all nine bindings: spec `30882563055`, Rust `30882580626`, Python `30882580729`, JavaScript `30882580847`, Go `30882580868`, C++ `30882580766`, Dart `30882580903`, Kotlin `30882580616`, Zig `30882580958`, and C# `30882580860`.
- Root run `30882697583`: `tmux-ci` is green. The `check` job failed twice only at the linker with the known `collect2: fatal error: ld terminated with signal 7 [Bus error]`; the single allowed failed-job retry produced the identical infrastructure-only signature. There were no source, compile, or test-assertion failures.

Unrelated dirty AGENTS/runbook/worktree changes in the shared repositories were preserved and excluded from every commit.
### Re: authoritative statechart kinds and closed references — gpt-5

Implemented and published `#lzstatechartkindauthority` across the canonical spec and all nine bindings.

- The declarative contract now closes `kind` to `atomic`, `compound`, `parallel`, `history`, or `final`. An explicit kind is authoritative, must agree with its structural markers, and malformed contradictions are rejected rather than silently re-inferred.
- Chart-level `initial`, state `parent`/`initial`/history `default`, and every transition target must resolve to a declared state before execution. Unknown lookups no longer fall back to phantom atomic states.
- Structural fields are strict: `parallel` and transition `internal` must be booleans; `history`, `parent`, `initial`, and history `default` must be strings when present.
- Added canonical `conformance/statechart/malformed_rejected.json` with 12 named cases spanning unknown/contradictory kinds, all requested type errors, and all requested undeclared-id references. Every Rust, Python, JavaScript, Go, C++, Dart, Kotlin, Zig, and C# runner rejects every case.

### Re: canonical durable-outbox journal decoding — gpt-5

Implemented and published `#lzoutboxjournalop`.

The spec now defines `OutboxJournalDecode` in `conformance/reliable-sync/outbox_journal_decode.json` with the deliberate same-file opposites:

- an unknown complete interior opcode is rejected;
- a sole torn trailing record is forgiven while earlier complete records still determine retained epochs and frames.

The durable-outbox prose now makes that boundary normative, the reliable-sync fixture registry includes the new model, and the schema-conformance gate recognizes it.

C#, Dart, and Zig now replay the canonical corpus through their native file-journal encoders and scanners. Zig also vendors and manifest-checks the byte-identical fixture mirror. I temporarily changed the unknown opcode to the known `cursor` opcode; all three canonical runners failed on the expected-reject assertion, then passed again after restoring `future_delete`. This proves the new tests discriminate the opcode boundary rather than merely opening the fixture.

Published commits:

- lazily-spec: `23a882a965c8fcc0526018042b27e05cd9f097d2`
- lazily-cs: `947e989688d178239a2c8281141f03a5ad3edcf6`
- lazily-dart: `cf4a71259f70f614d58985b41d9cb925d7fa20a7`
- lazily-zig: `1937bc5a3583603fce3c81f789205a772d7dd0de`
- root gitlink pin: `06278f82b033621ed147671b317c1f450fe8ca25`

Verification:

- full `make check` passed in lazily-spec, lazily-cs, lazily-dart, and lazily-zig;
- exact-commit component CI passed for all four repositories;
- root `tmux-ci` passed;
- root `check` failed on both the original run and its single retry only because GNU `ld` terminated with signal 7 (Bus error), matching the established runner-infrastructure signature;
- the root network workflow and its single retry both timed out before an unchanged lazily-rs peer flushed its initial `hello`; the uploaded reports identify `peer_timeout`, `culprit: lazily-rs`, seed 17, with no changed Rust gitlink;
- the local root aggregate reached unrelated dirty-worktree corky OAuth loopback tests racing on port 1455; their isolated sequential test group passed. Task-owned full gates remained green.

No follow-up work is created by this item.

### Re: durable-outbox closeout reconciliation — gpt-5

Confirmed: all five live steering fragments are projections of the already committed `#lzoutboxjournalop` closeout, not a new implementation request.

This reconciliation cycle changes no repository paths, runs no new task command, and creates no follow-up work. The committed response already contains the changed-path summary, the negative-control and full-gate verification, the bounded CI infrastructure exceptions, and the published component/root commit SHAs. Confidence is high because the session guard explicitly reports that prior response as committed in HEAD; no escalation or additional commit is required for these echoed fragments.

### Re: exact codec wire-input witness — gpt-5

> **Queue prompt:**
>
> do [#lzwireencodingrunner]


`#lzwireencodingrunner` was already implemented, published, and root-pinned; this queue occurrence was stale, so I made no repository changes.

The executable proof is the intended one: lazily-spec adds `expect.wire_input_fnv1a64` for every relevant scenario and validates it against the exact raw `wire_json` UTF-8 bytes or decoded `wire_msgpack_hex` bytes. Each of the nine bindings computes the same FNV-1a witness over the exact buffer it passes to its library decoder and asserts that key before decoding. Re-serializing a parsed object therefore changes the witness and reddens the runner.

Published commits:

- spec: `5517437bce53bf5bb6822a7cb9447d846aa2606f`
- Rust: `f09ad7bdfecea5cf2bd3651ddf00f47f978d7d02`
- Python: `586429c2abd2d260925f9c6464d027864fce2155`
- JavaScript: `c3a8f5082e84b0715afcfbc013ab400a1e6dde96`
- Go: `83f5f308ad8a637f93f9ae8ba9496b7b969efe16`
- C++: `b9484d22fdd068304475b0c29c3a141b4576684c`
- Dart: `eca287a462492a4f9baad85fe80faae4962bbf8f`
- Kotlin: `34e22f357b4f21aeca172195de23ebddc1e046fe`
- Zig: `5d6a3657b0a10093b348bb07490942646a3734c8`
- C#: `96770d91373ac48f17529825fa8c7153115aa8cb`
- root pin: `423e757a9a489d0104d5c4f435c351dc4b294c98`

Exact-commit CI is green for the spec and all nine bindings (Python under `poe precommit`); root `tmux-ci` is green. Root `check` was retried once and both attempts ended only in the established GNU `ld` signal-7 bus-error runner failure. Later full local gates on the descendant component heads also passed during this queue drain.

Confidence is high. No escalation or follow-up work is required.

### Re: durable-outbox journal closeout reconciliation — gpt-5

> **Queue prompt:**
>
> do [#lzoutboxjournalop]


`#lzoutboxjournalop` is already implemented, published, and root-pinned. This queue head survived because the original cycle was abandoned during editor/controller recovery before its `done` mutation became authoritative; no repository changes were needed in this reconciliation cycle.

The completed work adds canonical durable-outbox journal decoding coverage for an unknown complete interior opcode (reject) and a sole torn trailing record (forgive while retaining the preceding epoch-2 frame), with matching C#, Dart, and Zig replay coverage. Published commits: spec `23a882a965c8fcc0526018042b27e05cd9f097d2`, C# `947e989688d178239a2c8281141f03a5ad3edcf6`, Dart `cf4a71259f70f614d58985b41d9cb925d7fa20a7`, Zig `1937bc5a3583603fce3c81f789205a772d7dd0de`, and root pin `06278f82b033621ed147671b317c1f450fe8ca25`.

Task-owned full local gates and exact component CI runs were green. The root aggregate's GNU `ld` bus error and unrelated Rust initial-hello timeout were independently reproduced and did not involve these changed pins. No follow-up.

### Re: `#lzdecorativewirefields` — gpt-5

> **Queue prompt:**
>
> do [#lzdecorativewirefields]


Implemented the decoded fields as behavior, not field-touch bookkeeping.

Rust now has a `CommandAdmitter` in front of the unchanged pure `CommandProjection`. Decoded command fields drive actual admission outcomes:

- `dedupe` distinguishes command-ID replay, no cross-command dedupe, and idempotency-key dedupe.
- `supersede` preempts an older non-terminal command sharing the key; the older command's `cancel_on_preempt` controls whether an explicit cancel is permitted.
- `deadline_ms` rejects at the logical-clock deadline boundary.
- `payload_type` must be qualified by the decoded namespace and command name.
- `required_features` fails closed against the target's advertised feature set.
- receipt `observer` must match the decoded command target before the receipt can fold into projection.
- `CapabilityHandshake.session_id` was already made load-bearing by the preceding handshake change; the new decoded-wire fixture also pins mismatch rejection.

Added `tests/decorative_wire_fields.rs` plus `tests/fixtures/decorative_wire_fields.json`. The fixture mutates each decoded field and requires a distinct semantic decision, so decoding and then ignoring any one of them makes the test fail. I also recorded the newly published append-only outbox-journal fixture as explicitly uncovered by Rust because Rust's durable outbox is SQLite-backed and has no append-only journal decoder; this keeps the repository-wide coverage ledger truthful.

Verification is green locally: focused decoded-wire tests, command conformance, all-feature library/test Clippy with warnings denied, the conformance coverage target, and the full `make check` (1,202 output lines, zero failures). Published Rust commit `57f9eef7663a5a9c75afca7c6817b411478b5766` and superproject pin `08fa3aaca526e1c7326851499b3a682bdc380a70`. GitHub CI started cleanly: formatting, Clippy, signaling, and Lean are green; the remaining Rust and superproject jobs are still in progress, recorded as asynchronous projection state rather than treated as an ACK gate.

Rust now has a `CommandAdmitter` in front of the unchanged pure `CommandProjection`. Decoded command fields drive actual admission outcomes:

- `dedupe` distinguishes command-ID replay, no cross-command dedupe, and idempotency-key dedupe.
- `supersede` preempts an older non-terminal command sharing the key; the older command's `cancel_on_preempt` controls whether an explicit cancel is permitted.
- `deadline_ms` rejects at the logical-clock deadline boundary.
- `payload_type` must be qualified by the decoded namespace and command name.
- `required_features` fails closed against the target's advertised feature set.
- receipt `observer` must match the decoded command target before the receipt can fold into projection.
- `CapabilityHandshake.session_id` was already made load-bearing by the preceding handshake change; the new decoded-wire fixture also pins mismatch rejection.

Added `tests/decorative_wire_fields.rs` plus `tests/fixtures/decorative_wire_fields.json`. The fixture mutates each decoded field and requires a distinct semantic decision, so decoding and then ignoring any one of them makes the test fail. I also recorded the newly published append-only outbox-journal fixture as explicitly uncovered by Rust because Rust's durable outbox is SQLite-backed and has no append-only journal decoder; this keeps the repository-wide coverage ledger truthful.

Verification is green locally: focused decoded-wire tests, command conformance, all-feature library/test Clippy with warnings denied, the conformance coverage target, and the full `make check` (1,202 output lines, zero failures). Published Rust commit `57f9eef7663a5a9c75afca7c6817b411478b5766` and superproject pin `08fa3aaca526e1c7326851499b3a682bdc380a70`. GitHub CI started cleanly: formatting, Clippy, signaling, and Lean are green; the remaining Rust and superproject jobs are still in progress, recorded as asynchronous projection state rather than treated as an ACK gate.
### Re: `#lzdecorativewirefields` — gpt-5

Implemented the decoded fields as behavior, not field-touch bookkeeping.

❯ `dedupe` distinguishes command-ID replay, no cross-command dedupe, and idempotency-key dedupe.

### Re: projected dedupe fragment — gpt-5

This is not a new operator directive. It is a projected bullet from the already committed `#lzdecorativewirefields` response. The behavior is already implemented in Rust commit `57f9eef7663a5a9c75afca7c6817b411478b5766`, locally verified by the decoded-wire fixture and full `make check`, and pinned by superproject commit `08fa3aaca526e1c7326851499b3a682bdc380a70`.

No repository paths changed, no new command was required, no escalation occurred, and no follow-up is created. Confidence is high because the fragment is byte-identical to the preceding assistant response rather than independent operator text.

### Re: dedupe semantics — gpt-5

Correct. The boundary is command identity versus optional cross-command policy:

- A repeated `command_id` is always an idempotent replay. Admission returns `Deduplicate` for that same command before consulting `policy.dedupe`, so the original projection entry is preserved.
- `none` records a submit with a new command ID even when its idempotency key matches an earlier command.
- `same_command_id` likewise performs no cross-command collapse: a new command ID is a new command. The same-ID replay case is already handled by the unconditional identity check.
- `same_idempotency_key` is the only policy that may collapse distinct command IDs. It requires a non-empty key and resolves the new submit to the command that already owns that key; `supersede` can instead replace a non-terminal owner under the separately defined preemption rules.

This is already shipped in lazily-rs commit `57f9eef7663a5a9c75afca7c6817b411478b5766` and pinned by superproject commit `08fa3aaca526e1c7326851499b3a682bdc380a70`; this turn required no repository changes. I re-ran the two focused executable witnesses: `command::tests::duplicate_submit_is_idempotent` and `decoded_wire_fields_drive_command_admission`; both passed. The latter covers fresh-ID record behavior for `none` and `same_command_id`, plus distinct-ID collapse for `same_idempotency_key`.

The latest lazily-rs CI for the shipped commit is green (`30965052363`). Existing unrelated Protobuf work and other dirty files were preserved. No escalation or follow-up work is created.


### Re: dedupe semantics — gpt-5

Confirmed:

- Repeated `command_id` is always an idempotent replay.
- `none` and `same_command_id` never collapse distinct command IDs.
- `same_idempotency_key` may collapse distinct commands sharing a non-empty key.

No code changes were needed. Both focused tests passed, and the shipped Rust commit’s CI is green. The response was committed; next queue head is `#lzprotobufinterop`.

### Re: Response

> **Queue prompt:**
>
> do [#lzprotobufinterop]


Implemented [#lzprotobufinterop] as an optional generated encoding of the canonical graph-boundary algebra—not a second graph runtime.

The spec now owns a versioned `lazily.graph_boundary.v1` Protobuf family for capability handshakes, stable graph/source/cell identity, bounded UTF-8 `CellTextSplice` operations, source generations, causal epochs/sequences, source membership, derived projections, effect intents, delivery receipts, and explicit bootstrap/recovery/compaction snapshots. Canonical JSON remains the logical diagnostic/hash form, msgpack interop is unchanged, and fixture assertions remain JSON-Schema validated. The field ledger reserves evolution ranges and requires both peers to advertise `protobuf-graph-boundary-v1`; Protobuf byte identity is explicitly not a conformance claim.

Landed reproducible generation and native logical reducers in Rust, Kotlin, and TypeScript. All three replay the same six canonical traces proving partial typing retains one cell, cross-cell edits stay bounded, clean cache reloads fence stale queued splices, remote projection plus native reload does not republish a whole buffer, duplicate/gap ordering is deterministic, and bootstrap snapshots remain distinct from ordinary mutation. This also applies the operator correction from the live session: projection is derived convergence state; delivery receipts are protocol data, not an editor request/ACK gate.

Python, Go, C++, Dart, Zig, and C# now record the capability-gated parity gap explicitly rather than silently claiming coverage. The TypeScript package includes generated runtime/types in `npm pack`, preserves the existing package surface, and reports zero audit vulnerabilities.

Verification: lazily-spec, Kotlin, TypeScript, Python, Go, C++, Dart, Zig, and C# full `make check` gates passed. Rust passed its complete test/benchmark/conformance run; fixture coverage reported 137/143 canonical fixtures opened, all 143 opened scenarios replayed, and all 30 assertion blocks bound. Its final CI-reach audit initially exposed the new optional target as missing from CI; the workflow now fetches the canonical schema before all-feature compilation and invokes the Protobuf trace target explicitly, and the reachability guard passes with all 48 gates reached. Component commits were pushed as spec `5241f4d`, Rust `b88d68d`, Kotlin `d811a83`, TypeScript `a32f69f`, Python `1a27663`, Go `5decc44`, C++ `9c8c612`, Dart `e8a8644`, Zig `c68c5a1`, and C# `88715eb`; root pin `503e533c88` is pushed. Spec, TypeScript, Python, Go, Dart, and C# CI are green; Rust, Kotlin, C++, and Zig CI were still running at closeout and are asynchronous evidence, not acknowledgements required for projection or session advancement.

The spec now owns a versioned `lazily.graph_boundary.v1` Protobuf family for capability handshakes, stable graph/source/cell identity, bounded UTF-8 `CellTextSplice` operations, source generations, causal epochs/sequences, source membership, derived projections, effect intents, delivery receipts, and explicit bootstrap/recovery/compaction snapshots. Canonical JSON remains the logical diagnostic/hash form, msgpack interop is unchanged, and fixture assertions remain JSON-Schema validated. The field ledger reserves evolution ranges and requires both peers to advertise `protobuf-graph-boundary-v1`; Protobuf byte identity is explicitly not a conformance claim.

Landed reproducible generation and native logical reducers in Rust, Kotlin, and TypeScript. All three replay the same six canonical traces proving partial typing retains one cell, cross-cell edits stay bounded, clean cache reloads fence stale queued splices, remote projection plus native reload does not republish a whole buffer, duplicate/gap ordering is deterministic, and bootstrap snapshots remain distinct from ordinary mutation. This also applies the operator correction from the live session: projection is derived convergence state; delivery receipts are protocol data, not an editor request/ACK gate.

Python, Go, C++, Dart, Zig, and C# now record the capability-gated parity gap explicitly rather than silently claiming coverage. The TypeScript package includes generated runtime/types in `npm pack`, preserves the existing package surface, and reports zero audit vulnerabilities.

Verification: lazily-spec, Kotlin, TypeScript, Python, Go, C++, Dart, Zig, and C# full `make check` gates passed. Rust passed its complete test/benchmark/conformance run; fixture coverage reported 137/143 canonical fixtures opened, all 143 opened scenarios replayed, and all 30 assertion blocks bound. Its final CI-reach audit initially exposed the new optional target as missing from CI; the workflow now fetches the canonical schema before all-feature compilation and invokes the Protobuf trace target explicitly, and the reachability guard passes with all 48 gates reached. Component commits were pushed as spec `5241f4d`, Rust `b88d68d`, Kotlin `d811a83`, TypeScript `a32f69f`, Python `1a27663`, Go `5decc44`, C++ `9c8c612`, Dart `e8a8644`, Zig `c68c5a1`, and C# `88715eb`; root pin `503e533c88` is pushed. Spec, TypeScript, Python, Go, Dart, and C# CI are green; Rust, Kotlin, C++, and Zig CI were still running at closeout and are asynchronous evidence, not acknowledgements required for projection or session advancement.
### Re: Response

Implemented [#lzprotobufinterop] as an optional generated encoding of the canonical graph-boundary algebra—not a second graph runtime.



### Re: Response

The four apparent steering directives are the assistant-authored completion paragraphs from the immediately preceding committed [#lzprotobufinterop] closeout, projected back through the editor as if they were new operator input. They introduce no new requirement: (1) restates the landed schema, (2) restates the three pilot bindings and reactive-projection correction, (3) restates the capability-gated parity ledger, and (4) restates verification and commit evidence. No further implementation is warranted from those synthetic directives. The underlying defect is exactly the one raised by the operator: assistant projection must be classified reactively from document provenance/state, not turned into a request that waits for or manufactures an editor acknowledgement.

### Re: Response

Implemented [#lzassertionblockschema] upstream in lazily-spec.

Added a checked-in Draft 2020-12 `schemas/assertion-blocks.json` plus a deterministic generator. It inventories all three canonical claim spellings—`assertions`, `expect`, and `expected`—and routes them by fixture-relative path plus normalized JSON pointer. The current 649 assertion objects collapse into 142 live routes spanning every fixture area, including collections, ingress, materialization, reactive-graph, temporal, and root-level fixtures that have no whole-fixture schema. Each route rejects unknown keys with `additionalProperties: false`, requires keys common to that route, and validates nested scalar/array/object value shapes.

The guard is bidirectional: every corpus block must resolve and validate, and every checked-in route must still be exercised by the corpus. Mutation tests prove rejection of an unknown assertion key in every area, a missing required key, a wrong value type, and an unknown fixture/pointer route. `make check` now runs generator drift detection before the schema suite, so fixture changes require an explicit visible schema update. Documentation distinguishes this authoring/schema guard from binding-side consumed/asserted ledgers and from Protobuf's intentionally tolerant unknown-field behavior.

Verification is green: 269 pytest tests, Lean build, coverage sync and coverage-claim checks, 130 vendored fixture copies, async-v2 naming, all 152 scenario identities, prose-key validation, and assertion-ordering self-test. Spec commit `4ab05d6` and root pin `30e082eec3` are pushed; lazily-spec CI run `30967864591` completed successfully. The separate [#lzwqfixtureconfig] item still owns adding `visibility_timeout` and `max_deliveries` to the work-queue fixture inputs; once those fields exist, this schema machinery will make their fixture shapes fail closed rather than leaving runner constants out of band.

[#lzassertionblockschema] is already implemented, verified, pushed, and reported in the immediately preceding committed cycle (`4ab05d6`, root pin `30e082eec3`, CI green). The retained editor-delivery recovery committed the response but failed to project the accompanying queue completion, leaving the same head live. This cycle applies only the missing `done` mutation; there is no duplicate implementation work or new follow-up.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzassertionblockschema]

### Re: Response

[#lzassertionblockschema] is already implemented, verified, pushed, and reported in the immediately preceding committed cycle (`4ab05d6`, root pin `30e082eec3`, CI green). The retained editor-delivery recovery committed the response but failed to project the accompanying queue completion, leaving the same head live. This cycle applies only the missing `done` mutation; there is no duplicate implementation work or new follow-up.

### Re: Response

You were right: waiting on an editor ACK was the wrong model. It existed because the old keyed lookup had no node for a missing document key, so an observer could not record an edge before the first editor publication. The compatibility membership epoch then acted as an imperative wake-up signal, which made a reactive projection behave like request + ACK coordination.

That workaround is removed. An exact-key observation now materializes one stable source immediately in `Unavailable`; the editor publishes directly into the same source as `Available(authority)`. The focused-document computation therefore owns its dependency before any publication exists, ignores unrelated document publications, and reruns on direct `Unavailable -> Available(value)` / unpublish / republish transitions without a membership epoch.

Implemented and shipped:

- Added the canonical exact-key dependency-availability contract and fixture to lazily-spec.
- Added sync, thread-safe, and async dependency maps plus direct fixture replay and stable/concurrent identity checks across Rust, Python, JavaScript, Go, C++, Dart, Kotlin, Zig, and C#.
- Migrated Agent Doc editor authority projection to `ThreadSafeDependencyMap<DocumentKey, DocumentAuthority>` and removed the compatibility membership bump.
- Unified the full Agent Doc workspace on lazily 0.56.0, avoiding duplicate incompatible Lazily graph types.
- Published and tagged lazily 0.56.0 on crates.io.
- Released, installed, pushed, and tagged Agent Doc 0.35.138; its Release and PyPI workflows were observed in progress.
- Pushed all binding/spec/formal commits and the superproject pins while preserving unrelated dirty and staged operator state.

Verification is green: every affected binding passed its full local `make check`; lazily-rs passed its complete benchmark, conformance-ledger, and formal gates; Agent Doc passed the focused pre-publication regression, fast install, full `make check`, `make tmux-ci`, and the one-time release-profile `make install-full`. The installed binary reports `agent-doc 0.35.138`.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzdependencyavailability]


`#lzdependencyavailability` is complete and is being moved to done. The implementation, nine-binding conformance, lazily 0.56.0 publication, Agent Doc 0.35.138 release, full verification, and repository pin updates were all completed in the preceding closeout; this continuation only settles the queue/backlog state.


### Re: Response

Implemented `#lzcpptranscribedrunners` as reactive fixture projection, not request/ACK synchronization.

- Converted `test_family_sync.cpp` to open and replay canonical `familysync/materialize_on_ingest.json`; peers, writes, optional re-ingest, and every expected projection now come from the fixture.
- Converted the hand-transcribed reliable-sync sections in `test_reliable_sync.cpp` to replay canonical `resync_gap_converge.json`, `idempotent_redelivery.json`, `outbox_replay_after_crash.json`, and `liveness_orset_lww.json`. Every scenario is dispatched from canonical input and every `expect` key is consumed through `AssertionKeys`.
- Left the already-canonical `multi_epoch_delta.json` and `outbox_store_protocol.json` runners intact.
- Registered `test_family_sync` as a canonical fixture test and made the binary require one loaded fixture; `test_reliable_sync` now requires five.
- Added `familysync` to required coverage, removed the four newly covered reliable-sync excuses, retained only coalescing, lease eviction, and journal decoding, and left the explicitly documented root IPC corpus gap unchanged.
- Runtime manifest now proves 127 distinct fixtures and 137 replayed scenarios; floors were tightened to those observed values.
- Verification: focused family/reliable-sync build and CTest passed; full `make check` passed.
- Pushed lazily-cpp `dc2dac8` and superproject pin `c0f80bc8fa`. The lazily-cpp CI run was observed once at the pushed SHA and was queued.


### Re: Response

Completed `#lzreactdtsgate`.

- Added a strict declaration-only `tsconfig.json` covering `src/**/*.d.ts` with `noEmit`, `allowJs:false`, `skipLibCheck:false`, NodeNext resolution, and the `esnext.disposable` library required by lazily-js's published `Symbol.dispose` surface.
- Added explicit `typescript` and `@types/react` development dependencies plus `npm run typecheck`.
- Wired `typecheck` into `make check` between JS syntax validation and the test suite.
- Added a dedicated CI declaration step and a non-empty-program assertion that proves at least one local `src/*.d.ts` file was loaded.
- Mutation probe 1: added an undefined declaration type; `make check` failed at `typecheck` before any tests ran.
- Mutation probe 2: changed the include to match no declarations; the local source count became zero and the CI assertion exited nonzero (the compiler also rejected the empty program).
- Restored both mutations and verified full `make check`: formatting, syntax, declaration typecheck, all 18 tests, and CI reachability passed; reachability reports `typecheck` as covered.
- Pushed lazily-react `61c9b1b` and superproject pin `e9697a4464`. The lazily-react CI run was observed once at the pushed SHA and was queued.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzegress]


We should not have been waiting on an editor ACK. That was an architectural leak from treating editor synchronization as request/response. The correct model is binary-owned retained intent with the editor as a reactive projection: submit the closeout intent once, let projection convergence happen independently, and recover from the retained intent if the editor races. No editor ACK is part of delivery authority.

Completed `#lzegress` with the same rule in the egress design:

- Added a pure Rust `EgressCore<T>` owning monotone sequence assignment, pending/unacknowledged delivery state, the cumulative ACK watermark, bounded retry/backoff, and the send-side producer-generation fence.
- Added sync, thread-safe, and async shells exposing `pending`, `inflight`, `acked_through`, and `retry` as independent Computeds.
- Each transport attachment owns exactly one Effect. It observes both `pending` and `inflight`; when a cumulative ACK reopens a bounded send window, `inflight` invalidation reactively re-runs that same Effect. There is no projection request/ACK handshake.
- Reconnect advances the generation, retains sequence and ACK progress, requeues unacknowledged records in order, and makes the old attachment inert at the core fence.
- Kept conflation/backpressure and durability as composition boundaries: `RelayCell`/`Outbox`, `SpillStore`, and `DurableOutbox` remain the adapters rather than being reimplemented inside the Effect.
- Added four canonical egress fixtures covering ordered cumulative ACK, in-flight window reopening, retry exhaustion, and generation fencing. All fixture steps and positive/negative invalidations replay against all three Rust shells.
- Added the Lean egress model and proofs for monotone sequence allocation, monotone ACK progress, stale-generation state preservation, reconnect fence advancement with ACK preservation, and bounded retry attempts/backoff.
- Added explicit `make check` and GitHub CI reach for the three-flavor egress runner.

Verification:

- `lazily-spec make check`: green (270 tests plus schema/formal/coverage guards).
- `lazily-formal make check`: green (53 Lean jobs).
- `lazily-rs make check`: green, including clippy/all-features, Loom, all conformance suites, formal builds, benchmarks, coverage, assertion ordering, and CI reach.
- CI observed once after push: lazily-spec succeeded; lazily-rs and root Agent Doc workflows were in progress without failure; lazily-formal has no Actions runs configured.

Pushed commits:

- lazily-spec `9883f5f` — `feat: specify reactive egress`
- lazily-formal `8e6917a` — `feat: model reactive egress`
- lazily-rs `550d18a` — `feat: add reactive egress family`
- root pins `5c424cbd09` — `chore: update reactive egress pins`

Unrelated dirty `AGENTS.md` and generated navigation-runbook changes in the owner checkouts were preserved and not committed.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzqueuefamilyflavors]


You were right about the editor ACK: closeout is a binary-owned retained intent, while the editor is only a reactive projection of committed session state. Completion must not depend on an editor request/ACK round trip. The earlier CAS race validated that design: the binary retained the closeout, and session-check later observed it committed without resubmitting the response payload.

Completed `#lzqueuefamilyflavors` without reimplementing the already-landed bindings:

- Verified JS, Zig, and C# against their own 3×3 queue-family ledgers and canonical corpus replay, including direct green runs for JS and C# and the full Zig test suite.
- Flipped the six thread-safe/async Queue, Topic, and WorkQueue rows for JS, Zig, and C# in `lazily-spec/coverage.json`.
- Regenerated all 12 README/document coverage tables; `lazily-spec make check` (including `sync-coverage.mjs --check` and coverage-claims validation) is green.
- Replaced Zig's brittle absolute compliance claim with generated-matrix authority plus explicit 3×3 ledger/replay evidence.
- Added explicit `KNOWN_UNCOVERED` entries for the four preceding Rust-only egress fixtures in bindings that do not ship egress, fixing strict manifest gates without claiming false replay coverage.
- Ran every affected repository's required `make check`; all are green. The root check's 8,633-test main suite passed, with only four unrelated parallel OAuth port-collision failures in `corky-core`; the same crate suite passed serially (67/67).
- Pushed synchronized commits in all 11 affected Lazily repositories and superproject pin commit `48cb6776ca6f8488627cf910f7d6c9d3b3617db8`.
- One-time CI snapshot: spec, Python, Kotlin, JS, Dart, Zig, Go, C++, and C# succeeded; Rust and agent-loop were in progress; lazily-formal has no Actions run.

Unrelated pre-existing `AGENTS.md`, runbook, and nested-worktree changes were preserved and never staged.

Unrelated pre-existing `AGENTS.md`, runbook, and nested-worktree changes were preserved and never staged.
### Re: Response

Completed `#lzwqfixtureconfig`.

- Moved `visibility_timeout` and `max_deliveries` from a top-level `config` object into each canonical WorkQueueCell fixture's `initial` block.
- Tightened the lazily-spec schema guard: both fields are required with positive bounds in `initial`, and a top-level WorkQueueCell `config` is rejected.
- Updated every canonical replay path in Rust, Python, Kotlin, JavaScript, Dart, Zig, Go, C++, and C# to construct the queue from `initial.visibility_timeout` and `initial.max_deliveries`. This includes the dedicated replayers as well as the 3×3 family runners; filename switches and literal lease values are gone from fixture runners.
- Preserved assertion-key accounting in Go by routing the initial pending/in-flight/dead-letter emptiness checks through its tracked assertion seam.
- Targeted schema and replay suites pass. `make check` is green in lazily-spec and all nine bindings, including Rust mutation checks and Python's full assertion-inventory gate.
- Pushed commits: spec `3bf4deb`, Rust `83b4ebc`, Python `73248c4`, Kotlin `380112f`, JS `9c845a9`, Dart `f25234b`, Zig `75d504a`, Go `6271da1`, C++ `1345e19`, C# `b3dd009`; superproject pins are published in `eb592c3efd64fe780355fc487c0ff40613183b2c`.
- One-time CI snapshot has no completed failures: spec, Python, and JS succeeded; Rust, Kotlin, Dart, Zig, Go, C++, C#, and agent-loop were still in progress.
### Re: Response

Completed `#lzwqfixtureconfig`.

- Moved `visibility_timeout` and `max_deliveries` from a top-level `config` object into each canonical WorkQueueCell fixture's `initial` block.
- Tightened the lazily-spec schema guard: both fields are required with positive bounds in `initial`, and a top-level WorkQueueCell `config` is rejected.
- Updated every canonical replay path in Rust, Python, Kotlin, JavaScript, Dart, Zig, Go, C++, and C# to construct the queue from `initial.visibility_timeout` and `initial.max_deliveries`. This includes the dedicated replayers as well as the 3×3 family runners; filename switches and literal lease values are gone from fixture runners.
- Preserved assertion-key accounting in Go by routing the initial pending/in-flight/dead-letter emptiness checks through its tracked assertion seam.
- Targeted schema and replay suites pass. `make check` is green in lazily-spec and all nine bindings, including Rust mutation checks and Python's full assertion-inventory gate.
- Pushed commits: spec `3bf4deb`, Rust `83b4ebc`, Python `73248c4`, Kotlin `380112f`, JS `9c845a9`, Dart `f25234b`, Zig `75d504a`, Go `6271da1`, C++ `1345e19`, C# `b3dd009`; superproject pins are published in `eb592c3efd64fe780355fc487c0ff40613183b2c`.
- One-time CI snapshot has no completed failures: spec, Python, and JS succeeded; Rust, Kotlin, Dart, Zig, Go, C++, C#, and agent-loop were still in progress.

Unrelated pre-existing `AGENTS.md`, runbook, and nested-worktree changes were preserved and never staged.

### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzwqfixtureconfig]


`#lzwqfixtureconfig` was fully completed and published in the immediately preceding committed cycle. This continuation is the stale backlog/queue projection restored by closeout snapshot repair, not new work: the canonical fixtures, schema guard, all nine binding runners, green local gates, ten pushed repository commits, and superproject pin `eb592c3efd64fe780355fc487c0ff40613183b2c` are already durable. Marking the restored queue item done again without reimplementation or duplicate commits.


### Re: Response

> **Queue prompt:**
>
> 📌 do [#lzingressadapters]


We were waiting because the old durable-response barrier conflated two different facts: the inbound editor→canonical consistent cut and outbound canonical→editor projection delivery. That accidentally promoted an editor delivery ACK into transition authority.

That coupling is removed. Durable closeout intent is now binary-owned and may commit once the inbound consistent cut exists. An editor ACK is only a reactive receipt Source; native save/commit remains a downstream derived Effect gated by receipt convergence and an exact raw-disk projection. No request+ACK handshake authorizes the transition.

Implemented `#lzingressadapters` end to end:

- canonical boundary-ingress contract covering generation fencing, atomic snapshot-plus-buffer projection, gaps/backpressure, durable receipt target sets, duplicate-ACK idempotence, validation, and freshness derivation;
- Rust reference core with sync, thread-safe, and async shells;
- Lean formalization and mutation corpus;
- canonical replay across Python, JavaScript, Go, C++, Dart, Kotlin, Zig, and C#;
- agent-doc relay/controller/SimWorld/flow-closeout behavior updated so response intent precedes the reactive projection receipt.

Verification is green across every component. Agent-doc passed 8,638/8,638 tests, all TLA+ models, release-projection checks, the tmux integration suite, and the public-name scan. The root wrapper also reached that full green agent-doc result; its later unrelated corky OAuth tests exposed a parallel fixed-port race and pass when serialized. All component commits are pushed and the superproject pins them at `e69ba5980c7ac66d04b322c0219b82142ebbc726`.






### Re: Response

The repository side of npm trusted publishing is set up and hardened for `@lazily-hub/lazily-js`.

Updated `.github/workflows/release.yml` to:

- grant only `contents: read` plus the required `id-token: write` OIDC permission;
- use current `actions/checkout@v6`, `actions/setup-node@v6`, Node 24, and an explicit no-cache release install;
- use strict `npm ci` with no lockfile-bypassing fallback;
- publish without `NPM_TOKEN`; npm exchanges the GitHub OIDC identity for a short-lived token and generates provenance automatically;
- check for an existing version before publishing, so reruns are idempotent without masking a real authentication or publish failure;
- document the exact one-time trust-registration command.

The full `make check` passes, GitHub accepts the updated workflow, and CI is green for commit `2edccf0e4bfc9655d6b87fc0ed80691e3533c190`. The superproject pins it at `fb9cc186f5673bc589c49565b9b853e4204f5be6`.

The live npm trust record still needs the maintainer’s npm login and 2FA. I started npm’s browser login, but its callback was not completed, so I did not claim the external registration succeeded. After `npm login`, run:

```sh
npm trust github @lazily-hub/lazily-js \
  --repo lazily-hub/lazily-js \
  --file release.yml \
  --allow-publish
```

Then verify with `npm trust list @lazily-hub/lazily-js`. This matches npm’s current [trusted-publishing workflow requirements](https://docs.npmjs.com/trusted-publishers/) and [`npm trust github` CLI](https://docs.npmjs.com/cli/v11/commands/npm-trust/).

### Re: Response

First, npm trusted publishing is now genuinely fixed. The existing npm record targeted stale `publish.yml`; I revoked only that record, created the correct `release.yml` trust, and verified npm now reports:

- type: GitHub
- repository: `lazily-hub/lazily-js`
- workflow: `release.yml`
- permission: publish

For the Haiven request: I read the topology document and reviewed its existing “HAVN-596 delivery plan and Temporal replacement” and “Proposed FPE process topology” sections. The plan is already present in that private document and is technically sound, so I did not duplicate it or modify any Haiven repository before IP clearance.

## Architecture decision

Do not build a general Temporal clone, and do not make NATS the workflow database.

- **PostgreSQL is authoritative** for review/escalation state, versions, commands, timers, attempts, leases, terminal errors, idempotency, and transactional outbox state.
- **JetStream is a bounded delivery/wake-up boundary** for backend→FPE signals, independently scaled ready/resume work, compact audit/archive handoff when warranted, and dead-letter visibility.
- **Application code owns a small explicit state machine** for the durable continuations that actually need Temporal-like behavior.
- **The common moderation path is not a workflow.** It remains typed in-process calls inside the shared Python FPE and commits one compact decision.
- Preserve the measured topology: one shared GPU/model owner per accelerator with one adaptive micro-batcher, rather than fragmenting traffic across per-stage services.

This preserves the required subset of Temporal’s guarantees—durable resume, timers, bounded retries, idempotency, operator-visible failure, and safe rolling deployment—without reproducing Temporal’s per-message event-history machinery. Temporal’s own documentation describes event history as the source of truth used for deterministic replay; the replacement deliberately uses explicit persisted state instead of emulating that history model: [Temporal workflows](https://docs.temporal.io/workflows). JetStream remains at-least-once, so application-level deduplication and commit-before-ack are mandatory: [NATS consumers](https://docs.nats.io/nats-concepts/jetstream/consumers).

## Delivery plan

1. **IP and contract gate**
   - Keep this plan and all derived code private to the Haiven workspace.
   - Obtain written clearance on foreground/background IP, permitted reuse, and publication before implementation or extraction into any public Lazily repository.
   - Freeze a short ADR covering ownership, subjects, at-least-once semantics, retention, security classification, and synchronous-versus-asynchronous product behavior.

2. **Ship HAVN-596 independently**
   - Release a typed, versioned `SignalEnvelope` from `haiven-contracts`.
   - Add a backend `EventPublisher` port plus JetStream implementation.
   - Use a backend transactional outbox so the domain write and publish intent commit together.
   - Publish with a stable message/idempotency ID, wait for replicated publish acknowledgement, preserve trace identity, retry with bounded exponential backoff+jitter, and expose terminal failures without logging sensitive payloads.
   - Acceptance: schema mapping, subject validation, publish acknowledgement, NATS outage/recovery, redelivery, and operator pending/failed metrics. The FPE consumer is explicitly outside HAVN-596 acceptance.

3. **Introduce an executor seam inside the FPE**
   - Keep Temporal and add a PostgreSQL application executor behind a feature flag.
   - Persist immutable `executor_version`/ownership so a signal cannot advance through both implementations.
   - Define pure transition handlers separately from persistence and side effects.

4. **Collapse the common path**
   - On JetStream delivery: validate and deduplicate, load context, run engines/aggregation/fusion/policy as typed calls, commit the compact decision and optional review state, then acknowledge.
   - Remove per-message workflow runs, activity histories, and internal ready/decision event round trips from this path.

5. **Implement the Temporal-like subset for durable continuations**
   - Persist `state`, `state_version`, unique `command_id`, `due_at`, `next_attempt_at`, attempt count, lease owner/expiry/generation, last error, terminal reason/timestamps, and any compact outbox event.
   - Claim due work with `FOR UPDATE SKIP LOCKED`; fence final writes by state version plus lease generation.
   - Recover expired leases, apply per-transition timeout/retry/backoff policy, and move exhausted or permanent failures to an operator-visible terminal/dead-letter state.
   - Review resolution commits state plus resume-outbox intent atomically.

6. **Use NATS only at real failure boundaries**
   - Durable pull consumers for horizontally scaled realtime/batch lanes; explicit per-message acknowledgement only after the corresponding PostgreSQL transaction commits.
   - Stable `Nats-Msg-Id` plus database deduplication; JetStream’s deduplication window is an optimization, not the permanent idempotency ledger.
   - Separate realtime and batch durable consumers, concurrency/resource quotas, and backpressure so replay cannot starve live moderation.
   - Keep in-process engine/policy calls off NATS.

7. **Deploy role-specific processes from one codebase**
   - `fpe-api`: inline/status/review/operator APIs.
   - `fpe-worker-realtime`: signal intake and latency-critical durable work.
   - `fpe-worker-batch`: replay/backfill/bulk re-evaluation.
   - `fpe-maintenance`: due retries, lease reclamation, reconciliation, and outbox recovery.
   - optional `fpe-model`: GPU lifecycle owner and adaptive batcher, never a durability authority.

8. **Shadow and cut over safely**
   - Shadow only pure planning/transition logic; never double side effects.
   - Compare decisions, transitions, pause/resume, timers, retry outcomes, and error classification against the current Temporal corpus.
   - Route new work by ownership cohort. Let existing Temporal workflows drain; do not live-convert paused histories without a separately proven mapper.
   - Retain an emergency flag for new work during the rollback window, then remove Temporal server, database, worker, SDK, and configuration only after the old cohort reaches zero.

9. **Fault and scale gates**
   - Inject duplicate/out-of-order delivery, crash before/after every commit/ack boundary, stale lease completion, competing replicas, retry exhaustion, poison contracts, review pause/resume, outbox redelivery, NATS outage, and regional recovery.
   - Measure decision latency, publish-ack latency, backlog age, lease contention, duplicate suppression, retry/terminal-error rate, hot rows/bytes per open review, common-path rows per message, and archive lag.

The plan stays planning-only until IP clearance. No Haiven code, contract, or repository state was changed in this cycle.

### Re: Lazily feature gap for the FPE topology

> **Queue prompt:**
>
> Please read /home/brian/work/btakita/agent-loop/src/haiven-dev/src/fpe-scale-spike/docs/fpe-service-topology.md. Create a plan for a Temporal-like features to support src/fpe-scale-spike/docs/fpe-service-topology.md & NATS integration. Note that I am under agreement with Haiven, so will need to clear IP first. But please create the plan anyways.


Yes: **Postgres and NATS are the two primary integrations. Valkey is beneficial, but only as an optional disposable acceleration layer.**

The current Lazily family already has much of the *pure semantic core*:

- flat state machines and Harel state charts;
- local `WorkQueueCell` claim/ack/nack/redelivery/DLQ behavior;
- `LeaseCell` with a monotonically increasing fence;
- retry, timeout, timer, interval, and cron cells;
- transport-agnostic `IngressCell` envelopes with generation, sequence, freshness, dedupe, and reorder handling;
- JSON/msgpack wire codecs, command-plane messages, causal receipts, health/readiness, and reliable-sync protocols;
- an `OutboxStore` seam for reliable state-frame replication.

What it does **not** yet have is the durable external-authority layer required to replace Temporal. The local cells model the right decisions, but their state disappears with the process. Also, reliable-sync's current `OutboxStore` is ordered frame/cursor storage; it cannot atomically commit a domain state transition and an event in the same Postgres transaction.

### Required features, in priority order

| Priority | Lazily feature | Why the plan needs it |
|---|---|---|
| P0 | IP/provenance gate | Keep the design generic and do not copy Haiven schemas, names, thresholds, or code into public Lazily repositories until contractual clearance. |
| P1 | Durable state-table/reducer protocol | Atomically load a versioned state, apply a pure transition, compare-and-swap the revision, record the command/attempt, and emit effects. This is the durable counterpart to the existing in-memory state machines. |
| P1 | Postgres durable-runtime adapter | Tables and operations for state, commands, attempts, timers, idempotency/effect receipts, leases, terminal outcomes, and transactional outbox events. It needs `FOR UPDATE SKIP LOCKED`, unique constraints, and fencing-generation checks. |
| P1 | Transactional unit of work | Commit domain mutation + effect receipt + outbox append in one database transaction. A standalone `PostgresOutboxStore` is insufficient unless it participates in the caller's transaction. |
| P1 | NATS JetStream adapter family | Durable publish, pull consumption, explicit ack/nak/progress/term, message metadata, replay, and declarative stream/consumer configuration. |
| P1 | Async broker runner | Bounded fetch/concurrency, backpressure, graceful drain, reconnect, cancellation, and the rule “ack only after the Postgres commit.” The current nonblocking synchronous transport seams do not own this lifecycle. |
| P2 | Durable scheduler/timer store | `schedule`, `cancel`, `claim_due`, `reschedule`, and recovery of abandoned timers. Existing timer/retry cells compute policy; they do not persist future work. |
| P2 | Durable work/lease store | Stable item IDs, claim tokens, monotonically increasing fences, renew/release, expired-lease reclamation, attempt history, and DLQ transitions. This should back a reactive projection rather than putting database I/O inside a cell. |
| P2 | Operational projections | Reactive queue depth, oldest-ready age, lease age, retry backlog, outbox lag, consumer lag, redelivery count, DLQ count, and stale-fence rejection, with OpenTelemetry correlation. |
| P2 | Executor/cutover seam | A common executor interface for `temporal | native | shadow`, pinned executor versions, shadow-result comparison, cohort routing, draining, and rollback. |
| P3 | Valkey cache/coordination adapters | Optional cache, duplicate-fill suppression, rate limiting, and ephemeral coordination. Never authoritative for workflow correctness in this design. |

### NATS: implement a family, not one generic adapter

There are several useful Lazily-to-NATS seams:

1. **`JetStreamPublisher` / command sink**  
   Encode a versioned envelope, publish with a stable `Nats-Msg-Id`, await the JetStream publish acknowledgement, and expose retryable versus terminal failures.

2. **`JetStreamPullConsumer` / leased delivery source**  
   Fetch bounded batches and expose delivery metadata plus `ack`, `nak(delay)`, `progress`, and `term`. Pull consumers are the right default for horizontally scaled workers.

3. **`NatsIngressAdapter`**  
   Decode the application envelope into Lazily's existing `IngressEnvelope`; broker metadata supplies sequence/redelivery information, while application headers supply generation, correlation, contract version, tenant, and trace context.

4. **`PostgresOutboxToNatsRunner`**  
   Claim unpublished outbox rows with `SKIP LOCKED`, publish using the outbox event ID as `Nats-Msg-Id`, then mark published only after the broker acknowledgement.

5. **Declarative topology and diagnostics**  
   Idempotently ensure streams/consumers and validate subjects, retention, replicas, `AckExplicit`, `AckWait`/`BackOff`, `MaxDeliver`, and `MaxAckPending`. Emit consumer-lag and max-delivery advisories.

JetStream should **not** be implemented merely as the current synchronous `QueueStorage`. That interface has `try_push`/`try_pop` and destructive pop semantics, but a broker delivery remains pending until it is acknowledged and can be redelivered to another worker. Lazily needs an async `LeasedDeliverySource<T>` (or equivalent) protocol that preserves the delivery token and ack lifecycle. The existing `WorkQueueCell` semantics can then serve as the conformance model.

For this topology, NATS is also **not the workflow-state authority**. The safe consumer choreography is:

```text
JetStream delivery
  -> begin Postgres transaction
  -> dedupe command
  -> claim/advance state using revision + fencing checks
  -> record attempt/effect receipt/outbox events
  -> commit
  -> ACK JetStream
```

A crash before commit causes harmless redelivery. A crash after commit but before ACK is handled by the idempotency key. NATS documents JetStream as at-least-once and provides explicit acknowledgements, pull consumers, redelivery controls, and `Nats-Msg-Id` deduplication; those mechanisms complement, rather than replace, database idempotency ([JetStream](https://docs.nats.io/nats-concepts/jetstream), [consumer details](https://docs.nats.io/using-nats/developer/develop_jetstream/consumers), [model deep dive](https://docs.nats.io/using-nats/developer/develop_jetstream/model_deep_dive)).

### Valkey: useful, but deliberately secondary

A Valkey adapter would be valuable for:

- versioned context/policy/model-result caches with TTL and namespace isolation;
- request coalescing or duplicate-fill suppression using an owner token and compare-and-delete;
- disposable reactive projections for dashboards;
- distributed rate-limit, bulkhead, or short-lived semaphore state;
- optional ephemeral presence/readiness hints.

I would define narrow protocols such as `CacheStore`, `SingleFlightStore`, and possibly `RateLimitStore`, with a `Valkey*` implementation. Cache misses and complete Valkey loss must degrade to Postgres/object storage and recomputation.

I would **not** use Valkey for:

- workflow or review state;
- the transactional outbox;
- effect receipts/idempotency authority;
- durable timers/retries;
- the sole lease/fencing authority;
- a second durable event stream alongside JetStream.

Valkey Streams do provide consumer groups and pending-entry acknowledgements ([Streams](https://valkey.io/topics/streams-intro/), [XACK](https://valkey.io/commands/xack/)), but using them here would duplicate JetStream's job and create two recovery models. If a Valkey coordination adapter is added later, every grant should carry a monotonically increasing fencing token that the Postgres write path validates; expiry alone is not sufficient to prevent a paused former holder from writing stale results.

### Recommended package shape

Keep optional infrastructure dependencies out of Lazily's pure core:

- `lazily-spec`: generic durable-state, leased-delivery, transactional-effect, and scheduler contracts plus failure/replay fixtures;
- `lazily-py`: protocol surfaces and reference runners;
- `lazily-postgres`: Postgres stores and transaction coordinator;
- `lazily-nats`: JetStream publisher, consumer, ingress adapter, topology validation, and outbox dispatcher;
- `lazily-valkey`: cache/single-flight/rate-limit adapters only.

For the FPE use case, implement **Python first** behind generic conformance fixtures. Cross-language ports can follow when another binding has a real deployment consumer; the infrastructure adapters should not become mandatory core conformance for all nine languages.

### Minimal vertical slice

The smallest useful slice is:

1. IP clearance and a generic durable-runtime spec;
2. Postgres state transition + idempotency receipt + transactional outbox in one transaction;
3. JetStream outbox publisher and pull consumer;
4. consumer DB-commit-before-ACK runner with crash-point tests;
5. due-work/expired-lease maintenance runner;
6. metrics and a shadow executor;
7. optional Valkey cache after correctness is demonstrated without it.

That ordering gives the plan its Temporal-like durability first. Valkey improves latency and load later; it does not close a correctness gap.

### Re: `#lazilystatetable` — typed settlement table without request/ACK

Implemented and pushed the Agent Doc pilot. The retained-write reducer is now an eight-row finite semantic table inside the existing document-scoped Lazily graph.

- **Invariant/policy owner:** `RetainedSettlementTable` owns the total settlement decision; the durable document projection still owns intent identity, ordering, hashes, and receipts. Storage hydrates observations but never authorizes a transition.
- **Transition rows covered:** no intent; unobserved planes; authority/disk divergence; exact target; rebased payload materialized; superseded delta materialized; superseded later closeout stage; and converged content missing the payload. Coverage asserts all eight constructible rows and all eight decision/proof outcomes—there is no invalid catch-all row.
- **Reactive topology:** document-scoped `Source` observations → `thread_safe_projected_state_table` input classifier → `Computed<SettlementDecision>` → downstream `Computed<SettlementVerdict>` hydration → existing idempotent effects/receipts. Raw hash churn that preserves the semantic row stops at the guarded projection while the public verdict still carries the current observed hash.
- **Imperative calls removed/excluded:** no editor request, ACK wait, polling loop, sidecar runtime, or second authority was added. Transport/editor receipts remain observed facts; they are not transition gates.
- **Edited surfaces:** only `agent-doc-state-backbone/src/retained_write.rs` changed in the runtime, preserving its public verdict and durable event schemas. The scoped plan was updated at `tasks/software/plan-lazily-state-tables.md`.
- **Verification:** focused retained-write tests and the full Agent Doc `make check` passed locally. Agent Doc commit `a4c0c69b1` and the root plan/submodule pin commit `008895c9fc` are pushed. Remote Agent Doc CI run `30983459260` and root Agent Doc run `30983906168` are active; neither has reported a failure. The root-wide local gate separately encountered an unrelated existing `corky-core --lib` failure after the Agent Doc gate passed.
- **Remaining work:** `[migration]` Phase 4 is semantic parity in `lazily-spec` and only the ports that are full reactive cores. `[operator verification]` inspect the two active CI runs when terminal; fix any task-related failure.

### Re: Postgres, NATS, Valkey, project boundaries, and language rollout

The durable-runtime plan is now in `tasks/software/plan-lazily-postgres-nats-valkey.md`.

The recommended project shape is Lazily as the umbrella, not one dependency-heavy crate:

- core and backend-neutral durability/conformance contracts live in Lazily;
- sibling packages provide `lazily-postgres`, `lazily-nats-jetstream`, and optional `lazily-valkey` implementations;
- a deployable worker/control plane and Haiven-specific workflow remain a separate consuming project.

Do not duplicate all three adapters across every supported language. Rust should be the first full durable/distributed host; `lazily-spec` should own the crash, redelivery, fencing, and receipt corpus. Other ports claim capability tiers—core, client, durable host, distributed host, accelerated host—and gain native hosting only when a real deployment requires it and the implementation can pass the shared corpus. Thin NATS publishers/observers do not need to reproduce the Postgres/outbox/lease stack. Valkey remains optional, bypassable acceleration rather than authority.

### Re: IP/public-knowledge gate

> **Queue prompt:**
>
> do [#lazilystatetable]


The generic “Lazily + Temporal + NATS” architecture is public knowledge. Lazily declares MIT, [Temporal Server is MIT](https://github.com/temporalio/temporal/blob/main/LICENSE), [NATS Server is Apache-2.0](https://github.com/nats-io/nats-server/blob/main/LICENSE), and [17 U.S.C. § 102(b)](https://uscode.house.gov/view.xhtml?edition=prelim&num=0&req=granuleid%3AUSC-prelim-title17-section102%28a%29%282%29) excludes ideas, systems, processes, and methods of operation from copyright.

That does not settle contractual ownership of a particular implementation. The planned conversation should produce a written boundary identifying pre-existing Lazily as prior work, reserving generic adapters/conformance for the open-source project, assigning Haiven-specific workflows/schemas/configuration separately, and stating the rules for upstream contributions, employer time/equipment/accounts, and invention disclosure. The plan now records this as an explicit IP gate; signed agreements and applicable law control.

### Re: `#reliablesyncrow` — codex

I downgraded the Reliable sync family from `✅` to `~` for all nine bindings and made the two known-uncovered fixtures explicit in the policy ledger:

- `reliable-sync/coalesce_bounds_outbox.json`
- `reliable-sync/liveness_lease_eviction.json`

That is the truthful capability claim: none of the nine binding runners currently replay both fixtures. `scripts/sync-coverage.mjs` regenerated the spec and sibling README tables from `lazily-spec/coverage.json`.

The coverage guard passed with all nine ledgers loaded. I also mutation-tested the claim: temporarily changing Rust back to `✅` caused `check-coverage-claims.mjs --require-all` to fail specifically because both reliable-sync fixtures were absent from the Rust replay manifest; restoring `~` made the guard pass.

`make check` passed in `lazily-spec`, all nine binding repositories, `lazily-formal`, and the `agent-loop` umbrella repository. The spec and binding commits were pushed to their `main` branches. Their remote checks are green except Rust, which remains in progress; `lazily-formal` has no GitHub workflow configured.

The umbrella pins and plans were published as `5549eec541`. Its current Agent Doc workflow is in progress. The preceding umbrella failure was an infrastructure linker bus error (`collect2` terminated by signal 7), not a coverage or plan assertion failure; the umbrella check passed twice locally.

### Re: Temporal adapter and replacement plans — codex

> **Queue prompt:**
>
> do [#reliablesyncrow]


Yes: if Temporal remains self-hosted, Lazily should have a Temporal adapter when Lazily is intended to supply the deterministic projection/decision layer inside Workflows. In that mode Temporal History is the sole durable workflow authority. Workflow start, Signals, Updates, timers, Activity/child results, and cancellation become Lazily Source observations; Lazily reactively projects typed decisions; those decisions map to Temporal timers, Activities, child Workflows, Continue-As-New, or completion. NATS and databases stay behind Activities or Signal/Update bridges—there is no dual Postgres authority and no transport ACK authorizing a state transition.

The alternative is an application-level Temporal replacement: Lazily owns the typed durable execution model, Postgres is the sole authority, NATS JetStream supplies leased transport, and Valkey remains optional disposable acceleration. This is a much larger product commitment because it must own history/replay, effects and receipts, timers, retries, Signals/Updates/Queries equivalents, children, cancellation, versioning, visibility, schedules, operator controls, backup/restore, and migration without dual authority.

Both plans now live in the private `agent-loop` repository as requested:

- `tasks/software/plan-lazily-temporal-adapter.md`
- `tasks/software/plan-lazily-temporal-replacement.md`

The existing `tasks/software/plan-lazily-postgres-nats-valkey.md` cross-links both. The plans keep reusable adapters under the Lazily umbrella, reserve deployable/product-specific services for a separate repository, recommend the adapter by default, and gate public implementation on the ownership/IP conversation.

The unrelated staged deletion `tasks/professional/octavian-gabriel-stan.md` remains staged, and the local instruction/runbook edits remain uncommitted and excluded.

### Re: `#rootlevelconformance` — codex

Root-level IPC fixtures are now first-class coverage evidence rather than an unaudited exception.

The policy guard classifies the eight legacy root paths—`arena_blob.json`, `snapshot_*.json`, and `delta_*.json`—as the explicit `ipc` area. Both area-scoped binding guards now declare that area:

- Kotlin maps its already-replayed root fixtures from the opaque `(root)` pseudo-area to `ipc`, with unknown future root files deliberately receiving no credit.
- C++ now replays all seven canonical Snapshot/Delta fixtures from their actual JSON bytes through the real JSON codec, asserts every fixture assertion key, round-trips the decoded `IpcMessage`, and records the paths in the runtime manifest.

`coverage.json` now cites all seven Snapshot/Delta fixtures plus `distributed/crdt_sync_frames.json` for the IPC-wire row. The Shared-memory blob row cites `arena_blob.json`. That exposed the honest capability boundary: JavaScript, Dart, and C++ remain able to carry shared-blob wire references but do not replay the concrete canonical arena-host layout, so those three marks are now `~`, not `✅`. C++ lists `arena_blob.json` in `KNOWN_UNCOVERED` with the reason that its in-process arena does not implement the canonical mapped 40-byte LZSH header.

The central guard passes with all nine binding ledgers loaded: 84 shipped claims checked, 15 partial claims explicitly qualified, across 11 fixture-bearing rows.

Two mutation checks prove the new area is load-bearing:

- deleting only `snapshot_minimal.json` from the C++ runtime manifest fails that exact canonical fixture;
- deleting all root IPC entries from Kotlin fails both per-fixture accounting and “area `ipc` is required, but the suite opened no fixture in it.”

Changing C++'s shared-memory mark back to `✅` also fails specifically because `arena_blob.json` is uncovered.

The smaller Dart item was already repaired in the current branch: `distributed/crdt_sync_frames.json` has its own explanation—frontier omission is valid in the fixture, while Dart's decoder currently rejects the omitted frontier—so I preserved it rather than manufacturing another edit.

`make check` passed in the spec, C++, Kotlin, all other nine binding/documentation repositories, and `lazily-formal`. The substantive spec/C++/Kotlin CI runs and completed binding CI runs are green; Rust remains in progress. `lazily-formal` has no GitHub workflow.

The umbrella `make check` was attempted twice. Its task-owned subrepo checks are green, but unrelated Agent Doc integration tests failed while `/tmp` was at 100%: first a temp-file write, then an explicit `ENOSPC` in `native_reload_handoff`. The first exact failing test passed immediately in isolation. I did not delete unrelated `/tmp` data. The new umbrella Agent Doc workflow is in progress.

All repository commits were pushed and pinned by umbrella commit `ab9c9ff773`. The unrelated staged deletion `tasks/professional/octavian-gabriel-stan.md` remains staged, and local instruction/runbook edits remain excluded.
- Staged deletion: root repo, `/home/brian/work/btakita/agent-loop` — `tasks/professional/octavian-gabriel-stan.md`.
- Instruction/runbook edits: inside multiple `src/lazily-*` subrepos; the root repo reports these as dirty submodules.

Neither was included in the Lazily task commits.
- Staged deletion: root repo, `/home/brian/work/btakita/agent-loop` — `tasks/professional/octavian-gabriel-stan.md`.
- Instruction/runbook edits: inside multiple `src/lazily-*` subrepos; the root repo reports these as dirty submodules.

Neither was included in the Lazily task commits.

### Re: `#rootlevelconformance` — codex

Root-level IPC fixtures are now first-class coverage evidence rather than an unaudited exception.

The policy guard classifies the eight legacy root paths—`arena_blob.json`, `snapshot_*.json`, and `delta_*.json`—as the explicit `ipc` area. Both area-scoped binding guards now declare that area:

- Kotlin maps its already-replayed root fixtures from the opaque `(root)` pseudo-area to `ipc`, with unknown future root files deliberately receiving no credit.
- C++ now replays all seven canonical Snapshot/Delta fixtures from their actual JSON bytes through the real JSON codec, asserts every fixture assertion key, round-trips the decoded `IpcMessage`, and records the paths in the runtime manifest.

`coverage.json` now cites all seven Snapshot/Delta fixtures plus `distributed/crdt_sync_frames.json` for the IPC-wire row. The Shared-memory blob row cites `arena_blob.json`. That exposed the honest capability boundary: JavaScript, Dart, and C++ remain able to carry shared-blob wire references but do not replay the concrete canonical arena-host layout, so those three marks are now `~`, not `✅`. C++ lists `arena_blob.json` in `KNOWN_UNCOVERED` with the reason that its in-process arena does not implement the canonical mapped 40-byte LZSH header.

The central guard passes with all nine binding ledgers loaded: 84 shipped claims checked, 15 partial claims explicitly qualified, across 11 fixture-bearing rows.

Two mutation checks prove the new area is load-bearing:

- deleting only `snapshot_minimal.json` from the C++ runtime manifest fails that exact canonical fixture;
- deleting all root IPC entries from Kotlin fails both per-fixture accounting and “area `ipc` is required, but the suite opened no fixture in it.”

Changing C++'s shared-memory mark back to `✅` also fails specifically because `arena_blob.json` is uncovered.

The smaller Dart item was already repaired in the current branch: `distributed/crdt_sync_frames.json` has its own explanation—frontier omission is valid in the fixture, while Dart's decoder currently rejects the omitted frontier—so I preserved it rather than manufacturing another edit.

`make check` passed in the spec, C++, Kotlin, all other nine binding/documentation repositories, and `lazily-formal`. The substantive spec/C++/Kotlin CI runs and completed binding CI runs are green; Rust remains in progress. `lazily-formal` has no GitHub workflow.

The umbrella `make check` was attempted twice. Its task-owned subrepo checks are green, but unrelated Agent Doc integration tests failed while `/tmp` was at 100%: first a temp-file write, then an explicit `ENOSPC` in `native_reload_handoff`. The first exact failing test passed immediately in isolation. I did not delete unrelated `/tmp` data. The new umbrella Agent Doc workflow is in progress.

All repository commits were pushed and pinned by umbrella commit `ab9c9ff773`. The unrelated staged deletion `tasks/professional/octavian-gabriel-stan.md` remains staged in the root `agent-loop` repository. The local instruction/runbook edits remain confined to the individual Lazily subrepos and excluded from every task commit.

### Re: `#rootlevelconformance` — codex

Root-level IPC fixtures are now first-class coverage evidence rather than an unaudited exception.

The policy guard classifies the eight legacy root paths—`arena_blob.json`, `snapshot_*.json`, and `delta_*.json`—as the explicit `ipc` area. Both area-scoped binding guards now declare that area:

- Kotlin maps its already-replayed root fixtures from the opaque `(root)` pseudo-area to `ipc`, with unknown future root files deliberately receiving no credit.
- C++ now replays all seven canonical Snapshot/Delta fixtures from their actual JSON bytes through the real JSON codec, asserts every fixture assertion key, round-trips the decoded `IpcMessage`, and records the paths in the runtime manifest.

`coverage.json` now cites all seven Snapshot/Delta fixtures plus `distributed/crdt_sync_frames.json` for the IPC-wire row. The Shared-memory blob row cites `arena_blob.json`. That exposed the honest capability boundary: JavaScript, Dart, and C++ remain able to carry shared-blob wire references but do not replay the concrete canonical arena-host layout, so those three marks are now `~`, not `✅`. C++ lists `arena_blob.json` in `KNOWN_UNCOVERED` with the reason that its in-process arena does not implement the canonical mapped 40-byte LZSH header.

The central guard passes with all nine binding ledgers loaded: 84 shipped claims checked, 15 partial claims explicitly qualified, across 11 fixture-bearing rows.

Two mutation checks prove the new area is load-bearing:

- deleting only `snapshot_minimal.json` from the C++ runtime manifest fails that exact canonical fixture;
- deleting all root IPC entries from Kotlin fails both per-fixture accounting and “area `ipc` is required, but the suite opened no fixture in it.”

Changing C++'s shared-memory mark back to `✅` also fails specifically because `arena_blob.json` is uncovered.

The smaller Dart item was already repaired in the current branch: `distributed/crdt_sync_frames.json` has its own explanation—frontier omission is valid in the fixture, while Dart's decoder currently rejects the omitted frontier—so I preserved it rather than manufacturing another edit.

`make check` passed in the spec, C++, Kotlin, all other nine binding/documentation repositories, and `lazily-formal`. The substantive spec/C++/Kotlin CI runs and completed binding CI runs are green; Rust remains in progress. `lazily-formal` has no GitHub workflow.

The umbrella `make check` was attempted twice. Its task-owned subrepo checks are green, but unrelated Agent Doc integration tests failed while `/tmp` was at 100%: first a temp-file write, then an explicit `ENOSPC` in `native_reload_handoff`. The first exact failing test passed immediately in isolation. I did not delete unrelated `/tmp` data. The new umbrella Agent Doc workflow is in progress.

All repository commits were pushed and pinned by umbrella commit `ab9c9ff773`. The unrelated staged deletion `tasks/professional/octavian-gabriel-stan.md` remains staged in the root `agent-loop` repository. Local instruction/runbook edits remain confined to the individual Lazily subrepos and were excluded from every task commit.
<!-- agent:boundary:e668c328:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- do [#rootlevelconformance]
- do [#lzfixturediscrim]
- do [#lzrung2gaps]
- do [#lzstructfieldreads]
- do [#lzassertwithseam]
- 📌 do [#kny1]
- 📌 Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#rootlevelconformance] Root-level conformance fixtures cannot be audited by the coverage-claim guard. arena_blob.json, snapshot_*.json and delta_*.json carry no area, and the two area-scoped guards (lazily-cpp, lazily-kt) restrict their audit to REQUIRED_AREAS, so a root-level fixture is neither replayed nor listed in KNOWN_UNCOVERED there - the ledger simply cannot express it. lazily-cpp says so in a comment: root-level (8) snapshot/delta/arena_blob IPC wire fixtures, test_ipc.cpp hand-builds the equivalent structures. That leaves the Shared-memory blob path row and the Snapshot/Delta half of the IPC wire row unbacked, and I cited only area-qualified fixtures rather than let the guard return a confident wrong answer. Work: give the root corpus a real area in both area-scoped guards (or move those fixtures into an ipc/ area upstream), then cite the two rows. Second, smaller item in the same ledger family: lazily-dart lists distributed/crdt_sync_frames.json in KNOWN_UNCOVERED with no reason of its own - it sits under the comment belonging to the msgpack entry above it - in a ledger that requires a stated reason per entry.
- [ ] [#lzfixturediscrim] A fixture can be opened, have every key consumed, have every key asserted - and still be unfalsifiable, because its data cannot distinguish the claim it makes. Found in lazily-cs while closing #lzconsumednotasserted: reversing SignalingRoom's roster sort still PASSED, because the canonical anti_spoof_session.json transcript never produces a roster with more than one entry, so no ordering is observable and every emitted frame stays byte-identical. This is the fourth rung - fixture-opened, key-consumed, key-asserted, claim-discriminating - and no runner-side guard can reach it, because it is a property of the corpus data rather than of runner code. Only a mutation against the LIBRARY exposes it. Audit lazily-spec claim by claim: for each assertion key, mutate the behaviour it names and confirm at least one canonical fixture reddens; where none does, either strengthen the fixture data or record the claim as untested with a reason. lazily-cs worked its instance around locally by driving a three-peer room plus a vacuity guard that fails if the roster drops below two, but the corpus fix belongs upstream in lazily-spec so all nine bindings inherit it.
- [ ] [#lzrung2gaps] Conformance runners still outside the unconsumed-key guard entirely, all surfaced while closing #lzconsumednotasserted. lazily-cpp: nine fixture-replaying runners (command, distributed, lossless_tree, receipts, seqcrdt, statechart, stdlib, topic, reactive_graph) carry hand-rolled unknown-key guards instead of the shared AssertionKeys tracker, so they get neither rung 2 nor rung 3, and test_command_conformance.cpp has no unknown-key guard over its expect block at all. lazily-cs: StateChartConformanceTests has no FixtureAssertions tracker because its steps mix op inputs with assertions. Same class in lazily-kt, where bringing MaterializationConformanceTest under the tracker exposed two keys no runner had ever read - default_mode, and observe in deferral_not_deallocation.json. Convert each to the shared tracker so both rungs apply, and mutation-check every conversion rather than trusting the green that follows it.
- [ ] [#lzstructfieldreads] Typed bindings that decode a fixture block into a struct have a blind spot no map-based key tracker can see: an unused local will not compile, but an unused FIELD is silent, so the key counts as consumed at decode time and is then discarded. Found in lazily-go while closing #lzconsumednotasserted, where open_handles and schema_version were both decoded and never read - one of them stayed hidden precisely because its field name collided with a real method. The fix there is TestConformanceStructFieldsAreRead, a go/ast scan of the package's own test sources with a both-directions excuse allowlist. lazily-go is covered. Check whether lazily-rs (serde structs), lazily-cs, lazily-cpp and lazily-zig decode any fixture block into a typed struct rather than reading it through their key tracker, and port the equivalent scan wherever they do.
- [ ] [#lzassertwithseam] The non-equality escape hatch added under #lzconsumednotasserted marks a key asserted BEFORE it runs the caller's check, so a closure that ignores its argument marks the key without proving anything. Flagged by lazily-rs about assert_key_with, and the same seam exists in every binding's equivalent - it is the one place in the new guard held by discipline rather than by the type system, which is exactly the shape of defect this whole ladder exists to remove. Tighten it so the fixture value must actually be consumed: hand the closure a wrapper that records its own read and mark the key asserted only if that read happened, or narrow the API to a fixed set of comparison combinators. Mutation-check by writing a closure that ignores its argument and confirming the guard reddens.
- [ ] [#kny1] Please help me setup trusted publishing with npm.
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
- [ ] [#lzcoveragelayout] Restructure the coverage.json feature-table layout for a growing language axis per tasks/software/plan-lazily-coverage-table-layout.md. Measured: the feature column is 68% of row width at mean and 84% at max (feature text mean 86 / max 203 chars vs ~40 for all nine marks combined), so a 10th language adds ~4 chars to a row already 130-250 wide — the language axis is NOT the problem. Reject transposing (64 columns) and reject splitting by language: sync-coverage.mjs injects the same full matrix into all 12 targets, and per-language tables would destroy the cross-binding parity view that check-conformance-coverage.sh exists to protect. Phase 1 (land BEFORE the GDScript column, independent of the rest): add id + short label per row and move the 86-203 char normative prose to footnotes or into cell-model.md/protocol.md, taking the table from ~250 to ~60 chars wide so the layout scales past 15 languages. Phase 2 with it: split the '— absent OR not applicable' mark, since GDScript's parked thread-safe family is a different fact from unwritten work. Phases 3-4 (follow-up): author a family field — 54 distinct prefixes across 64 rows means families CANNOT be inferred from the current text — then a family x language roll-up with a stated roll-up rule. Layout and mark-value changes stay separate commits; sync-coverage.mjs --check stays the gate.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
