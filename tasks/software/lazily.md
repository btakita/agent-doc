---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 926da2b0-b539-4f91-8b48-3f9a0f4db425
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

> **Queue prompt:**
>
> do [#lzcoveragefamily]


The agent-loop superproject pointer bump is deferred: origin/main is under active multi-pane divergence (concurrent conformance pointer bumps on the same lazily submodules), so forcing a rebase would collide across 10+ commits. The substantive work is on every lazily repo's `main`; the pointer bump is workspace bookkeeping the normal sync flow reconciles (recorded as #lzsuperpointers). CI for the just-pushed commits is still pending on GitHub; locally `coverage-check` (all 12 targets) and `coverage-claims-check` are green. Phases 3–4 (an authored `family` field + family×language roll-up) are filed as #lzcoveragefamily — the plan's explicit follow-up, blocked on nothing.

### Re: do [#lzcoveragefamily] — Phases 3-4 landed — opus-5

All 70 rows carry an authored `family`; 29 families, no orphans. Names follow `conformance/` directory names wherever one exists — seven (`queue`, `topic`, `work-queue`, `crdt`, `transport`, `security`, `instrumentation`) are authored because no directory carries them. Splitting the three channel primitives out of `collections` was the load-bearing call: as one 20-row family it would have read `~` forever and told you nothing, whereas `collections` now isolates a real Zig gap while queue/topic/work-queue read green.

The roll-up regenerates from `rows` alone — no second hand-maintained source. Rule: `✅` only when every required row is `✅`, `~` when mixed, `—` when nothing is shipped or partial, `⊘` only when the whole family is inapplicable. It ships *inside* the injected block rather than in the 12 hand-written legend lines, so every target states it and it cannot drift from the generator.

**One finding worth your attention.** The first render had `Wire codec` at `~` for eight of nine bindings while every MUST row was green — dragged down by `frame-codec-postcard`, which the spec marks **MAY** and which is Rust-only by design. That is precisely the "requires a stated roll-up rule or it lies" failure the plan warns about, so optional rows are now excluded from the roll-up (`optional: true`, one row today, shown as *(opt)* in the detail table where it is still scored). `Wire codec` now reads `✅` except Dart's genuine `ipc-wire` gap. I did not author a full RFC-2119 axis across all 70 rows — one flag on the one MAY row is the honest minimum, and inferring normativity from prose is the drift this file exists to prevent.

What the roll-up surfaces at a glance: **Reliable sync** is `~` across all nine, **Wire codec** trails only on Dart, **Transport & FFI** on JS/Dart/C++, **Causal receipts** on Rust, **Keyed collections** and **Message passing** on Zig.

Guards added, each driven end-to-end and reverted from a clean `coverage.json`: undeclared family, row with no family, declared family with zero rows, absent `families`, duplicate family id. All six branches of the roll-up rule verified the same way, plus the all-optional-family fallback. No mark value changed. `make check` green (exit 0), `coverage-check` and `coverage-claims-check` green, lazily-spec CI green.

Committed + pushed as one schema+generator commit to lazily-spec (`9cd02ed`) and a regenerate-only commit to each of the 10 siblings. Superproject pointers stay deferred under `#lzsuperpointers`, which is the remaining queue head.

### Re: do [#lzsuperpointers] — superproject pointers reconciled — opus-5

The divergence that forced the deferral is gone. `origin/main` was 4 ahead / 0 behind when I checked, so no rebase was needed and nothing had to be forced.

What was actually outstanding was narrower than the backlog text implied: the Phase 1 and Phase 2 pointer bumps had already been reconciled by earlier superproject commits, leaving exactly one commit per repo — the Phase 3–4 `#lzcoveragelayout` family + roll-up commit. Eleven repos: `lazily-{cpp,cs,dart,formal,go,js,kt,py,rs,spec,zig}`. `lazily-react` was already clean.

Before committing I verified each move rather than trusting the `+` marker:

- every submodule is on `main` with `ahead=0 behind=0` against its own `origin/main`, so each new SHA is already published — the superproject is not pointing at a local-only commit;
- `merge-base --is-ancestor` confirms all 11 are fast-forwards, `+1` commit each, not a rewind.

Committed as `945d985175` with `git commit --only -- <the 11 paths>`. The shared index was empty at the time, but `--only` is what makes that irrelevant: another pane staging mid-commit cannot leak into this one. `git show --stat` confirms exactly 11 gitlinks and nothing else. Pushed to `origin/main` as a clean fast-forward (`76b8adec3d..945d985175`); `git submodule status` now shows no `+` on any lazily entry and the superproject reads `ahead=0 behind=0`.

Two things I did not do, deliberately. I left each submodule's working-tree churn alone — every lazily repo carries a modified `AGENTS.md` and an untracked `runbooks/` (plus a stale worktree marker in `lazily-py`); that is unrelated tsift-init drift, not part of this item, and the `-dirty` suffix in the diff does not affect the recorded gitlink. And I skipped `make check`: this commit changes no source, only gitlinks, and the substantive coverage work was already verified green in each lazily repo last cycle. Running the agent-loop suite here would have tested agent-doc, which this commit does not touch.

Warning worth acting on outside this queue: the live JetBrains plugin is 0.2.349 against a 0.2.350 build, so editor delivery ACKs are running on a stale plugin until you reload the IDE host.

`#lzcoveragereadmerollup` remains the queue head.

### Re: do [#lzcoveragereadmerollup] — adopted; detail now lives in one file — opus-5

**Decision: adopt it.** The 70-row table renders in exactly one place — `lazily-spec/docs/coverage.md` — and all eleven READMEs carry the family × language roll-up plus a link. Injected block per README: **323 lines → 39**. `lazily-zig`'s README went from 678 lines to 394; `lazily-spec`'s own from 542 to 258.

**The stated trade-off is smaller than the backlog claimed, and I checked before accepting it.** The item says row-level cross-binding parity "is the drift-detection view `check-conformance-coverage.sh` exists to protect." That is not what that script does. It guards *fixture replay* — it fails when the canonical corpus grows a fixture this binding's suite never opens, read from the runtime manifest, per-binding and single-language. It never looks at a README. The cross-binding claim guard is `check-coverage-claims.mjs`, which ties every wire-bearing `✅` to a fixture the binding actually replays. Both run in CI, on the data. So the README was never the drift detector; dropping row detail from it costs a reader a click, not a guarantee.

**The real reason to do it is the sync surface, which is the part worth stating plainly.** A row mark usually does not move its family's roll-up cell — flipping one row inside an already-`~` family changes nothing at family resolution. So the common `coverage.json` edit now rewrites *one* file instead of twelve, and a row-level change stops fanning a commit out across eleven repos. That directly attacks the hot-multi-session-file problem that made `coverage.json` edits expensive in the first place.

One thing that made the decision easier: every binding README *already* carried a hand-written link to the canonical matrix, one line above the table. The "plus a link" half of the proposal was already there — the 323 lines below it were redundant with it.

Implementation notes:

- One `renderTable(withDetail)` over `coverage.json`, no second source. Each variant is rendered **once** and injected byte-identically, so two READMEs cannot drift apart by construction.
- A guard rejects any `TARGETS` list declaring other than exactly one detail target — two detail copies would reintroduce precisely what this removes.
- The roll-up legend said optional rows are "shown as *opt* below". In summary mode there is no below, so that clause is now conditional.
- Nine READMEs opened with "The full `lazily` capability set across every binding" — false once the block is a roll-up. Corrected in all nine. `lazily-cs` and `lazily-formal` already worded it accurately and were left alone.
- The link is absolute, since ten of the eleven summary targets render on a different repo's GitHub page where a relative path resolves to nothing.

Verification. `docs/coverage.md` was **not rewritten** by the split — that is the useful check, since it proves the detail rendering is byte-identical to what was already committed and the change is confined to the summary path. Three guard branches driven end-to-end and reverted: a stale summary target (exit 1 — the new code path), a stale detail target, and a second `detail: true` target. `make check` green, exit 0 captured directly rather than through a pipe; `coverage-check` and `coverage-claims-check` green (84 shipped marks across 11 fixture-bearing rows, 9/9 binding ledgers read). Zero mark values changed.

Committed and pushed to all 11 repos, and I bumped the superproject pointers in the **same** cycle this time (`587192bb5e`) rather than letting them drift into another `#lzsuperpointers`. All 12 lazily submodules read clean.

lazily-spec CI (run `31244856337`) is still in progress — recorded as pending, not waited on; local evidence above is what this closeout rests on.

### Re: do [#lzgdbinding] — blocker resolved, Phase 0 landed — opus-5

> **Queue prompt:**
>
> do [#lzgdbinding]


You picked the **Godot 4.4** floor, which unblocked Phase 0. I then settled the question the plan made contingent on it and built the scaffold.

**gdUnit4 over GUT**, confirmed against 4.4 rather than assumed — v5.x+ lists 4.3/4.4/4.4.1 as supported. The plan's stated reason was "it has an existing CI action," but there's a much better one specific to this binding: gdUnit4 reports **orphan nodes per test**. The one novel problem here is that `RefCounted` has no cycle collector, so back-edges are `WeakRef` and `ctx.scope()` is the sole strong owner. Orphan detection turns "teardown actually frees" from an assertion nobody remembers to write into a property the harness checks on every run. GUT has no equivalent. Current suite: 4 cases, **0 orphans**.

Two corrections worth having on the record. I first pinned gdUnit4 at **v6.2.1** because that is what `plugin.cfg` on `main` says — it is unreleased and 404s as a tag. Latest released is **v6.2.0**, which is what is pinned, and the plan now says to read the version from releases rather than the plugin manifest. Separately, I pinned CI to Godot **4.4.1**, not to your local 4.7.1: the floor is a promise to consumers, and running CI above it is exactly how a 4.5+ feature lands unnoticed and breaks only in someone else's project. The addon states the same thing in code — `Lazily.MIN_GODOT` is asserted by a test, so moving the floor requires editing a test.

**The vacuity guard is the part I'd point at.** gdUnit4's CLI exits 0 when it runs zero test cases, so a misnamed directory or a suite that failed to parse reports OK over nothing at all — the `#lzvacuousrun` failure this family guards against everywhere else. `make check` reads the executed-test count and fails when it is zero or unreadable. That guard caught two real problems while I built it: the first regex never matched gdUnit4's actual summary format, and it failed closed rather than passing. I then drove it end-to-end — empty `tests/` exits 2 as *missing evidence*, a deliberately broken assertion exits 2, the restored suite exits 0 with 4 cases. A guard this family would not trust unmutated.

Two smaller things the harness needed, both non-obvious: an import pass must run before the suite or every `class_name` fails to resolve — including gdUnit4's own CLI entrypoint, which dies as a parse error that reads like a broken framework rather than a missing step. And gdUnit4 refuses headless runs unless told to; our tests use no UI interaction, so `--ignoreHeadlessMode` is correct rather than a workaround.

Layout keeps `addons/lazily/` as the entire shipping surface, with tests, the runner, and `project.godot` outside it. gdUnit4 installs to `addons/gdUnit4/` gitignored, so a consumer never receives a test framework. One test asserts the addon reads no `ProjectSettings` — that is the defect that works in this repo and breaks in every consumer project, so it is worth a test rather than a convention.

**I did not add a `coverage.json` column.** A column of `—` marks would claim a binding that does not exist; per the plan that lands in Phase 1 with rows 0 and 15, once the runner replays real fixtures.

**One thing I deliberately stopped short of.** The scaffold is committed locally (`3d8e90d`, 13 files) but has **no remote** — creating a public repo under the `lazily-hub` org is outward-facing and wasn't explicitly authorized, so I left it as `#lzgdremote` at the top of the backlog. Say the word and it's a minute's work, including the submodule wiring.

Closing `#lzgdbinding` and replacing it with per-phase IDs — `#lzgdremote`, `#lzgdkernel` (Phase 1), `#lzgdrunner` (Phase 2) — rather than re-gating one parent through five phases. `#lzcppwasm` remains queued.
<!-- agent:boundary:139c989e:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzgdbinding]~~
- do [#lzcppwasm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzgdremote] [operator-action] Create the lazily-hub/lazily-gd GitHub remote, push the Phase 0 scaffold (local commit 3d8e90d in src/lazily-gd), and wire it into the agent-loop superproject as a submodule + .gitmodules entry. Held back deliberately: creating a public repo under the lazily-hub org is outward-facing and was not explicitly authorized. Everything else in Phase 0 is done and verified locally; this is one go-ahead away.
- [ ] [#lzgdkernel] Phase 1 of tasks/software/plan-lazily-gd.md: the cell kernel (coverage row 0) plus the loading invariant. Source/Computed/Effect/Context with explicit ctx threading (no ambient, no signal/get_signal), WeakRef back-edges with ctx.scope() as the SOLE strong owner, edge index keyed on get_instance_id() with O(1) dedup and prune-on-propagate BEFORE any edge-count assertion, teardown in reverse creation order. An Effect created outside any scope must ERROR, not silently collect. Exit criteria include a load-graph test asserting the kernel loads no other family — preload() is eager and transitive, and retrofitting layering after six families exist is the expensive order. Remember 4.5's @abstract is unavailable at the 4.4 floor. Treat any nonzero gdUnit4 orphan count as a failure, not noise.
- [ ] [#lzgdrunner] Phase 2 of tasks/software/plan-lazily-gd.md: the conformance fixture runner plus the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd), landed in the SAME phase as the runner per the plan. Mutation-check the guard before trusting green — lazily-cpp's queue tests were hand-transcribed rather than replayed, and a source grep cannot see that case. Only once the runner replays real fixtures does lazily-gd earn its coverage.json column (rows 0 and 15, KNOWN_UNCOVERED populated and shrinking); adding the column before then would claim a binding that does not exist.
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
