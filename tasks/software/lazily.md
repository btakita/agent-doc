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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260801-230430.md`*

Compacted content:
- Archived 3 response topic(s): do [#bugcoinedpretooluse]; do [#coinedpretooluseguard]; do [#lazilycppexcuses]
- Prior summary/context: prior compacted content: Archived 14 response topic(s): do [#lzbenchbudgetheadroom]; do [#lzinteroppeerci]; #lzcheckc...

### Re: do [#coveragejsonscores] — a shipped mark now has to cite a fixture the binding replays, and three could not — opus-5

Done, pushed, CI green. lazily-spec `d0b3fc1`-range, kt `3072525`, superproject `543a06b96`.

**The rule.** A row MAY carry `"fixtures": [...]`. `scripts/check-coverage-claims.mjs` then refuses a ✅ on that row unless the binding replays **every** cited fixture — read from that binding's own committed ledger (`KNOWN_UNCOVERED`, plus `REQUIRED_AREAS` where it declares one), never from a claim in lazily-spec.

`~` is exempt on purpose. It is a *visibly qualified* mark and the honest place to record "implemented, but not this wire" — lazily-cpp's msgpack codec is exactly that. The mark that misleads a reader is the unqualified one, so that is the one that has to be backed.

**The audit found four unbacked claims across six annotated rows.** The item predicted cpp would not be the only one; it wasn't.

| Finding | What it actually was | Resolution |
|---|---|---|
| Kotlin ✅ on the json reference codec | `CodecConformanceTest.kt` **does** replay it — but `codec` was absent from lazily-kt's `REQUIRED_AREAS`, so deleting that runner would have left its guard green | fixed the guard; **the mark stands** |
| Dart ✅ on "IPC wire — Snapshot + Delta + CrdtSync" | `distributed/crdt_sync_frames.json` is in lazily-dart's `KNOWN_UNCOVERED` | downgraded to `~` |
| Rust ✅ on "Causal receipts" | `receipts/causal_receipts.json` — lazily-rs's own reason is "No runner at all" | downgraded to `~` |
| Zig ✅ on "Message-passing + RPC command plane" | all eight `message-passing/*` fixtures are in lazily-zig's `KNOWN_UNCOVERED` | downgraded to `~` |

The Kotlin one is the most interesting: it is the *same* hole cpp had before `#lzcppjsoncodec` — an area kept out of `REQUIRED_AREAS`, so a whole area could go unreplayed without appearing in any gap list. The replay existed and nothing required it.

**Proof the guard is not decorative.** Four mutation probes, all caught:

- restoring Rust's unbacked ✅ → flagged by name
- citing a fixture not in the corpus → flagged
- hiding one sibling checkout → `--require-all` refused the partial audit
- removing the codec fixture from Kotlin's manifest → kt's guard went red, so the new area requirement bites

**Wired in.** `make check` in lazily-spec, and CI, which clones the nine sibling ledgers sparsely. `--require-all` refuses to pass on a partial audit — a checkout that silently stops cloning one binding would otherwise keep reporting green for every claim it never read. **CI confirms `9/9 binding ledgers read`**, so the guard is really running there, not skipping. All five lazily-spec gates pass locally including Lean.

**Two things I deliberately did not cite, and why.**

`Reliable sync` (row 42) reads ✅ in **all nine** bindings, while `reliable-sync/coalesce_bounds_outbox.json` and `liveness_lease_eviction.json` are in **all nine** `KNOWN_UNCOVERED` lists. Citing it would flag every column at once. That is a real finding, but the resolution is a family-wide decision — implement the two fixtures nine times, or downgrade the row — not something to settle by quietly annotating a JSON file at the end of a long cycle. Filed.

Root-level fixtures (`arena_blob.json`, `snapshot_*`, `delta_*`) **cannot be cited at all**: they have no area, and the two area-scoped guards exclude them from their audit entirely, so the rule cannot answer honestly for them. That leaves "Shared-memory blob path" and the Snapshot/Delta half of the IPC-wire row still unbacked. I cited only area-qualified fixtures rather than let the guard return a confident wrong answer. Filed, together with a smaller one: Dart's `crdt_sync_frames.json` entry carries no reason of its own — it inherited the comment above it — in a ledger that requires one per entry.

Last cycle's lazily-cpp CI is green (`30720210025`). Exchange is large (queue active, so noting rather than compacting).

### Re: do [#lzcppmsgpackwire] — lazily-cpp now ships the wire the token names — opus-5

Done, pushed. lazily-cpp `18a25cd`, lazily-spec `72164b1`, nine regenerated coverage tables, superproject `89a0e89`. `make check` green in both repos; lazily-spec CI green (`30727880842`), lazily-cpp CI still running (`30727880531`) — reporting from local evidence.

**What was actually wrong was a claim, not a missing file.** `kDefaultCodec = "msgpack"`, and `interop_peer` said `"codecs":["msgpack"]`, while `codec.hpp` wrote `{"type": 0, "value": …}` with integer `kind` discriminators on `NodeState`/`IpcValue` and offered a positional array form protocol.md excludes outright. Nothing in the tree was false about *itself* — the header is a competent MessagePack codec — so a file-presence audit, a round-trip test, and `coverage.json` would all have read green. Only the token was a lie.

**I took both exits, because they are not alternatives.** `msgpack_codec.hpp` is the spec wire; `codec.hpp` keeps its job and stops calling itself the msgpack codec.

The new header derives its frame from `json_from_ipc_message` rather than transcribing the shape a second time. That is the whole design decision. The external tags, the field names, `NodeSnapshot`/`NodeAdd` omitting an absent `key`, and `CrdtOp` always writing it are then identical to the reference codec *by construction* — and a second hand-written transcription of the same shape is precisely what produced the divergence being fixed. The two codecs now differ only in how a value tree is serialized, which is the only thing a codec is allowed to differ in.

One decision worth naming: byte payloads encode as arrays of integers, and msgpack `bin` is **rejected** on decode. `rmp_serde` serializes `Vec<u8>` through serde's default seq impl, so the reference decoder refuses `bin` in that position. Accepting it would have made lazily-cpp read frames no conforming peer can produce — a private extension wearing the token again, one layer down.

**Mutation-checked, because a fixture round trip cannot see an encoding rule.** The decoder is symmetric with the encoder, so most encoding mutations get caught at decode and prove nothing about the assertions. Four that isolate:

| Mutation | Caught by |
|---|---|
| Objects packed as positional arrays | decode refuses — the envelope is no longer a one-entry map |
| `encode_msgpack`/`decode_msgpack` delegate to the private framing | the schema-less view, on the `bin` payload |
| One spurious field added to nested maps only | `encoded_body_field_names` |
| An absent `NodeSnapshot.key` written as `null`, msgpack path only | `first_node_encoded_field_names` |

The last two round-trip cleanly and pass every value assertion. They are the ones that mattered.

I also mutation-checked the ledger flip itself: re-adding `codec/frame_roundtrip_msgpack.json` to cpp's `KNOWN_UNCOVERED` reddens `check-coverage-claims.mjs` against the ✅ I just set. The mark is backed by cpp's own ledger, not by the annotation.

**Two things I changed beyond the item, and why.** `interop_peer`'s `normalize_msgpack` routed through the private codec while advertising the token, so it now routes through the spec wire and its self-check asserts the bytes are externally tagged; the old 4private-codec normalization survives under an honest name. And `json` moved out of that handshake's `carve_outs` into `codecs` — `json_codec.hpp` landed in `#lzcppjsoncodec` and replays the canonical corpus, so the carve-out had been false since then. Both are the same defect class as the item, in the same file.

**What I found and did not fix.** `CapabilityHandshake.codec` is a bare `std::string` compared for equality, and nothing in lazily-cpp maps that token to an encoder — lazily-rs has `Codec::encode`/`Codec::decode`, cpp has three unrelated free functions where the one named plain `encode` is the private framing. The wire is correct now; nothing yet *binds* a negotiated token to it, so the identical trap is still reachable one layer up. That is an API addition rather than a wire fix, so it is filed as `#lzcppcodecdispatch` at the top of the backlog rather than folded in here.

`coverage.json` row 37 now carries the distinction in its own text instead of a `~` legend no column uses, so the next binding to ship a private MessagePack codec reads why that does not earn the mark. Seven bindings still carve out `msgpack` — that is `#lzmsgpackseven`, next in the queue.

### Re: do [#lzcppcodecdispatch] — the negotiated token now picks the encoder — opus-5

> **Queue prompt:**
>
> do [#lzcppcodecdispatch]


Done, pushed. lazily-cpp `fef0ed2`, superproject `c2302ef`. `make check` green (53/53, interop self-check OK, 115 fixtures across 21 areas); CI queued (`30730012680`). Last cycle's `#lzcppmsgpackwire` CI came back green (`30727880531`).

**Fixing the wire without fixing the selection would have left the defect alive.** `#lzcppmsgpackwire` made the msgpack bytes correct; nothing *chose* them. `CapabilityHandshake.codec` was a `std::string` compared for equality, and the three encoders were unrelated free functions where the one named plain `encode` is the private framing. A caller that had just negotiated `msgpack` and reached for the shortest name still emitted the wrong wire.

`Codec` is now a closed enum over the tokens this binding speaks, and `codec_dispatch.hpp` turns the negotiated token into the encoder — `codec_encode`/`codec_decode` on the enum, `negotiated_encode`/`negotiated_decode` on the session, so a boundary that ran a handshake never re-decides what it already agreed on.

**Two absences are the design.** `postcard` is not in the enum: protocol.md defines it, a peer may legitimately offer it, and lazily-cpp does not implement it — so `codec_from_token("postcard")` returns nullopt and the handshake fails rather than resolving to a default. The private framing is absent for the opposite reason: it is not a codec token at all, and the whole point is that it stays unreachable from a negotiated one. `codec.hpp` keeps its real job (same-binding serialization, chiefly the durable outbox) and is simply never what a handshake selects.

Frames are `std::vector<uint8_t>` for every codec. `json` is text on the wire, but giving one codec a different return type would put the branch back at the call site, which is the thing being removed.

**Mutation-checked in both directions**, because "the encoder is right" and "the decoder refuses a foreign frame" fail independently — a dispatch that encoded correctly but decoded the private form would still let a peer push bytes nothing agreed on:

| Mutation | Caught by |
|---|---|
| msgpack arm points at the private encoder | the spec-wire assertion (`msgpack_to_json` on the frame) |
| encoder left correct, decoder falls back to the private framing on error | `test_private_framing_is_unreachable_from_a_negotiated_token` |
| `hello` advertises `postcard` | the interop self-check's exact-list guard |

That last one is new and worth naming: `interop_peer`'s advertised `codecs` list is a string literal, so it can drift from the enum silently — the same shape of defect as advertising a codec whose bytes are wrong, which is how this whole thread started. The self-check now pins the exact list and requires every token in it to resolve to something dispatchable.

`check_compatible` also names both sides on a codec mismatch now instead of returning a bare "codec mismatch", which an enum makes free.

Next in the queue is `#lzmsgpackseven` — the seven bindings that still carve out `msgpack` in their `KNOWN_UNCOVERED`.

### Re: do [#lzmsgpackseven] — the seven carve-outs are gone; msgpack is nine for nine — opus-5

Done, pushed. Seven binding commits — py `2a2e98a`, kt `5c647f6`, js `c6d86bc`, dart `18cc87d`, zig `9741f67`, go `989df01`, cs `5728adf` — each CI-green on its own SHA, plus lazily-spec `34d0841`, ten regenerated coverage tables, superproject `0b94af4`. Row 37 of `coverage.json` is now ✅ in all nine columns and `check-coverage-claims.mjs` went from 44 to 51 backed marks.

**Every binding converged on the same design without being told to pick it**, and it is the one thing worth carrying forward: derive the msgpack frame from the binding's *existing json value tree* rather than transcribing the shape a second time. That makes the external tags, the field names, and both `NodeKey` rules identical to the reference codec **by construction**. A second hand-written transcription of the same shape is precisely what produced lazily-cpp's divergent framing — the defect `#lzcppmsgpackwire` existed to fix. Seven bindings just avoided re-creating it by never writing the shape down twice.

**Nobody added a dependency.** Not one of the seven. The reasons were the same everywhere: every mainstream msgpack library in these languages encodes a byte string as `bin`, which this wire never carries, and none of them hand you the external-tag / named-field / omit-when-absent rules — you fight them for all three. C# is the sharpest case, where MessagePack-CSharp would have needed union attributes for polymorphism *and* a `byte[]` override, to produce ~200 lines of wire.

**The mutation results say something my own C++ cycle got slightly wrong.** I reported three isolating mutations there; five bindings independently found that only two of them isolate. A positional encoder dies at *decode* — a self-describing codec cannot read its own output — so it never reaches the byte-introspection assertions. Five agents reordered their runners to read the encoded bytes **before** decoding, so that mutation reports as an encoding defect instead of a decoder crash. That is a better runner shape than the one I wrote, and I have since seen it catch the right assertion in each of them.

The two mutations that genuinely isolate both round-trip **cleanly**:

| Mutation | Round trip | Caught by |
|---|---|---|
| spurious extra nested field | passes — unknown keys are ignored on decode | `encoded_body_field_names` |
| absent `NodeSnapshot.key` written as `null` | passes — the decoder reads null-key as absent | `first_node_encoded_field_names` |

Every value assertion stays green under both. That is the entire argument for asserting on encoded bytes.

**`MIN_FIXTURES` was wrong in my brief and six agents told me so.** That floor is a lazily-cpp construct; it does not exist in the others, and lazily-kt had *deleted* it deliberately — a floor of 117 against 131 replayed meant roughly fourteen replays could have stopped running with CI still green. What these bindings use instead is strictly stronger: per-fixture accounting that fails in **both** directions, so an excuse naming a fixture the suite did open is reported stale. Six agents declined to invent a number. lazily-cs added one at 136 — correctly, because it was the one binding whose guard could report "OK: 2/2" over a short corpus and could not fail on magnitude, which is `#lzvacuousrun` exactly.

**Two findings I am filing rather than folding in.** The go agent flagged that bridging through a JSON tree can lose a u64 node id through `float64`; it used `UseNumber`. Checking the others, lazily-js **fails closed** above 2^53 in msgpack — but its json decoder routes through `JSON.parse` and silently rounds. So one binding refuses a frame it cannot represent under one codec and quietly mangles the same frame under the other, and the one that lies is the *reference* codec. That is `#lzjsintwidth`. Separately, `#lzkeynullstrict`: zig's `NodeSnapshot.fromJson` rejects an explicit `"key": null`, which a serde peer that simply did not apply `skip_serializing_if` legally emits and `rmp_serde` accepts — stricter than the reference on a frame the reference produces. Both are decode-contract questions across all nine, not msgpack-wire bugs, so folding them in here would have widened a closed item.

Exchange is large; queue is active, so noting rather than compacting.
<!-- agent:boundary:e74e9299 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 📌 do [#lzreadmelinkrot]
- 📌 do [#lzvacuousrun]
- 📌 do [#lzscenariobodyskip]
- 📌 do [#lzspecscenarioids]
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
- [ ] [#lzjsintwidth] lazily-js answers the same out-of-range integer two different ways, and the reference codec is the one that lies. Found while reviewing the msgpack work in #lzmsgpackseven. NodeId is u64 in lazily-rs, so a peer can legitimately emit a node id above 2 to the 53. The new msgpack decoder in src/msgpack-codec.js FAILS CLOSED on it - Number.isSafeInteger guards on both the read and the write, erroring with integer exceeds the safe-integer range. The json decoder does not: IpcMessage.decodeJson routes through JSON.parse, which silently rounds to the nearest double and hands back a corrupted node id with no error. So the binding refuses a frame it cannot represent under msgpack and quietly mangles the same frame under json, and json is the REFERENCE codec every binding must speak. Work: decide the contract - either both codecs fail closed, or both carry large ids losslessly via BigInt or a string-tagged form - then make the json path match, and add a fixture or unit test carrying a node id above 2 to the 53 so the choice is executable rather than prose. Check lazily-dart at the same time, since Dart web compiles int to double and may have the identical split. Mutation-check by feeding a large node id through each codec and confirming the non-chosen behaviour reddens.
- [ ] [#lzkeynullstrict] The shared json decode path rejects an explicit key of null on NodeSnapshot, but a conforming peer may legally send one. Surfaced by the lazily-zig agent while mutation-checking #lzmsgpackseven, and it is not zig-only because it lives in the value-tree decode both codecs now share. protocol.md says a self-describing codec OMITS an absent optional NodeKey, and every binding's encoder does. It does not say a decoder may refuse the null form: a serde-based peer that simply did not apply skip_serializing_if emits key null, and rmp_serde's own decoder accepts it and reads it as absent. lazily-zig's NodeSnapshot.fromJson errors instead, which makes it stricter than the reference implementation on a frame the reference implementation produces. Note the asymmetry that makes this easy to get wrong: CrdtOp.key is ALWAYS written as null when unset and every decoder must read that back as absent, so the null form is already required reading one field over. Work: settle whether omit-when-absent is an encoder rule only, say so explicitly in protocol.md, then audit all nine decoders for NodeSnapshot and DeltaOp NodeAdd and make them accept both the omitted and the null form while continuing to emit only the omitted one. Add a fixture scenario carrying the null form so the leniency is replayed rather than asserted in prose.
- [ ] [#lazilystatetable] Evaluate and pilot typed Lazily state tables as pure Computeds over enum product states; preserve exhaustive transition coverage without a second runtime, sidecar, or imperative ACK path. Plan: tasks/software/plan-lazily-state-tables.md
- [ ] [#reliablesyncrow] The Reliable sync row in coverage.json reads shipped in ALL NINE bindings while reliable-sync/coalesce_bounds_outbox.json and reliable-sync/liveness_lease_eviction.json are in ALL NINE KNOWN_UNCOVERED lists. The row claims resync coordinator plus at-least-once durable outbox plus OR-set/LWW liveness, and the two canonical fixtures for the liveness clause are replayed nowhere. I left the row uncited under coveragejsonscores rather than flag every column at once, because the resolution is a family-wide decision and not a JSON annotation. Work: pick one - implement the two replays in all nine bindings and then cite the row, or downgrade the row to partial everywhere and cite it so the guard holds the weaker claim. Either way it must end with fixtures on the row, because an uncited wire-bearing row is exactly the blind spot this guard was built to close.
- [ ] [#rootlevelconformance] Root-level conformance fixtures cannot be audited by the coverage-claim guard. arena_blob.json, snapshot_*.json and delta_*.json carry no area, and the two area-scoped guards (lazily-cpp, lazily-kt) restrict their audit to REQUIRED_AREAS, so a root-level fixture is neither replayed nor listed in KNOWN_UNCOVERED there - the ledger simply cannot express it. lazily-cpp says so in a comment: root-level (8) snapshot/delta/arena_blob IPC wire fixtures, test_ipc.cpp hand-builds the equivalent structures. That leaves the Shared-memory blob path row and the Snapshot/Delta half of the IPC wire row unbacked, and I cited only area-qualified fixtures rather than let the guard return a confident wrong answer. Work: give the root corpus a real area in both area-scoped guards (or move those fixtures into an ipc/ area upstream), then cite the two rows. Second, smaller item in the same ledger family: lazily-dart lists distributed/crdt_sync_frames.json in KNOWN_UNCOVERED with no reason of its own - it sits under the comment belonging to the msgpack entry above it - in a ledger that requires a stated reason per entry.
- [ ] [#lzvacuousrun] Rungs 2, 3 and 4 all reason about fixtures the run OPENED, so a run that opens ZERO fixtures reports three greens over nothing. Found in lazily-py while closing #lzguardsnotinci and verified concretely there: CI=true pytest tests/test_star_import.py gave 3 passed with every conformance guard satisfied, because an empty opened-set has nothing to complain about. py closed it with a vacuity check in pytest_sessionfinish that fails when the opened set is empty under CI. The same shape appeared independently in lazily-cs as OK-over-nothing output - rung 1 prints coverage OK 0/0 for an empty corpus listing and rung 4 prints 0/0 scenarios across 0 opened fixtures, both exiting 0 - and in lazily-js, lazily-go, lazily-dart and lazily-rs as the coverage script exiting 0 with a SKIP line when the sibling corpus is absent. lazily-cpp is the one binding already immune, because the MIN_FIXTURES and MIN_SCENARIOS floors it added under #lzscenariocoverage fire on an empty manifest. Port a positive-evidence floor to the other eight: every guard must assert it examined a non-zero, expected-magnitude population before it is allowed to report OK. Mutation-check by pointing each suite at an empty corpus and confirming red.
- [ ] [#lzreadmelinkrot] [recommended] Two small README link defects survived the family cross-linking pass, plus one log-noise issue, all found while doing it. First, lazily-zig README line 7 links lazily-py as github.com/btakita/lazily-py, the pre-lazily-hub namespace; GitHub redirects it so it is not dead, but it is the last non-canonical family link in the tree and it will break if that redirect is ever reclaimed. Second, lazily-react's opening paragraph linked @lazily-hub/lazily-js as the relative path ../lazily-js, which only resolves inside a workspace checkout and was dead on GitHub - already fixed in that pass, but it is worth a repo-wide sweep for other relative cross-repo links, since relative links never resolve across repositories. Third, lazily-zig's make check prints a bare failed command line mid-run while the target still exits 0 and every subsequent step passes, which makes a green gate read as red and is exactly the kind of noise that trains people to skim past a real failure. None of the three is urgent; all three are cheap.
- [ ] [#lzscenariobodyskip] The scenario ledger books a scenario at the point the iteration helper YIELDS it, which cannot distinguish a body that ran from one that skipped past. lazily-py proved this empirically against the contract's own probe: with yield-time booking, a continue at the top of the loop body still booked the scenario and rung 4 stayed silent. lazily-kt independently flagged the same thing, and lazily-js hit a live instance - the anchored scenario of collections stableid_alignment.json matched neither arm of an if/else dispatch chain and replayed nothing, invisible to rungs 2 and 3 because key records are keyed by fixture plus block plus key, so a sibling scenario had already marked expect key_equal asserted for the whole file. lazily-py has the reference fix: yield a view that books on first read of the scenario PAYLOAD (steps, ops, frames, expect) and stays silent for label keys (id, name, description), which also stops a by-name lookup from booking every scenario it scans past. Port that booking rule to the other eight bindings, and give every scenario dispatch chain a fail-closed else so an unmatched shape errors instead of passing. Mutation-check with a no-op body, not just a skipped iteration.
- [ ] [#lzspecscenarioids] The shared corpus does not identify scenarios uniformly, which forces every binding's scenario ledger to carry a positional fallback. collections mergecell_algebra.json carries three scenarios with neither an id nor a name - they are distinguishable only by their policy field - so all nine bindings now book them as position #0, #1 and #2 and print a NOTE every run. Separately, 28 fixtures identify a scenario by name while the three stdlib fixtures use id. Positional ids are fragile in exactly the way this ladder cares about: reordering the array silently rebinds every excuse and every ledger entry to a different scenario. Add a stable identifier to the mergecell scenarios and settle on one spelling across the corpus, then drop the positional fallback from the nine ledgers so the fallback cannot quietly become load-bearing. This is a shared-corpus change - a mutation probe against lazily-spec reddens every binding at once, so use a scratch copy and the fixture-dir env var rather than editing in place, and land the corpus change before the nine ledger changes.
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
