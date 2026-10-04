---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
  codex: 01a0fa46-ac02-7680-9541-ac8de07305db
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
  '#fix-ci': Fix CI
  '#followup': Add follow-up work into the backlog + queue
  '#rebase-conflicts': rebase + fix merge conflicts
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20261001-192444.md`*

Compacted content:
- Archived 23 response topic(s): are the dependencies feature-gated; #lzktoptionaldeps protobuf codec gated, published graph pinned; #lzgooptionalpgx pgx gated by relocation, published graph now empty; 20 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): durable runtime admission closeout; root instruction-files ta...

### Re: advance review — gpt-5 · 2026-10-01T15:33:13-04:00

I audited all seven review gates in three parallel, read-only workstreams and advanced the items whose evidence changed.

Lifecycle updates:

- Resolved `#lzreactpublish`: npm reports `@lazily-hub/lazily-react@0.3.0` as the latest package; GitHub has a published `v0.3.0` release; and the peeled tag plus `main` both resolve to prepared commit `dc2198713cdce925bccb5651ae6229ed27cf5347`.
- Returned `#lzmacosrunner` to the actionable backlog and queue. Its pinned evidence lists 6 incomplete Darwin-asset release pairs, while the live probe now measures 16 after adding `v0.35.419` through `v0.35.428`. The task is limited to refreshing the evidence baseline and re-running its self-test/status probes; the Mac-only release or SSH decision remains operator-gated afterward.
- Returned `#lzambienttokenresolve` to the actionable backlog and queue. All eight patches still apply and all fake-publisher success/failure/empty probes pass, but the recorded `src/agent-doc/Makefile` preimage hash drifted. The task is limited to manually refreshing that evidence and re-running the proof without resolving credentials, applying patches, or invoking a publisher.

Four gates remain unchanged:

- `#lzreleasepins`: concurrent superproject sessions are still active and the root advanced during the audit, so the gitlink-only pin window is not settled.
- `#lzrustpublish`: crates.io still exposes `lazily` 0.58.0, with no `v0.59.0` tag or release; Trusted Publishing remains the external blocker.
- `#lzagentdocartifactpurge`: both dry-run choices remain fresh and no artifact was deleted; explicit destructive authorization is still required.
- `#lzgithubbudget20`: the billing/payment configuration remains unprobeable through the available API, while current Actions runs show execution is not hard-stopped.

Verification and provenance:

- `make review-gate-status` intentionally exited 2 and identified exactly two stale-evidence gates: `#lzmacosrunner` and `#lzambienttokenresolve`; its self-test passed.
- `npm view`, `gh release view`, and `git ls-remote` independently confirmed the React package, release, and exact peeled tag commit.
- `cargo search lazily --limit 1` confirmed crates.io still reports 0.58.0.
- The artifact-purge full and durable-only dry runs reported 0 deletions.
- Changed paths before this closeout: none; this cycle only mutates and commits the session document through agent-doc.
- Tests: no source or build surface changed, so no project test suite was required.
- Confidence: high.
- Escalation: none. The remaining operator decisions stay explicit in their review items.

### Re: #lzambienttokenresolve — gpt-5 · 2026-10-01T21:46-04:00

