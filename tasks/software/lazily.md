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
<!-- agent:boundary:a36b3233:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzwireencodingrunner]~~
- ~~do [#lzoutboxjournalop]~~
- ~~do [#lzdecorativewirefields]~~
- ~~do [#lzprotobufinterop]~~
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
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
- [ ] [#lzcoveragelayout] Restructure the coverage.json feature-table layout for a growing language axis per tasks/software/plan-lazily-coverage-table-layout.md. Measured: the feature column is 68% of row width at mean and 84% at max (feature text mean 86 / max 203 chars vs ~40 for all nine marks combined), so a 10th language adds ~4 chars to a row already 130-250 wide — the language axis is NOT the problem. Reject transposing (64 columns) and reject splitting by language: sync-coverage.mjs injects the same full matrix into all 12 targets, and per-language tables would destroy the cross-binding parity view that check-conformance-coverage.sh exists to protect. Phase 1 (land BEFORE the GDScript column, independent of the rest): add id + short label per row and move the 86-203 char normative prose to footnotes or into cell-model.md/protocol.md, taking the table from ~250 to ~60 chars wide so the layout scales past 15 languages. Phase 2 with it: split the '— absent OR not applicable' mark, since GDScript's parked thread-safe family is a different fact from unwritten work. Phases 3-4 (follow-up): author a family field — 54 distinct prefixes across 64 rows means families CANNOT be inferred from the current text — then a family x language roll-up with a stated roll-up rule. Layout and mark-value changes stay separate commits; sync-coverage.mjs --check stays the gate.
- [ ] [#lzdependencyavailability] Implement tasks/software/plan-lazily-dependency-reactive-availability.md: model every late-bound dependency as a stable per-key reactive with explicit unavailable/available state so Computeds subscribe before first publication; cover single-threaded, thread-safe, async, and binding parity, then remove Agent Doc compatibility workarounds.
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
