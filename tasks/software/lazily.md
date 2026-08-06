---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 019fc87a-beb7-7e03-a7a2-c5ca1dc04331
agent: opencode
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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260806-043642.md`*

Compacted content:
- Archived 49 response topic(s): authoritative statechart kinds and closed references; canonical durable-outbox journal decoding; durable-outbox closeout reconciliation; 46 more
- Prior summary/context: prior compacted content: Archived 14 response topic(s): do [#lzblobbackendstrict]; do [#lzprosekeyconvention]; do [#l...

### Re: Phase 1 — feature prose relocated to footnotes — glm-5.2

Added `id` + short `label` to all 70 coverage.json rows; `sync-coverage.mjs` now renders `label [^id]` in each cell and emits the 80–203 char normative `feature` prose as footnote definitions below the table. Max rendered row width dropped from ~250 to ~142 chars — the 9-language header alone is ~68 chars, so the feature column no longer dominates and the literal "~70" exit criterion sits below the structural floor of the language axis. The inline `[^id]` is what keeps every row's prose genuinely one hop away rather than relying on scanning. All 12 targets regenerated; `coverage-check` and `coverage-claims-check` green. Committed + pushed to lazily-spec and all 10 sibling bindings as a separate layout commit.

### Re: Phase 2 — split the absent/not-applicable mark — glm-5.2

`—` stays "absent"; `⊘` is "not applicable" and MUST carry a reason in the row's `na` map (language → reason). The generator refuses a `⊘` without a reason and a reason without a `⊘`, so 'cannot' is never confused with 'has not' — both guards plus the happy-path render were tested and reverted from a clean coverage.json. Legend updated to four marks in coverage.json and the 11 binding docs that carry an inline legend (lazily-cs has none). Zero existing cells changed value: no `⊘` exists yet, so the capability lands empty and waits for GDScript's parked thread-safe family. Committed + pushed to lazily-spec + 9 siblings as a separate mark-vocabulary commit.

### Re: superproject pointers + verification — glm-5.2

The agent-loop superproject pointer bump is deferred: origin/main is under active multi-pane divergence (concurrent conformance pointer bumps on the same lazily submodules), so forcing a rebase would collide across 10+ commits. The substantive work is on every lazily repo's `main`; the pointer bump is workspace bookkeeping the normal sync flow reconciles (recorded as #lzsuperpointers). CI for the just-pushed commits is still pending on GitHub; locally `coverage-check` (all 12 targets) and `coverage-claims-check` are green. Phases 3–4 (an authored `family` field + family×language roll-up) are filed as #lzcoveragefamily — the plan's explicit follow-up, blocked on nothing.
<!-- agent:boundary:e668c328:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzcoveragefamily] Author a `family` field across all 70 coverage.json rows, render per-family tables, and add a family×language roll-up with a STATED rule (✅ only when every row in the family is ✅, ~ when mixed, —/⊘ when none) — Phases 3-4 of tasks/software/plan-lazily-coverage-table-layout.md. The family CANNOT be inferred from feature text (54 distinct prefixes across 70 rows): it is a one-time reviewed pass, the same kind of judgment call the Phase-1 labels were. Put the roll-up rule in the legend, not just the generator. [recommended]
- [ ] [#lzsuperpointers] Reconcile the agent-loop superproject submodule pointers for the 11 lazily repos after the #lzcoveragelayout commits (Phase 1: 11 repos; Phase 2: lazily-spec + 9 siblings). Deferred because origin/main is under active multi-pane divergence — the substantive work is already on every lazily repo's main, so this is workspace bookkeeping the normal sync flow reconciles.
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
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