> **Queue prompt:**
>
> do [#lzambienttokenresolve]


Refreshed the stale `src/agent-doc/Makefile` evidence without applying the credential patch or touching credential values:

- Updated the recorded SHA-256 from `b6df1af1…` to the current `39f4ae18…`.
- Shifted the prepared patch hunk from `@@ -352,7 +352,10` to `@@ -361,7 +361,10`, matching the current `wheel: python-bootstrap-test` context.
- `make publish-credential-proof` passed with exit 0 and zero failures. It exercised all eight publish sites using fake tooling, recording 21 fake `passage` lookups and 13 fake publisher invocations.
- `make review-gate-status` intentionally exited 2 because `#lzmacosrunner` remains stale, but it reports `#lzambienttokenresolve` as `blocked_fresh`: 8/8 preimages match, 8/8 patch sets apply, proof exit 0, and the plan remains unapplied.
- No credential was resolved or printed, no prepared patch was applied, and no publisher was invoked.

The evidence-only change is committed as `6e57996f3` (`docs: refresh ambient credential proof evidence`). Changed path: `tasks/software/plan-ambient-publish-token-point-of-use.md` only. `git diff --check` passed. The superproject has no GitHub Actions workflow named `CI` and `gh workflow list` returned no workflows, so there was no CI run to inspect.

`#lzambienttokenresolve` is re-gated on the remaining operator-only quiet-window patch application, Cargo/npm/PyPI credential rotations, and real interactive-shell name-only verification described in the plan. No additional actionable backlog follow-up is needed because those remaining steps require operator action and are retained in this review gate.

Confidence: high. Escalation: operator action only; no agent-side escalation occurred.

### Re: #lzmacosrunner — gpt-5 · 2026-10-01T22:06-04:00

> **Queue prompt:**
>
> do [#lzmacosrunner]


Refreshed the macOS release-gap evidence to the live measurement without building or uploading any Darwin artifact:

- Extended `MACOS_INCOMPLETE_PAIR_BASELINE` from 6 to 17 releases by adding `v0.35.419` through `v0.35.429`.
- Advanced the self-test's synthetic widening release from the now-baselined `v0.35.420` to `v0.35.430`, preserving the negative control that proves a newly incomplete release makes the gate stale.
- `make review-gate-self-test` exits 0.
- `make review-gate-status` exits 0 and reports `#lzmacosrunner` as `blocked_fresh`: the measured list exactly matches all 17 pinned releases, the weekly window is open, and the gap is not widening. The aggregate reports no stale gate evidence.
- The first full `make check` run exposed one `corky-core` library-test failure without a captured failing assertion. The exact 14-test target passed with the required CUDA feature and one test thread; a complete root `make check` rerun then exited 0 with zero failures. No Corky file was changed.
- `git diff --check` passed. This superproject has no GitHub Actions workflow named `CI`, so there was no CI run to inspect.

Changed path: `scripts/review-gate-status.py` only. Commit: `a5bd438c1` (`test: refresh macOS release gap evidence`). Per the session-document commit boundary, the ordinary repository push follows this agent-doc closeout and will be verified immediately afterward.

`#lzmacosrunner` is returned to review, blocked solely on the unchanged operator choice: run `make release-macos-assets TAG=<tag>` on a Mac or supply a Mac build host. No additional actionable backlog follow-up is needed because the remaining step requires operator Mac access.

The exchange is at the session-accretion warning threshold; the active queue was not stalled for compaction.

Confidence: high. Escalation: operator Mac access only.

### Re: #releasepublish — gpt-5 · 2026-10-01T22:50-04:00

> **Queue prompt:**
>
> do [#releasepublish]


No—not as a blanket release and publish operation yet.

The live evidence has four concrete blockers or scope mismatches:

- The superproject's `make publish` is the wrong release surface for this question: it publishes `instruction-files`, then `agent-doc` and `corky`; it does not publish the Lazily language packages.
- Rust is not ready to publish from this checkout. crates.io and the latest GitHub release both remain at `lazily` `v0.58.0`; `src/lazily-rs` is nine commits past that tag and its working tree is not clean. The prepared `v0.59.0` publication remains review-gated on crates.io Trusted Publishing.
- The superproject's Lazily gitlinks all differ from the currently checked-out submodule commits, so the existing `#lzreleasepins` quiet-window gate is still material.
- `make review-gate-status` now reports `#lzmacosrunner` stale again because `v0.35.430` added an eighteenth release with an incomplete Darwin asset pair. The credential plan remains safely unapplied and operator-gated as well.

Recommendation: do not invoke the root publish target and do not create a broad release. First refresh the `v0.35.430` macOS evidence, settle the intended Lazily gitlink pins, configure crates.io Trusted Publishing for `lazily-rs`, and choose an explicit repository/version set. Then release each selected package through its own verified workflow.

I returned `#lzmacosrunner` to the actionable queue with the new 18-release evidence-refresh target. The release-readiness question itself is complete.

Changed paths before closeout: none. Verification: `make review-gate-status` self-test passed and the aggregate intentionally exited nonzero for exactly the newly stale macOS gate; `cargo search`, GitHub Releases, `git describe`, repository status, and submodule status supplied the remaining evidence. No tag, release, credential, or publisher was invoked.

Confidence: high. Escalation: operator selection of the intended repository/version set plus the existing external gates.

### Re: harness switch to claude — opus-5.5 · TIMESTAMP

Saw the frontmatter change `agent: codex` → `agent: claude`. This session is now running on Claude Code (opus). No other prompt changed and the queue is not active, so there's nothing else to do. Add a prompt when you want the next lazily task started.

### Re: queue edit — `do [#lazilyintermediatelangua]` removed — opus-5-5 · 2026-10-03T01:00-04:00

You removed the `do [#lazilyintermediatelangua]` queue head while this turn was starting, so I stopped before doing any work on it. No repo changes were made. The backlog item `#lazilyintermediatelangua` stays open and unchanged.

Your edited queue prompt, "Should lazily have an intermediate language and use code generation?", is next in the queue. I'll answer it on the next drain cycle with the `#spec-test-commit-push` preset.

### Re: should lazily have an intermediate language + code generation? — opus-5-5 · 2026-10-03T01:08-04:00

> **Queue prompt:**
>
> do [#lazilyintermediatelangua]


> **Queue prompt:** Should lazily have an intermediate language and use code generation? See sdk.md.

**Short answer: split lazily in two.** The data plane should get a shared intermediate model with generated code. The reactive kernel should stay hand-written in each language. This is the same split haiven-sdk ended up with (`codegen/ir.py` → `ContractModel` → one `Backend` per language, `#sdkcodegenir`). But only half of lazily looks like an SDK, so only that half should copy the approach.

**Why sdk.md's answer carries over only partly.** haiven-sdk is almost entirely a *contract surface*: types, vocabularies, and REST/WebSocket operations taken from schemas the SDK doesn't own. Its runtime parts (session, handshake) stayed hand-written on purpose. lazily is the reverse. Most of its value is runtime semantics:

| Layer | What it is | Same in every language? | Generate it? |
|---|---|---|---|
| **Kernel**: `Source`/`Computed`/`Effect`, invalidation, batching, teardown scopes, the edge index | how updates propagate | the *rules* are the same, but the *code* depends on ownership, GC, threads, and async in each language | **no** |
| **Data plane**: msgpack/protobuf codecs, receipts, reliable-sync and durable envelopes, delta formats, error/operation vocabularies | bytes on the wire | yes, by definition | **yes** |
| **Conformance harness**: fixture loaders, step dispatch, assertion-block decoding | test plumbing | mostly the same | **yes, the scaffolding** |

**Why not generate the kernel.** A lazily IL that compiles the kernel into ten languages would fail three ways:

1. **Every binding's hard bugs were language-specific, not semantic.** Examples:
   - go's map data race (`#v0.23.2`), where reads ran off the context lock;
   - the lazily-py mypyc `.so` shadowing;
   - zig's posix moves;
   - GDScript's rule that `ctx.scope()` must be the *only* strong owner of effects;
   - the "invalidate outside the storage lock" rule in the queue family.

   Each of these is a lowering decision per language. An IL can't make that decision for you; it only moves it into a backend template that is harder to read than the code it replaces. ThingML's conclusion, cited in sdk.md, applies directly: generated code earns trust only if it is readable and fully customizable.
2. **Being idiomatic is the product.** Every binding exists so that people working in that language get native types, native async, and native ownership. A shared kernel template tends to produce the same code in every language.
3. **You already have the semantic layer that matters, and it isn't an IL.** `lazily-spec` (`cell-model.md`, the schemas) defines the rules. `lazily-formal` (Lean) proves them. The 222-file conformance corpus checks every binding against them. An IL would be a fourth description of the kernel, and the three you have are already hard to keep in step (see the `#lazilyspec` notes on fixtures reddening all nine bindings).

**Why the data plane should be generated.** Every binding hand-writes the same wire code today:
- rs: `protobuf.rs`, `receipt.rs`, `reliable_sync.rs`
- js: `msgpack-codec.js`, `protobuf-graph-boundary.js`
- go: `msgpack_codec.go`, `causal_receipts.go`, `reliable_sync.go`
- kt: `MsgpackCodec.kt`, `ProtobufGraphBoundary.kt`
- cpp: `codec.hpp`, `msgpack_codec.hpp`, `receipt.hpp`
- zig, dart, cs: their own equivalents

That's roughly 9 copies of the same schema-shaped code, the exact case where sdk.md applies TeaDSL's "M gateways × N languages → M + N" argument. The defects I've recorded in this area are the kind a shared model catches once instead of nine times:
- a >2^53 value that a JSON fixture can't carry as a number (wire as text/hex, expect as decimal);
- the rung-0 bind ledger's "declare from the bytes, never stringify";
- object-valued assertion keys that live only in `expect` blocks.

**What I'd build** (filed as recommended `#lzwiremodel`, appended to the queue):
- One language-neutral model in `lazily-spec`, built from `schemas/*.json` and `proto/`. It needs the type kinds the wire actually has: scalar, list, record, open enum, optional vs absent, and u64-as-text.
- The model committed as a golden `model.json`. Then a schema change shows up as one readable diff before any binding is touched (the same idea as haiven-sdk's `#sdkirsnapshot`).
- Thin per-binding backends for codec types, vocabularies, and fixture-loader scaffolding. Hand-written code calls into what they generate.
- **A small first step, not a rewrite:** one surface (receipts or the graph-boundary proto) in two bindings (rs plus one GC binding). It passes if conformance results are byte-identical and a generated file replaces a hand-written one. Grow it only if that removes more code than it adds.
- Keep the generator in Python inside `lazily-spec`, which already runs Python for provenance and inventory checks.

**When to revisit the kernel-IL question:** if a new binding turns into a mechanical port of an existing one (lazily-gd is 1.8k lines next to roughly 25–65k elsewhere), that's the moment to see whether a *template* of an existing binding is cheaper. Even then I'd generate a starting point and keep the result hand-maintained, not regenerate it on every change.

No repo changes this cycle; it was a design question. I marked `#lazilyintermediatelangua` done because this answers it.

### Re: #lzwiremodel, shared wire model + codec generation — opus-5-5 · 2026-10-03T15:50-04:00

> **Queue prompt:**
>
> do [#lzwiremodel]

**Done.** The receipts surface is now generated from one shared model in two bindings, Rust and Go. In each binding the generated file replaces hand-written declarations, and every test suite gives the same results. Pushed to all three repos:

| Repo | Commit | What changed |
|---|---|---|
| lazily-spec | `9ccc96a` | generator, golden model, check, CI step, docs |
| lazily-rs | `9a1ca9e` | `src/generated/receipts.rs` replaces the hand-written declarations in `src/receipt.rs` |
| lazily-go | `7fbfe4f` | `receipts_wire_gen.go` replaces the hand-written declarations in `causal_receipts.go` |

**How it works.**
- **Model.** `scripts/wire_codegen.py` reads the schemas listed in `codegen/surfaces.json` and builds one language-neutral model. It handles the type kinds the wire actually uses: string, u64, bool, list, nullable, record, closed enum, and externally-tagged envelope. It also tracks whether a field must always be present (and is `null` when empty) or may be missing.
- **Golden file.** The model is committed as `codegen/wire-model.json`, so a schema change shows up there as one readable diff. Every generated file's header carries the model's hash.
- **Backends.** The Rust and Go backends emit only declarations and codecs. Behaviour stays hand-written in each binding: constructors, `is_terminal`, and `ReceiptProjection`.
- **Fails closed.** Generation stops with an error on any schema shape it can't reproduce exactly: a non-local `$ref`, an integer without `minimum: 0`, an open object, a missing `description`, an enum value without docs, or a wire name that isn't a valid identifier. Open enums and may-be-missing fields are modelled, but neither backend emits them yet, so they're refused too.
- **Checks.** `make wire-codegen-check` is now part of lazily-spec `make check`. It fails if the golden model is out of date or if any checked-out binding's generated file differs by a single byte. A new lazily-spec CI step runs the same check with `--require-all` against each binding's published `main`; that ordering is why the bindings were pushed first. `make wire-codegen` regenerates everything.
- **Schema edit.** `schemas/receipts.json` only gained descriptions plus `x-lazily-enum-docs` and `x-lazily-envelope` annotations, which the generated doc comments need. The wire format is unchanged.

**The model caught a real mismatch on its first surface.** Go stored a receipt's `generation` as `int64`, so it accepted negative values the schema forbids (`minimum: 0`). Rust already used `u64`. Go now decodes it as `uint64`, which is an API change for Go callers. Go's command plane still uses `int64` generations everywhere, so I didn't spread the change through it. Instead, `receiptGenerationMatches` compares the two without wrapping either side, and the stale-generation status reports a large value capped at `MaxInt64` rather than wrapping. New Go tests pin the behaviour:
- the canonical fixture round-trips byte for byte;
- a negative generation is rejected;
- an unknown outcome is rejected;
- an empty batch encodes as `[]`, not `null`;
- the generation comparison never wraps.

**Verification.**
- **lazily-spec** `make check`: exit 0, including 17 new generator tests.
- **lazily-go** `make check`: exit 0, run again after rebasing onto the remote's 3 new commits.
- **lazily-rs** `make check`: exit 0 (917 test binaries ok, 0 failed). After rebasing onto the v0.59.0 version bump, I re-ran only the receipt-affected tests: the receipt unit tests, `command_conformance` (it decodes receipt frames) and `schema_compliance`. All passed.
- **CI:** lazily-go is green. lazily-spec and lazily-rs runs were still going when I closed out.
- **Shared working trees:** I committed only my own paths. The rebases first checked that each local-only commit matched its remote copy and that the other sessions' uncommitted README edits matched the remote, so those edits survived untouched.

**What this step doesn't settle:**
- **Lines of code.** On one surface, the generator (~560 lines) is bigger than the ~170 hand-written lines it replaced. It only pays off as more surfaces and bindings come on board.
- **Rust conformance.** Rust still doesn't replay `causal_receipts.json`; it remains in the known-uncovered list. Adding that replay means re-pinning the corpus-floor counts, which belongs with the next step.
- **Decode strictness.** Rust and Go still accept unknown keys and a missing `reason`/`payload_hash`, even though the schema forbids both. Enforcing that would change how they decode, so it's a decision for you, not a side effect of this change.

All three are in the new recommended `#lzwiremodel2`, appended to the backlog.

The exchange is 171 lines, over the 160-line warning point. I'm not stopping the queue for it; it can be compacted whenever you want.

### Re: #lzwiremodel2, Python backend, Rust receipt replay, strict decoding — opus-5-5 · 2026-10-03T17:58-04:00

> **Queue prompt:**
>
> do [#lzwiremodel2]

**Done.** The item had three parts, all completed. On the fourth, I decided not to grow the generator yet. Pushed to every repo; lazily-spec went last, because its CI checks each binding's published `main`.

| Repo | Commit | What changed |
|---|---|---|
| lazily-py | `77150de` | `_receipts_wire_gen.py` (generated) replaces the hand-written `ReceiptOutcome` / `CausalReceipt` / `CausalReceipts`; their non-wire behaviour moved to `_receipt_semantics.py` |
| lazily-rs | `e5564bd`, `549b95d` | Rust replay of `receipts/causal_receipts.json`, plus strict decoding |
| lazily-go | `c186ff0` | strict decoding |
| lazily-spec | `f3699e8` | Python backend, strict decoding in all three backends, CI fix, coverage, docs |
| 11 bindings | coverage sync | Rust's Causal receipts mark: `~` → ✅ |

**1. Python is the third binding on the receipts surface.**
- **Exports unchanged.** The generated types are still exported from `lazily` and `lazily.ipc`.
- **Behaviour.** Python can't add methods to a generated class from another module the way Rust `impl` blocks and Go methods can. So the generator makes each generated class inherit from a hand-written mixin, named in `surfaces.json`. That's where `is_terminal`, `group_by_causation`, `encode_json` and `decode_json` now live.
- **Envelope.** Python's frame type has always carried the `{"CausalReceipts": …}` tag itself, so the single-variant envelope lowers onto that record and its API is unchanged.
- **Size.** lazily-py `ipc.py` lost 141 hand-written lines. In their place are 81 lines of hand-written behaviour and 171 generated lines.

**2. Rust now replays `causal_receipts.json`.** A subagent did this in its own worktree.
- **What it checks.** The test decodes the fixture through `ReceiptMessage` and fails unless re-encoding gives back the same JSON. It then folds every receipt through the real `ReceiptProjection`, and checks each assertion key against the projection's own results.
- **Coverage.** The fixture left `KNOWN_UNCOVERED`; lazily-rs now opens 157 of 162 fixtures (was 156).
- **Not vacuous.** Breaking the stale-generation rule made the test fail. So did each of 7 changes to the fixture.
- **Floor pins.** No pin needed a new number: the guards derive their counts from the corpus at run time.
- **Coverage mark.** The coverage-claims guard then flagged Rust's `~` mark as indistinguishable from shipped, so I promoted it to ✅ and synced the coverage table into all 11 sibling repos.

**3. Decode strictness: I decided for strict, in every generated decoder.** A closed record now rejects an unknown key, and a required field must be present even when its value is `null`.

*Why strict:*
- Every receipt in the corpus has exactly the 7 schema keys, so no fixture changes.
- Every binding's encoder always emits `reason` and `payload_hash`.
- The spec already ruled this way for the blob `backend` discriminator (`#lzblobbackendstrict`): rejecting is conforming, and the lenient "forward-compat" decoders were the defect.

*How each backend does it:*
- **Rust:** `serde(deny_unknown_fields)`, plus a `required_nullable` deserializer, because a bare `Option` silently defaults a missing key to `None`.
- **Go:** a generated `UnmarshalJSON` checks the exact key set first. `encoding/json` on its own ignores unknown keys and matches keys case-insensitively, so `"Reason"` used to be accepted.
- **Python:** `from_wire` checks the key set and each value's JSON type, and range-checks `generation` as a u64.

*Behaviour change for callers:* Python used to accept a frame with no `receipts` list, and Rust and Go used to accept a missing `reason` or `payload_hash`; all of these now fail. It's recorded in the lazily-go and lazily-py changelogs.

**4. No second surface yet.** The rule was to grow only if the generator removes more code than it adds.
- **The count so far:** about 960 generator lines in total, against about 310 hand-written lines removed across Rust, Go and Python.
- **No cheap candidate.** I ran each candidate schema through the model. None lowers today:
  - `reliable-sync` and `delta` need multi-variant tagged unions.
  - `message-passing` needs newtype aliases.
  - `snapshot` lacks descriptions.
- **The better next step** is a fourth binding on receipts, which needs only a new backend. I filed it as recommended `#lzwiremodel3`, appended to the backlog. The full analysis is in `docs/wire-codegen.md` under "When to add a surface".

**CI fix found on the way.** lazily-spec CI clones lazily-py with only `scripts/` checked out. The generated-file check would have reported the Python file as missing and turned the build red. I added `src/lazily` to the sparse checkout and replayed that CI step against all three published `main`s: exit 0.

**Verification:**
- **Full `make check`, all exit 0:**
  - lazily-spec: run again after the CI and coverage edits.
  - lazily-py: 1178 passed, run again on the commit as rebased onto the remote's 0.42.0 release prep.
  - lazily-go: green.
  - lazily-rs: 7223 passed, 0 failed, with the replay and strict decoding together.
- **Strictness tests fail without strictness.** In Rust, Go and Python, I removed the strict check and confirmed the new tests failed, then restored it.
- **CI:** the lazily-spec, lazily-rs, lazily-py and lazily-go runs were queued or in progress at closeout.

**Shared working trees.** I committed only my own paths, and moved the shared checkouts forward only after confirming that each overlapping local edit was byte-identical to the remote. Where a checkout couldn't fast-forward (gd and zig, which held other sessions' changes, and kt, which has an unpushed local commit), I pushed the one-line coverage change from a scratch worktree at `origin/main`. Those three local histories are untouched.

The exchange is now about 280 lines. I'm not stopping the queue for it.

### Re: #lzwiremodel3, Kotlin on the receipts surface — opus-5-5 · 2026-10-04T00:38-04:00

> **Queue prompt:**
>
> do [#lzwiremodel3]

**Done.** Kotlin is now the fourth binding on the receipts surface. I picked Kotlin over js because Kotlin's extension members let the hand-written behaviour sit beside generated classes without mixins. In js, the codec sits inside the `index.js` monolith next to a hand-written `index.d.ts`, so it costs more. Pushed kt first, then spec, because spec CI checks each binding's published `main`.

| Repo | Commit | What changed |
|---|---|---|
| lazily-kt | `fd31508` | Generated `ReceiptsWireGen.kt` replaces the hand-written declarations and codec in `Receipt.kt`. `generation` is now `ULong`. Decoding is strict. |
| lazily-spec | `b97c278` | Kotlin backend, 2 generator tests, CI sparse checkout of lazily-kt, docs |

**How the Kotlin backend lowers the model:**
- **Types.** `u64` becomes `ULong`, a record becomes a `data class`, an enum becomes an `enum class` (`wireName` plus a `fromWire` that rejects unknown values), and the envelope becomes a `sealed interface` that includes `encodeJson`/`decodeJson`.
- **Behaviour stays hand-written** in `Receipt.kt`. That covers `isTerminal`, the `observed`/`accepted`/`applied`/`rejected` factories, `ofCausalReceipts`, `ReceiptApplyStatus` and `ReceiptProjection`.
  - The factories are now extensions on the generated companion object, so `CausalReceipt.observed(...)` reads the same as before.
  - `isTerminal` is now an extension property, so Java callers would reach it differently. In Kotlin it reads the same.
- **Formatting.** The generated file goes through lazily-kt's `spotlessCheck` like any other source. The backend emits exactly the layout ktlint produces, so a formatter change shows up as generated-file drift.

**Lowering found a real mismatch, the same one Go had.** Kotlin stored `generation` as `Long`, so it accepted negative values the schema forbids and couldn't hold anything above 2^63-1. It's now `ULong`, which is an API change for Kotlin callers: `ReceiptProjection.observe` and `ReceiptApplyStatus.StaleGeneration` take `ULong`. The command plane still uses `Long`. As in Go, `receiptGenerationMatches` compares the two without wrapping, and a receipt generation too large for a `Long` is reported as `Long.MAX_VALUE` instead of wrapping.

**Decode strictness now matches rs/go/py.** The old Kotlin decoder accepted all of these, and the new one rejects them:
- unknown keys;
- a missing `reason` or `payload_hash`;
- a number where a string belongs (`jsonPrimitive.content` reads either);
- negative generations.

The new decoder also rejects fractional, quoted or out-of-range generations, empty ids, and an envelope with zero or several tags.

**Verification:**
- **lazily-kt `make check`:** exit 0, run again after rebasing onto the remote's v0.43.0 prep. 518 JUnit tests, 0 failures.
- **New Kotlin tests** cover:
  - the canonical fixture re-encodes byte for byte;
  - a generation of `ULong.MAX_VALUE` round-trips;
  - 12 off-schema frames are rejected;
  - an empty batch encodes as `[]`;
  - the generation comparison never wraps.
- **Not vacuous.** With the unknown-key check removed from the generated decoder, the strict test failed; it passed again after restoring.
- **lazily-spec:** the generator tests pass (22). I replayed the CI wire-codegen step against sparse clones of all four published `main`s: exit 0.
- **lazily-spec `make check`:** everything passes except `coverage-check`, which reports stale coverage tables in the local lazily-zig and lazily-gd checkouts. Those checkouts are 4 commits behind origin and hold other sessions' edits to files origin also changed, so I didn't move them. My change doesn't touch `coverage.json`.
- **CI:** the lazily-spec and lazily-kt runs were still in progress at closeout.

**Shared working tree.** The local kt checkout had an unpushed commit, `a342adf`. It was already on origin as `f79a8de`, and the one other-session edit that overlapped origin (`scripts/check-conformance-coverage.sh`) matched origin byte for byte. So I committed only my 5 paths and rebased with autostash, and the rebase dropped the duplicate. The tsift `AGENTS.md` churn in the checkout is untouched.

**The line count still doesn't favour the generator.** The Kotlin backend is about 330 lines. It replaced 177 hand-written lines, and 58 of those came back as hand-written behaviour. A binding gains strictness and a single source of truth, not fewer lines. The step that could start removing more than it adds is modelling multi-variant tagged unions, which `delta` and then `reliable-sync` need. I filed that as recommended `#lzwiremodel4`, appended to the backlog. `docs/wire-codegen.md` has the updated count and reasoning.

The exchange is 282 lines, over the warning point; I'm not stopping the queue for it.

### Re: #lzwiremodel4, multi-variant unions and the delta surface — opus-5-5 · 2026-10-04T00:46-04:00

> **Queue prompt:**
>
> do [#lzwiremodel4]

**Done.** The wire model now handles multi-variant tagged unions, and `delta` is the second surface. It's modelled only for now: the golden model is checked, but no binding generates from it yet. Pushed as lazily-spec `feb96c9`. The receipts output is byte-identical in all four bindings.

**Modelling `delta` needed more than unions.** Each item below was a separate gap, and the generator fails closed on each until it's modelled:

| Kind | Where `delta` needs it |
|---|---|
| `union`: closed single-key objects with a PascalCase tag, plus string-`const` unit variants | `DeltaOp` (10 variants), `IpcValue`, `NodeState` (`"Opaque"`) |
| references into `defs.json`, pulled in transitively | `NodeId`, `NodeKey`, `IpcValue`, `NodeState`, `ShmBlobRef` |
| `alias` for named scalars | `NodeId` = u64, `NodeKey` = string with a length range and a pattern |
| `bytes` (integers 0..=255) | inline payloads |
| named inline enum, field `default` | `ShmBlobRef.backend` (default `shm`) |

Every backend refuses these kinds as "modelled but not yet lowered". The schema edits are documentation only: descriptions, the `DeltaFrame` envelope annotation, and docs for the `backend` values.

**Proof the model reads `delta` the same way the schema does.** A new test reads every `{"Delta": ...}` frame in the conformance corpus two ways: through a strict reading of the model, and through the JSON Schema validator.
- It covers 21 frames plus 338 single-edit variants of them, such as a dropped key, an unknown key, a lowercase tag, a bad `key`, a byte of 256 or a `backend` of `rdma`. 35 of those variants are schema-valid.
- The two readings must give the same verdict on every one, and they do.
- **Not vacuous:** making the model drop `pattern`, or treat bytes as a list of u64, each produces a disagreement the test reports.

**Fixed: the model builder failed open.** A schema root without the `x-lazily-envelope` annotation was skipped silently, so `signaling.json`, `durable-envelope.json` and `lossless-tree-delta.json` each "modelled" as zero types. The builder now refuses such a root, and refuses any schema that yields no declarations. Both cases have regression tests.

**Found: queue ops in deltas are decoded by only one binding.** `protocol.md` makes `QueuePush`, `QueuePop` and `QueueClose` ordinary `DeltaOp` variants, and only lazily-cs decodes them. The other nine bindings reject a schema-valid `Delta` that carries one. The corpus has `QueuePush` frames in one reliable-sync fixture, and nothing for `QueuePop` or `QueueClose`. I filed this as actionable `#lzdeltaqueueops` at the top of the backlog. It needs codec changes in nine repos plus a new fixture, and a new canonical fixture reddens every binding until each one handles it.

**What lowering `delta` still needs:**
- **Codec-dependent `key`.** lazily-rs omits an absent `NodeAdd.key` in self-describing codecs and keeps it in positional Postcard. A generated serde derive can't express that.
- **Null `backend`.** The schema rejects `backend: null`, but `protocol.md` requires decoders to read it as `shm`. A decoder generated from the model alone would refuse it.
- **Union lowering in every backend.**

I filed this as recommended `#lzwiremodel5`, appended. The other candidate schemas each still fail on one specific shape; `docs/wire-codegen.md` lists them, since the old list was out of date.

**Verification:**
- **lazily-spec:** 371 tests pass, including 33 generator tests. `make -k check` fails only at `coverage-check`, which reports stale coverage tables in the local lazily-zig and lazily-gd checkouts. Those checkouts are still 4 commits behind origin and hold other sessions' edits to files origin also changed, so I didn't move them.
- **lazily-rs `make test-schema-compliance`:** exit 0 against the edited `defs.json`/`delta.json`. No binding keeps its own copy of either schema.
- **CI:** last cycle's lazily-spec and lazily-kt runs are both green. This push's lazily-spec run was in progress at closeout.

The exchange is 331 lines; I'm not stopping the queue for it.

### Re: #lzdeltaqueueops, queue op-log DeltaOps family-wide — opus-5-5 · 2026-10-04T01:17-04:00

> **Queue prompt:**
>
> do [#lzdeltaqueueops]

**Done.** Every lazily binding with an IPC codec now decodes and encodes `QueuePush`, `QueuePop` and `QueueClose`. lazily-gd has no IPC codec, so it needed nothing.

I didn't add a new fixture, which would have meant a ten-repo registration change. Instead, the `Delta` scenario in both `codec/frame_roundtrip_json.json` and `frame_roundtrip_msgpack.json` now carries all ten ops: `op_count` is 10 and `op_variants` has the three new names. Each binding's existing round-trip runner became the test, and fixture and scenario counts didn't move. Following the `#subagents` preset, I gave each of the eight bindings that needed code changes its own subagent and sandbox, outside the IDE-watched tree. I did lazily-cs myself and integrated serially. The bindings were pushed first and lazily-spec last.

| Repo | Commit | Notes |
|---|---|---|
| lazily-rs | `6d78383` | Postcard tags 7/8/9 appended, so existing tags are unchanged. The bridge hub's catch-all became explicit arms that refuse queue ops. Mirrors resynced. |
| lazily-go | `0faed4e` | JSON + msgpack, read filter, spill |
| lazily-py | `2cdadee` | Mirrors resynced |
| lazily-kt | `27a570e` | `GraphView.applyDelta` refuses queue ops before applying anything |
| lazily-js | `ff93020` | `.d.ts` updated; `GraphView` refuses queue ops and now also refuses unknown ops instead of skipping them. Size budget 2460 B → 2520 B (see below). |
| lazily-dart | `645a65f` | Mirrors resynced |
| lazily-zig | `70029b0` | `StateGraphMirror` refuses queue ops; every `switch` names all ten variants |
| lazily-cpp | `ea83fca` | All 4 codecs; a compile-time variant-count check makes every encoder handle a new op |
| lazily-cs | `10449d3` | Already decoded them; the runner's `OpVariantOf` only knew seven |
| lazily-spec | `75117f5`, `1c50800` | Fixtures, docs |

**Semantics, following lazily-cs:**
- **Read filtering:** checked per node, like `Invalidate`.
- **Blob spill:** a large `QueuePush` payload spills and resolves like a `CellSet` payload; `QueuePop` and `QueueClose` carry no bytes.
- **Graph projections refuse queue ops loudly.** Where a binding has a graph projection that applies deltas, it now rejects queue ops with "requires a queue projection adapter" instead of skipping them. go, py, dart and cpp have no projection that applies deltas, so there was nothing to refuse there; nobody invented one.
- **Unknown keys:** no binding rejects unknown keys for the existing ops, so the new ops weren't made stricter than the rest.

**Verification:**
- **Full `make check`, exit 0 in every repo:**

  | Repo | Tests passed |
  |---|---|
  | rs | 7257 |
  | go | 918 |
  | py | 1188 |
  | kt | 526 |
  | js | 701 |
  | dart | 761 |
  | cpp | 66 |
  | cs | 333 |
  | zig | 1137 on 0.16.0/master, 1075 + 4 skipped on 0.15.2 |

- **Mutation check:** in each of the nine, removing the `QueuePush` decode arm reddened the codec round-trip conformance test, and restoring it turned it green.
- **Mirrors:** I ran lazily-spec's mirror check against the pushed rs, py, dart, go and zig trees: 135 vendored fixtures matched byte for byte.
- **CI:** spec, rs, go, py, kt, js, zig, cpp and cs are green on the pushed commits. lazily-dart's first run failed its mirror-drift test. That was ordering: Dart CI ran before spec was published and compared the new mirrors against the old canonical fixtures. I re-ran it after the spec push, and the re-run was in progress at closeout.

**For you:**
- **js bundle size:** `make check` failed on the size budget for the `index: PROTOCOL_ID + Snapshot` bundle, which grew 60 bytes. The budget was raised to 2520 B, as the last two wire changes did. Revert that in `.size-limit.json` if you'd rather shrink the bundle.
- **`delta_sequential.json`** still says "all 7 DeltaOp variants". That's true of its graph ops, and zig now scopes its `has_all_op_variants` check to those seven. I left the wording alone, because rs, py and dart mirror that file byte for byte.
- **Disk:** the rs sandbox build output went to `~/.cache/claude-qops` (73 GB), because `/tmp` filled up. All sandbox worktrees are removed. That cache directory is safe to delete, but deletion is blocked for me outside `/tmp`, so please run `rm -rf ~/.cache/claude-qops`.

The exchange is 374 lines; not stopping the queue for it.

### Re: #lzwiremodel5, union lowering + delta generated into lazily-py — opus-5-5 · 2026-10-04T01:34-04:00

> **Queue prompt:**
>
> do [#lzwiremodel5]

**Done.** The Python backend now generates unions, and the `delta` surface is generated into lazily-py. It's the first surface where the generator removes more hand-written code than its backend adds. lazily-py was pushed first, then lazily-spec.

| Repo | Commit | What changed |
|---|---|---|
| lazily-py | `4d8e2ac` | `_delta_wire_gen.py` (generated) replaces about 440 hand-written lines in `ipc.py` |
| lazily-spec | `955a14f` | Python union lowering, `delta` target, tests, docs, provenance |

**Why Python first.** lazily-py's hand-written IPC types already had the shape a generated union takes: `DeltaOp_CellSet(node, payload)`, `IpcValue_Inline(data)`, `NodeState_Opaque()`. Its `NodeId` is also a plain `int`. Kotlin types `NodeId` as `Long`, so it would first need the same `ULong` migration receipts needed. Rust keeps or omits `NodeAdd.key` depending on the codec, which a generated serde derive can't express.

**How Python lowers a union:**
- **Classes.** A base class, plus one frozen, slotted dataclass per variant.
- **Payloads.** A record payload is flattened into its variant, so `CellSetBody` becomes `DeltaOp_CellSet(node, payload)` and the body record is never emitted. Any other payload becomes one field, named by the target (`variant_fields`: `data`, `blob`).
- **Unit variants** encode to the bare tag (`"Opaque"`).
- **Two new target options:**
  - `external`: a type stays hand-written. The generator imports it and never emits it, nor anything reachable only through it.
  - `envelope: false`: lazily-py's own `IpcMessage` carries the `{"Delta": ...}` tag, so no envelope class is emitted.
- **The backend also lowers aliases and optional fields.** `NodeId = int` is range-checked. An optional `key` is omitted on encode when absent and read as absent from either an omission or `null`.

**What stays hand-written in lazily-py:**
- **`NodeKey` and `ShmBlobRef`** are `external`, because each has a rule beyond its schema. `NodeKey` enforces byte and segment bounds. `ShmBlobRef` must read `backend: null` as `shm`, which the schema rejects (`#lzblobbackendstrict`). Both moved to `_wire_scalars.py` so the generated module can import them without a cycle, and `ipc.py` re-exports them.
- **Constructors, `IpcValue.of`, per-peer read filtering and the epoch decision** live in `_delta_semantics.py`, as mixins.
- **Public names, field order and constructor signatures are unchanged.**

**Decoding is now strict.** It rejects:
- unknown keys;
- a missing `ops` list, which used to default to empty;
- a node id or epoch that isn't an unsigned integer (including a bool) or is past 2^64-1;
- a byte outside 0..=255, or base64 where bytes belong;
- `{"Opaque": ...}` for the unit variant;
- a non-string `NodeKey`.

Each rejection is a `ValueError` naming the field; a missing field used to raise `KeyError`. It's recorded in lazily-py's CHANGELOG.

**Line count.** About 440 hand-written lines were removed. 171 lines of behaviour moved to `_delta_semantics.py`, and the Python backend grew about 360 lines. Python is the only binding generating `delta`, so this is the first surface that clears the "removes more than it adds" test.

**Verification:**
- **lazily-py `make check`:** exit 0, 1207 passed. That covers format, lint, the `ty` type check, tests, conformance coverage, interop peer and ci-reach. New tests cover the strict refusals, the absent and `null` key forms, the lenient `backend: null`, the node-id range check, and that the behaviour moved to `_delta_semantics.py` still works.
- **Generated code vs schema.** A new spec test executes the generated Python module on every corpus `Delta` frame and 338 single-edit variants. The two hand-written types are replaced by test stand-ins that follow the schema exactly. Each verdict must match the schema's, and each accepted frame must re-encode byte for byte; an explicit `key: null` re-encodes omitted.
- **Not vacuous:**
  - Removing the unknown-key refusal from the generator makes that test fail, and removing it from the generated file makes the lazily-py tests fail.
  - One mutant survived, harmlessly: widening the generated byte check to 256 changes nothing, because Python's `bytes()` itself refuses 256.
- **lazily-spec:** 372 tests pass. `wire_codegen --check` is green across all five generated files against the local siblings. Receipts output is byte-identical.
- **Spec `make check` also caught a leftover from the last item:** `provenance/fixtures.json` was stale after the round-trip fixtures gained the queue ops, and spec CI doesn't run that gate. I regenerated it in this push.
- **CI:**
  - lazily-dart's re-run from `#lzdeltaqueueops` is now green.
  - The lazily-py and lazily-spec runs for this push were in progress at closeout.

**Next:** recommended `#lzwiremodel6`, appended. It covers union lowering for Go and Rust, starting with Go because Go has no codec-dependent fields, and then generating `delta` into lazily-go. `docs/wire-codegen.md` lists what each backend still needs.

The exchange is past 430 lines; not stopping the queue for it.
<!-- agent:boundary:cfd052cf -->
<!-- /agent:exchange -->
 
## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzwiremodel6] [recommended, lazily-spec] Sixth wire-model step: union lowering in the Go and Rust backends (Go first: no codec-aware fields), then lower delta into lazily-go with ShmBlobRef/NodeKey external as in Python. Rust needs a codec-aware optional (NodeAdd.key omitted in self-describing codecs, kept in Postcard); Kotlin needs the NodeId Long->ULong migration first. Design: src/lazily-spec/docs/wire-codegen.md (When to add a surface).
- [ ] [#lzmacosrunner] [operator-decision, agent-doc/lazily-py] Refresh newly stale macOS coverage evidence: add v0.35.430 to MACOS_INCOMPLETE_PAIR_BASELINE and update the recorded review list from 17 to the currently measured 18 incomplete-pair releases; advance the widening self-test fixture beyond the baseline, run make review-gate-self-test, make review-gate-status, and the full root make check, commit and push the evidence refresh, then return this item to review blocked solely on Mac release access.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzambienttokenresolve] Refresh the point-of-use credential plan's stale evidence for src/agent-doc/Makefile: update the recorded preimage SHA-256 and hunk offsets by hand, then rerun make publish-credential-proof and make review-gate-status until #lzambienttokenresolve is blocked_fresh. Do not resolve or print credential values, apply the prepared patches, or invoke a real publisher. After evidence is fresh, re-gate on the remaining operator-only quiet-window application, Cargo/npm/PyPI credential rotations, and real interactive-shell name-only verification in tasks/software/plan-ambient-publish-token-point-of-use.md.
- [/] [#lzreleasepins] After the other superproject sessions settle, update only the Lazily gitlinks to released commits, rerun the exact-revision interop gate, and push the intended pin commit without unrelated workspace changes.
- [/] [#lzrustpublish] [operator-decision] Configure crates.io Trusted Publishing for lazily-hub/lazily-rs workflow release.yml; then tag and publish the prepared v0.59.0 commit ea2910d and verify crates.io. Blocked solely on the missing publisher configuration.
- [/] [#lzgithubbudget20] Set the btakita personal Actions product budget to $20/month with Stop usage enabled; retain $0 hard-stop budgets for Packages, Codespaces, Git LFS, and AI credits. NOT probeable from here by construction, and `make review-gate-status` now records it as `unprobeable` rather than re-measuring it every cycle: /users/btakita/settings/billing/actions and /users/btakita/settings/billing/usage both return 404 with a `user`-scope hint for this token, and the REST billing API exposes no payment-method field even with that scope. Actions execution itself is not blocked — recent agent-doc CI runs start and pass — so this is spend-limit hardening, not an outage. Blocked until a valid payment method is added to the personal GitHub account. No agent-executable slice.
- [/] [#lzagentdocartifactpurge] [operator-decision, btakita/agent-doc] Read-only audit with counterpart durability classification (commits 667eb4766, 434b32548) lives at src/agent-doc/tasks/agent-doc/artifact-purge-audit-2026-09-27/ and is confirmed the ONLY handoff on disk, so only one basis can be authorized. Run the plan from src/agent-doc with `python3 scripts/purge-actions-artifacts.py`; the destructive run is `--execute --authorize-deletion btakita/agent-doc --max-deletions 3000` and the 250 default cap otherwise aborts. TWO authorizable options, both re-proved by live dry run: (a) full plan — 2,666 artifacts (43.83 GB), 0 refused, 0 skipped; (b) `--durability durable` — 2,136 artifacts (34.83 GB), refusing exactly the 530 rows whose only counterpart is a PyPI file. Prefer (b): the prune floor is 0.35.113 and v0.35.119, the oldest PyPI-only-backed version, is only the THIRD live release above it, so option (a) additionally accepts that 9.01 GB rests on a thinner margin than version numbering suggests. 2,136 rows rest on GitHub release assets, which have not decayed. 571 artifacts are withheld at audit time and are never deletable. Hard deadline: the 7-day `--max-handoff-age-days` guard aborts the whole run after 2026-10-04T05:56:08Z unless the audit is regenerated or `--allow-stale-handoff` is passed; `make review-gate-status` imports that guard from the purge script and flips this gate to `evidence_stale` 24h BEFORE it expires rather than after. Blocked solely on explicit destructive authorization; no artifact has been deleted.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
