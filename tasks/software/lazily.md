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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260730-171412.md`*

Compacted content:
- Archived 9 response topic(s): do [#designimplementtransport]; do [#lzingressbindings] using subagents; do [#lzcoverageaudit] + do [#lzzig0152]; 6 more
- Prior summary/context: prior compacted content: Archived 16 response topic(s): #lazilyrenameinverted; #lzinteropcs; #lzstdlibbarrier; 13 more
<!-- agent:boundary:796d47b1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 do [#lzscenariocoverage]
- do [#lzconsumednotasserted]
- do [#lzcpptranscribedrunners]
- do [#lzreactdtsgate]
- 📌 do [#lzegress]
- do [#lzqueuefamilyflavors]
- do [#lzwqfixtureconfig]
- do [#lzingressadapters]
- 📌 do [#kny1]
- 📌 Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#lzscenariocoverage] A fixture with multiple named scenarios can be PARTIALLY replayed and nothing notices. Confirmed this cycle: liveness_orset_lww.json carries four scenarios; lazily-dart replays three, lazily-rs and lazily-go skip derived_live_doc_aggregate_converges_under_retry entirely, lazily-cs replays all four. Both existing guards are blind to it by construction — the coverage guard asks only whether the FILE was opened, and the new key-consumption guard only binds blocks a runner actually reaches, so an unreplayed scenario contributes no unconsumed key. The fixture is counted as covered and its scenario is never run. Add a per-scenario ledger: the runner declares which scenario ids it replayed, the guard compares that against the ids present in the fixture, and a missing id is an error unless explicitly excused with a reason. Same both-directions rule as the fixture allowlist, so an excuse for a scenario that IS replayed also fails. Mutation-check per binding by dropping one scenario from a runner and confirming the guard reddens.
- [ ] [#lzconsumednotasserted] Key CONSUMPTION is not key ASSERTION, and the guards landed under #lzassertunknownkeys only prove the former. A runner can read a key and then do nothing with it: lazily-dart collections_extra_conformance_test.dart lines 459 and 477 continue/break past sibling_a_cached and downstream_consumer_reran, which counts as consumed under the tracker because the loop reads them. The same shape appears wherever a value is read into a variable that is never compared. This is the next level down from the fixture-opened / key-consumed ladder and it is where a green suite can still assert nothing. Audit each binding for read-then-discard: named skips inside a consuming loop, values bound but never compared, and assertion arms whose comparison is against a literal rather than the fixture value. Prefer a structural fix where one exists (require every consumed key to reach a comparison), and where it does not, make the named skip an explicit declared exception with a reason so it is visible rather than silent.
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
