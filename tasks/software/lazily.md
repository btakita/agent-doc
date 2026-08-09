---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 2c6171b1-ef84-46fb-bafc-fd78406d58f9
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

### Re: do [#lzgdremote] — repo created, CI green on the floor — opus-5

> **Queue prompt:**
>
> do [#lzgdremote]


`github.com/lazily-hub/lazily-gd` is public, `main` is pushed, and it's wired into the superproject as a submodule (`5e11ace633`). CI is **green on Godot 4.4.1**: 4 test cases executed, 0 orphans.

**Wiring `.gitmodules` needed care.** It already had an uncommitted `src/haiven-dev` stanza from another pane — and `src/haiven-dev` is a committed gitlink in HEAD with *no* `.gitmodules` entry, so that pane is mid-repair of a genuinely broken state. Committing the file wholesale would have taken their fix. I committed HEAD's content plus only my `lazily-gd` stanza, then restored the full working-tree version, so their edit is still sitting uncommitted and still theirs. Verified after the fact: the committed `.gitmodules` has zero `haiven-dev` lines, the working tree has them back.

**Then CI failed twice, and both failures were worth having.**

The first was `set: Illegal option -o pipefail` — make invokes `/bin/sh`, which is dash on Ubuntu. This wasn't cosmetic: the suite pipes Godot through `tee`, so without `pipefail` the recipe reports *tee's* exit code and a failing test run reads as success. That's the same false-green the vacuity guard exists to stop, arriving by another route. Fixed with `SHELL := /bin/bash` and proven by forcing the run to fail (`make test GODOT=false` now exits 2). Invisible locally because Arch's `/bin/sh` is bash.

The second was mine, and it's the more useful correction. **I pinned gdUnit4 wrong.** I told you last cycle I'd confirmed gdUnit4 against the 4.4 floor — that check was real but I then pinned v6.2.0, and upstream's compatibility row listing `v4.3/v4.4/v4.4.1` covers the **v5.x** line. On 4.4.1, v6.2.0 fails to resolve its own classes and then *hangs instead of exiting*, so the job ran 3+ minutes with nothing to show. I cancelled it rather than let it burn the limit, downloaded 4.4.1 locally, and reproduced the hang exactly — then verified **v5.1.1** passes on both 4.4.1 and 4.7.1. That's the pin now.

Two lessons went into the code rather than just this message: verify a framework pin against the **floor** engine, not the one you happen to run (4.7.1 hid this completely), and read the released tag rather than `plugin.cfg` on `main` — which says `6.2.1`, unreleased, and 404s. Both are comments at the pin.

I also bounded both Godot invocations in `timeout`. A hang is a distinct failure mode from a red test, and an unbounded CI job reports nothing at all. Guard driven: `TEST_TIMEOUT=0.1` exits 2, normal run stays green. My first attempt at that check used `TEST_TIMEOUT=1` and passed — the whole run takes 0.83s, so the test was inconclusive rather than the guard being sound.

Queue head is now `#lzgdkernel` (Phase 1 kernel), then `#lzgdrunner`, then `#lzcppwasm`.

### Re: do [#lzgdkernel] — Phase 1 cell kernel landed — opus-5

> **Queue prompt:**
>
> do [#lzgdkernel]


`Source` / `Computed` / `Effect` / `Context` / `Scope` are implemented under `addons/lazily/cell/`. 23 tests, 0 orphans, green on **both** Godot 4.4.1 (the CI floor) and 4.7.1. CI green: `59a6f94`, superproject bumped at `2094c945a4`.

**The ownership model is the whole design here.** Back-edges are `WeakRef`, forward edges strong, because `RefCounted` has no cycle collector and a bidirectional strong graph would leak every graph ever built. That has a consequence I made structural rather than documentary: since a `Scope` is the *only* strong owner of an Effect, `ctx.effect()` refuses outright and tells you to use `ctx.scope().effect()`. An unowned Effect wouldn't error — it would be collected and silently stop running, which is the worst failure shape available.

Edge index keys on `get_instance_id()` for O(1) dedup, which is safe specifically because Godot ids are monotonic and never alias. Dead refs are pruned inside `_live_dependents()` before any count is read; GDScript has no finalizer hook, so an unpruned count reads high — that's why `dependent_count()` is the only sanctioned way to ask.

**One test earns its place over the obvious one.** The four-deep chain test reads each level on the way down, which can *mask* a one-level-mark defect by refreshing the chain as it goes. So there's a second test that reads only the deepest node after a write. That's the one that would actually catch the hybrid-staleness failure where writes vanish at depth 2.

**A deviation worth flagging:** the tracked read is `read()`, not `get()`. `Object.get(property)` is a Godot builtin, and shadowing it would break `obj.get("prop")` for every consumer holding a lazily handle. `read()` is already the reference binding's name for the tracked read, so this costs nothing cross-binding — but it *is* a deviation from the plan's wording and I'd rather you see it than find it.

**The load-graph check needed rethinking mid-build.** I first reached for `ResourceLoader.get_dependencies()` and it returns `[]` for GDScript — it models `.import`-style resource deps, not `class_name`/`extends`/`preload` edges. Grepping source for `preload(` proves only what's written. So it now *measures*: a dedicated headless process loads the kernel entry and asserts via `has_cached()` that no non-kernel file became resident. Running it outside the suite is load-bearing — inside, everything is already resident and the check would describe the harness instead of the kernel. It also refuses to pass vacuously when there's nothing outside the closure to assert against. Driven both ways: a cross-family `preload()` fails it, removing it passes.

**My own bug, caught by my own test:** `.eager()` scheduled a recompute but nothing flushed it, so an eager cell sat stale until some unrelated write happened to flush. Eager now settles immediately outside a batch, and with the batch inside one.

Still deliberately **not** a `coverage.json` column. A mark has to be backed by a fixture the binding replays, and the runner is Phase 2 — claiming a column now is exactly what `check-coverage-claims.mjs` exists to catch. Queue: `#lzgdrunner`, then `#lzcppwasm`.

### Re: do [#lzgdrunner] — Phase 2 runner landed; the corpus found four kernel bugs — opus-5

The runner replays **9 of the 21 canonical `reactive-graph` fixtures** — every one whose ops the Phase 1 vocabulary covers. 25 tests, 0 orphans, green on 4.4.1 and 4.7.1. CI green (`a2155ba`), superproject bumped (`62e9d55975`).

**The corpus found four kernel bugs my own tests missed.** This is the argument for replaying fixtures rather than writing more unit tests, so it's worth being specific:

- **Disposal never invalidated dependents.** A computed kept serving a value derived from a disposed cell, so the read *succeeded* instead of reporting `read_after_dispose`. Disposal looked like it worked.
- **Disposal and writes used the same invalidation path.** They are not the same: a write is a value change and must re-run effects; a dispose must invalidate caches *without* making surviving effects fire. `disposal_does_not_run_surviving_effects` is exactly that distinction, and I'd collapsed it.
- **`disarm()` deferred releasing ownership to teardown.** The fixture asserts owned-count is zero *at the disarm*, with members still live and wired. Mine dropped them later.
- **`_flush()` had no re-entrancy guard.** An effect that wrote started a nested drain which consumed the queue the outer loop was bounding, so a non-settling graph spun instead of reporting exhaustion.

I also had to make read failures observable through the context. `push_error` is invisible to a caller, so `expect.error` had nothing real to assert against.

**Two of my own mistakes, both caught by driving the thing:**

The guard's first version listed families to *exclude*, and it was already wrong — it missed 40 fixtures because I guessed directory names instead of deriving them. Rewrote it to name what the binding **implements**; everything else is excused automatically. An exclusion list needs editing every time the corpus grows, and the edit that never happens is the one that turns a gap green.

And the Phase 2 commit **failed CI** — the workflow checked out `lazily-gd` alone, with no `lazily-spec` sibling. That failure was the guard working: it hard-fails under CI on a missing corpus precisely so "replayed zero fixtures" can't report OK. Fixed by checking out both repos side by side.

**Verification worth naming.** The guard is mutation-checked in four branches (empty manifest → missing evidence; a manifest entry naming no real file → corrupt evidence channel; a dropped replayed fixture → unexcused; restored → passes). More importantly the *replay* is perturbation-checked: making invalidation one-level-only reddens `transitive_invalidation_reaches_depth` at depth 2 and beyond while depth 1 stays correct. A replay that survives that perturbation isn't replaying anything.

**One thing I deliberately did not do, and it's a blocker rather than remaining work.** I did not add the `coverage.json` column, even though my own backlog item scoped it here. `check-coverage-claims.mjs` treats *absence* from a binding's `KNOWN_UNCOVERED` as "replayed", while this guard excuses 128 fixtures by family — so the column would let lazily-gd carry marks it hasn't earned, inside a CI-gated shared repo. Filed as `#lzgdcoveragecolumn` with the fix stated. Shipping a known-unsound claim into shared CI is the one thing this whole family of guards exists to prevent.

Queue: `#lzgdcoveragecolumn`, `#lzgdvocab`, `#lzcppwasm`.

### Re: do [#lzgdcoveragecolumn] — the contract first, then the column — opus-5

`GDScript` is the 10th column in `coverage.json`: **`~` on Reactive graph, `—` on the other 28 families**. `make check` green in both repos, CI green in both (`8afbc27` spec, `d88243d` gd), superproject bumped at `e5d85bbce3`.

**The blocker was real, and it was worse than "the formats don't line up."** `check-coverage-claims.mjs` knew two ledgers — `KNOWN_UNCOVERED` and `REQUIRED_AREAS` — and lazily-gd declares a third. To a parser that never read `IMPLEMENTED_FAMILY_PREFIXES`, this binding looked like *"audits the whole corpus, declares 12 gaps"*, so all **128 family-excused fixtures counted as replayed**. An unread scoping ledger doesn't weaken the audit visibly; it silently **inverts** it for one binding.

I proved that rather than asserting it, in a scratch tree with only the gd sibling present: a GDScript `✅` on the `codec/frame_roundtrip_json.json` row **passes** the pre-fix parser and **fails** after. Also driven both ways — an in-family *replayed* fixture still passes, an in-family fixture named in `KNOWN_UNCOVERED` still fails. Four branches, not one.

**The more useful half of the fix is the one that isn't about lazily-gd.** Reading one more array name would leave the next binding free to invent a fourth narrowing mechanism and have it ignored the same way. So an **unrecognized ledger array in any binding guard is now a hard failure**: `SCOPING_ARRAYS` / `NON_SCOPING_ARRAYS` classify the eight that exist across the ten guards, and anything else stops the build with instructions. Driven by adding a bogus `SOME_NEW_SCOPE=(...)` to the gd guard and confirming red. Violation messages now also name *which* ledger excluded the fixture — three different files fix the three cases, and "does not replay X" sent you to the wrong one.

**Why `~` and not `✅` on row 0.** The runner replays 9 of 21 `reactive-graph` fixtures. Row 0's own prose says "all cells guarded", and `failed_compute_is_never_cached.json` — the fixture that proves a failed compute isn't cached — is one of the 12 it doesn't replay. `signal_materializes_*` (that's `computed().eager()`) and `churn`/`recycled_id` are also row-0 semantics and also unreplayed. `~` is documented as the visibly-qualified mark the guard leaves unverified by design; it's the honest cell here, and `✅` would have been exactly the unearned claim this whole guard family exists to stop.

**Two things worth flagging.**

Thread-safe rows are `—`, not `⊘`. The plan resolved them as "parked / out of scope for a first release" and pointed at `#lzcoveragelayout`'s not-applicable mark — but `⊘` means *structurally cannot*, requires a recorded `na` reason, and Godot does have threads. "Has not" and "cannot" are the distinction that mark exists to preserve, so I didn't blur it.

And a real gap I found while doing this, filed at the **bottom** of the backlog as `#lzcoveragerowfixtures` so it doesn't preempt the drain: **only 11 of 70 rows cite fixtures at all.** The claim guard checked 84 shipped marks — the other several hundred have nothing to verify against, including row 0. The guard is sound; it just has almost nothing to read. That's its own cycle because closing it will redden bindings whose marks are currently unbacked, which is the point.

Column churn hit 12 repos (the language axis is in every README roll-up). Each got a `git commit --only -- README.md`, so the concurrent session's uncommitted `AGENTS.md` edits are still uncommitted and still theirs in all ten siblings.

Queue: `#lzgdvocab`, then `#lzcppwasm`.

### Re: do [#lzcppwasm] — five phases landed; the target found three real defects — opus-5

`lazily-cpp` builds and **replays the canonical corpus** on wasm32. `make check` green (57 tests), `make wasm` green on both tiers, **87 core + 44 threaded distinct fixtures** replayed under Node — 118 of the 135 the native suite replays. Committed `efa68fd` + `c3a85f8`, superproject bumped at `3bdb55b9f3`.

**The header analysis in the plan was right but stale in one place, and I re-measured rather than trusted it.** `lazily.hpp`'s closure is 50 headers now, not 46. More usefully, the plan listed `<atomic>` among the wasm-hostile deps — it isn't. `<atomic>` compiles and works in single-threaded wasm; only `<thread>` and friends need `-pthread`. That distinction is why the core tier is much larger than the plan's four-header sketch implied.

**Tier assignment is measured, not declared.** I compiled every conformance runner with `em++` in each tier and recorded which succeeded. That turned up something the plan didn't anticipate: **21 of 30 runners are core-tier**, including codec, command, coordination, CRDT, lossless-tree and message-passing — families well outside `core.hpp`'s closure that are wasm-clean anyway. Six of them only needed `#include <lazily/lazily.hpp>` replaced with what they actually use; they were reaching for the umbrella out of habit, and that alone made them native-only. `wasm-tiers.conf` is the single source of truth for that table, read by both `tests/wasm.cmake` and the matrix guard, so the build and the published matrix cannot drift apart.

**The freestanding claim is compiled, not asserted.** `tests/wasm_core_include_check.cpp` includes `core.hpp` and nothing else, builds for wasm32 on every wasm run, and checks the include guards of the umbrella and native-only headers to prove they were *not* pulled in. A grep over include lines proves only what is written — the same trap as `ResourceLoader.get_dependencies()` in the GDScript cycle.

**Three defects the target surfaced, each fixed at the cause:**

- **emcc defaults to `-fignore-exceptions`.** It compiles `throw` into an abort and drops every `catch`. Four suites died with `Aborted(undefined)` on precisely their error-path fixtures. The dangerous version of this isn't the crash — it's that a suite whose error-path fixtures silently never ran would still have written a manifest and looked covered.
- **CMake's `file(STRINGS)` splits on non-ASCII bytes** without `ENCODING UTF-8`, so an em dash in a *comment* arrived as a second row and parsed as a malformed entry. Caught by the malformed-row check I had added minutes earlier, which is the argument for writing the check before you think you need it.
- **The bench harness printed nothing at all when a stage threw** — results accumulate and print at the end, so one exception took the whole report with it. Stages now name themselves on failure; that is how `bench_scale`'s `std::bad_alloc` on the 32-bit heap was identified rather than guessed.

**The matrix is generated from the manifests, and I drove the guard in seven branches** — empty manifest, missing manifest, hand-edited table, stale excuse, phantom family, unexcused gap, restored. The stale-excuse and phantom-family arms are checked in both directions exactly as `KNOWN_UNCOVERED` is; and per `#lzgdcoveragecolumn`, `WASM_ABSENT_REASONS` is the *only* narrowing mechanism, said so in the comment, because an unread second ledger does not weaken an audit — it inverts it.

**One of those seven branches caught something I would have shipped.** With an empty manifest the guard exited 1 *and printed nothing*: `grep` returns 1 on no matches, and under `set -euo pipefail` that aborted the script before the floor check could say why. It failed closed, which is right, and failed silently, which is useless — and the empty case is exactly the one that needs a diagnostic.

**Then I perturbed the kernel, because a replay that survives perturbation is not replaying anything.** Making `KeyedOrder::insert` prepend reddens `CollectionsFamilyConformance`, and restoring returns green. Worth flagging what it also showed: the **core tier's 18 collections fixtures did not catch it** — only the threaded tier's runner did. The core-tier replay is real, but it does not assert insertion order. That is the same shape as `#lzcoveragerowfixtures`, the next queue item.

**Two things I did not do, both deliberate.** I did not touch `coverage.json` — it tracks language bindings, not build targets of one binding. And I did not split the large headers; the plan's reasoning holds and nothing I measured contradicts it.

**Honest gaps.** `transport`/`reliable_sync` are native-only and not coming — they are POSIX shared memory and file-backed storage, and a wasm equivalent would be a different implementation behind the `BlobBackend` seam. `test_ipc` drives `LazilyFfiChannel`, the native C-ABI seam with no wasm consumer. Every one of those carries its reason in the matrix; there are no blank cells. Wasm benchmarks live in their own `BENCHMARKS.md` section with their own environment line, the scale ladder stops at 2M and says so at run time, and `transport` reports itself skipped rather than vanishing from the output.

**CI is still running** (`31290533974`) — the emsdk download is slow. Local evidence is green, but the wasm job has never been observed passing, and its `setup-emsdk@v14` pin at `6.0.6` is the one thing I could not verify locally. Filed as `#lzcppwasmci` rather than called proven.

Queue: `#lzcoveragerowfixtures`.

### Re: do [#lzcppwasmci] — wasm CI green; the red run was worth having — opus-5

> **Queue prompt:**
>
> do [#lzcppwasmci]

Run `31290754362` is **fully green**: `wasm`, `test (g++)`, `test (clang++)`. The guard reached its verdict rather than merely exiting zero — `wasm tier/matrix guard: OK (87 core + 44 threaded distinct fixtures replayed)`, the same numbers the local run reports, which is the actual confirmation that the environment-independence fix worked. Fix in `9436a88`, superproject at `885c1bcc00`.

**The `setup-emsdk@v14` pin at `6.0.6` was fine** — that was the risk I filed this item for, and it wasn't the problem. Both tiers built and replayed on the runner on the first attempt (24 core runners / 25 tests, 7 threaded / 8 tests).

**What actually failed was my matrix, and it was a design error rather than a CI quirk.** The table carried a Native column sourced from `build/conformance-fixtures-loaded.txt` — a manifest the *native* job writes. The wasm job never builds the native suite, so every native cell read zero there and the generated table could never match the committed one. A published table has to be derivable entirely from evidence its own job produces; mine wasn't, and no amount of local testing would have shown that, because locally both manifests always exist.

The audit is now stated against the **canonical corpus** instead of against native coverage. That is environment-independent and a stronger claim: every family the spec ships either replays on a wasm tier or carries a recorded reason, whatever the native suite happens to do.

**Second bug in the same failure:** `sort` is locale-dependent, so `(root)` ordered differently on the runner than on this machine. Pinned with `LC_ALL=C`. Any script generating a committed, diffed artifact needs that.

**Restating the audit against the corpus surfaced something the native comparison was hiding.** Three families — `egress`, `protobuf`, `agent-doc` — are replayed by lazily-cpp in **no** target at all, native included. Against native coverage they were invisible, because native replays them zero times too. Their reasons now say "a binding gap, not a wasm limit" explicitly. Collapsing those two kinds of absence would let "we never wrote this runner" hide behind "wasm cannot do this" — the same has-not/cannot distinction `⊘` preserves in `coverage.json`, which is why I kept the thread-safe rows at `—` last cycle.

Re-driven after the change: stale excuse and unexcused gap both still fail, and the guard passes with the native manifest absent — the exact CI condition that failed. `make check` green (57 tests), `make wasm` green on both tiers.

Queue: `#lzcoveragerowfixtures`.

### Re: do [#lzgdvocab] — 5 of 12 fixtures landed; two op families remain — opus-5

> **Queue prompt:**
>
> do [#lzgdvocab]

The runner now replays **14 of 21** canonical reactive-graph fixtures, up from 9. `KNOWN_UNCOVERED` drops from 12 named excuses to **7**. Commits `2a13609` and `0254fa0`, superproject at `53a59648ad`; the first is CI-green and the second is running.

Three op families landed, in the order the item itself prescribes.

**`signal` / `dispose_signal` (3 fixtures).** A signal is `ctx.computed(fn).eager()` — an eager `Computed`, not a distinct kind. The only observable that can tell the two apart is `computes_of`: a lazy and an eager computed return identical *values* for every read sequence in these fixtures, so a values-only assertion passes against a binding whose eager construct never materializes.

**`dispose_signal` needed real kernel work, not a runner alias.** It does not dispose anything — it de-eagers, and the backing value must stay readable *and* stay fresh. Routing it to `dispose()` makes the value unreadable; freezing it at the last eager materialization looks correct if you only check that the read succeeds. So `Computed.lazy()` clears the eager bit and deliberately leaves `_stale` alone. I also corrected a comment on `dispose()` that cited this fixture as its justification — that path is teardown, and the citation was wrong.

**`fail_next` (1).** The failure is injected in the runner, not the kernel: the contract is that a failed compute is never cached, and that holds however the body failed. A kernel-side failure switch would test the switch.

**`shape=scenarios` (1)**, plus `batch`.

**Two gaps this turned up, both fixed rather than noted.** `cleanup_order` only observed *effects* — scope-owned computeds and explicitly disposed cells recorded nothing, so the fixture's entire claim (that scope teardown equals the fold of individual disposals) could not be evaluated, because one of the two routes was invisible. And a scenario the runner *skips* raises no failures of its own, so replaying 1 of 2 scenarios would have reported the fixture conformant; the suite now asserts replayed scenario ids against the declared ones.

**The perturbation that matters most:** welding eagerness into invalidation — re-pulling per *write* instead of per batch — reddens **only** `signal_materializes_once_per_batch`. That is the clause-3 defect that ships unnoticed, since the final value is correct and only the work doubles. It is the known lazily-go failure, and no other fixture in the corpus can catch it. Separately, a no-op `lazy()` reddens only `dispose_signal_reverts_to_lazy`.

**One perturbation did not redden, and that was worth knowing.** Deleting the per-scenario state reset left `scope_teardown_equals_fold_of_disposals` green — both of its scenarios define the same ids, so a leaking runner just overwrites the stale entries. The isolation was correct by construction and unproven by the corpus, so it now has its own test on a synthetic input. Synthetic input is right there and nowhere else in this suite: it tests the runner's bookkeeping, not a normative expectation.

**What remains, and why it is not done.** Two families, 7 fixtures. `merge_cell` (5) is the only group needing **kernel** work — a folding `Source` with a merge policy, plus the `merges_of` observable — and `fanout`/`churn` (2) needs real id recycling, which Godot's monotonic instance ids do not provide. I narrowed `#lzgdvocab` to exactly those two rather than closing it, since the item's own framing is one op family per step and each group's definition of done is removing its `KNOWN_UNCOVERED` entries.

`make check` green: 26 test cases, 0 orphans.

### Re: do [#lzcoveragerowfixtures] — opus-5

`check-coverage-claims.mjs` now verifies **415 shipped marks across 49 rows**, up from 84 across 11. Commit `5e351b7` in lazily-spec, 11 sibling README syncs, superproject at `e98df71c70`; spec CI green (run 31293119177).

**38 rows now cite the fixtures that back their prose.** The guard was sound and had almost nothing to read — including row 0, the one row lazily-gd is scored on. Citations went on reactive-graph, materialization, familysync, statechart, collections, queue, topic, work-queue, merge-algebra, relaycell, crdt, lossless-tree, and stdlib.

**The two citations worth arguing about, and one of them I got wrong first.**

RelayCell does **not** cite `egress/*`. I was about to, because the row lives in the `egress` family and seven bindings excuse those four fixtures — which would have made this item look productive by reddening seven columns. `docs/relaycell.md` §8 says the opposite: conformance is pinned by replaying `mergecell_algebra.json` through a relay, and overflow validation, spill, and transport are proved by each binding's own model checker "not a portable fixture." The `egress/*` fixtures back `EgressCore` (`docs/transport-egress.md`) — the send-side core with sequence, window, watermark, retry, fence. **coverage.json has no row for it at all.** So four canonical fixtures score nothing and seven bindings excuse them against no claim. That is a real gap and it is now `#lzegressrow`, not a misattribution folded into RelayCell.

Merge-algebra cites the reactive-graph merge fixtures alongside `mergecell_algebra.json`, because `merge_cell_acquires_no_dependency_edge` is what pins the row's own "read-any-cell / write-`Source` split" clause structurally. That is the citation that reddened Go.

**Ten marks were unbacked, and every one is confirmed by that binding's own ledger, not inferred from a red guard.**

- **C++, three materialization rows.** Its `KNOWN_UNCOVERED` comment states it: `observational_transparency` and `deferral_not_deallocation` are "mirrored by hand in test_reactive_family.cpp and its async / thread-safe siblings; only `entry_kind_orthogonal_to_mode.json` is replayed from the canonical bytes." Two of three, hand-transcribed.
- **Go, merge-algebra.** `merge.go` and `relay.go` implement `MergePolicy` — the library is there. The reactive-graph *runner* has no merge/fold node kind, so all five merge/exact-fold fixtures sit in `reactiveGraphUnsupported` with "merge ops are NOT implemented and the drift assertion is NOT loosened."
- **Zig, three CRDT rows and three lossless-tree rows.** `text_crdt.zig`, `seq_crdt.zig`, and `lossless_tree.zig` exist with their own tests; the ledger reason is "No runner at all in this binding."

All three are "implemented, but not this wire" — which is what `~` is for, per the guard's own header — so those cells are `~`, not deleted citations. Roll-ups follow: Go reactive-graph, C++ materialization, Zig CRDT, Zig lossless-tree.

**What I did not cite.** Registers (LWW/MV + `PnCounter` + `CellCrdt`) has no canonical fixture anywhere in the corpus, so it stays unverifiable — filed rather than papered over. Row 0 cites only the fixtures matching its prose (two cell kinds, Effect sink, eager `Computed`, batch); the disposal and scoping fixtures are real but no coverage row claims them, and citing them on row 0 would have over-claimed to inflate the verified count.

`make check` green in lazily-spec: 279 schema tests, Lean build, all nine guards.

### Re: do [#lzegressrow] — opus-5

`coverage.json` now scores **EgressCore** as three rows of its own. The claim guard verifies **445 shipped marks across 55 fixture-bearing rows**, up from 415/49. Spec commit `f6d575f` (CI green, run 31294188085), 11 sibling README syncs, superproject at `2fb5a6f67e`.

**Three rows, not one, because egress mirrors ingress.** The item left the shape open — one row or three, "if the three context flavors are scored separately like queue/topic/work-queue". Egress is the send-side mirror of ingress, ingress is already scored as core + thread-safe + async, and `EgressCell` / `ThreadSafeEgressCell` / `AsyncEgressCell` are three real shells over one `EgressCore`. Collapsing them to one row would have made egress the only family in the corpus whose shell axis is invisible.

**Rust ships all three; nothing else has an egress runner.** That is read from evidence on both sides: `tests/egress_family_conformance.rs` drives `SyncModel`, `SharedModel` and `AsyncModel` through the same `replay_corpus` over all four fixtures, with a per-shell test asserting the full step count; and seven bindings (py, kt, js, dart, zig, go, cs) name all four in `KNOWN_UNCOVERED` with "Reactive egress is currently Rust-only". C++ and GDScript are absent by scope rather than by ledger — `egress` is not in lazily-cpp's `REQUIRED_AREAS`, and lazily-gd's `IMPLEMENTED_FAMILY_PREFIXES` claims only `reactive-graph/`.

**The honest consequence is that eight columns lost a ✅.** The Egress family roll-up goes from shipped to partial for Python, Kotlin, JS, Dart, Zig, Go, C++ and C#. They still ship RelayCell; they were reading as shipped on a family whose core primitive they do not implement, because the only row in it was RelayCell. That is the whole point of the item, and it is why the four egress fixtures existed with nothing to score.

**Three perturbations, three directions.** Flipping Python to ✅ on the new row fails with all four fixtures named from lazily-py's own ledger. Citing a fixture outside the corpus fails as an uncited-corpus error. And adding one egress fixture to **lazily-rs's** `KNOWN_UNCOVERED` reddens all three new rows — which is the direction that matters, since it proves the Rust marks are backed by that binding's ledger rather than passing vacuously. The mark count moving exactly +3 (one Rust ✅ per new row) is the same confirmation arithmetically.

**One thing the guard structurally cannot see, stated rather than implied.** It resolves fixtures per binding, not per shell, so all three rows cite the same four files and a binding replaying only its sync shell would pass all three. That is not a defect I introduced — queue, topic and work-queue have carried it since they were scored — but the shell axis is verified by the binding's own per-shell tests, not by this guard.

**I also cited the ingress fixtures while I was in the same audit surface.** The three ingress rows carried *no* fixtures at all despite nine bindings marked ✅ and no binding excusing a single `ingress/*` file — an omission from `#lzcoveragerowfixtures`, not a considered exclusion. Citing the seven `IngressCell` fixtures verifies 27 more marks and reddens nothing.

**And it turned up the same gap one family over.** `conformance/ingress/boundary_ingress_adapter.json` declares `"model": "BoundaryIngressAdapter"` — not `IngressCell` — and **nine bindings replay it against no row at all**. I did not fold it into the ingress rows to inflate the count; citing it there would claim IngressCell conformance from an adapter fixture, exactly the misattribution I refused for RelayCell and `egress/*` last cycle. Filed as `#lzboundaryingressrow` at the bottom of the backlog.

`make check` green in lazily-spec: 279 schema tests, Lean build, all nine guards. `AGENTS.md` carries another session's uncommitted edits in lazily-spec and stayed uncommitted — every commit was `git commit --only -- <paths>`.

Queue: `#lzcppmatreplay`.

### Re: do [#lzcppmatreplay] — opus-5

> **Queue prompt:**
>
> do [#lzcppmatreplay]


C++ is back to ✅ on all three materialization rows. `lazily-cpp` now replays `observational_transparency.json` and `deferral_not_deallocation.json` from the canonical bytes in **all three shells**, the two `KNOWN_UNCOVERED` entries are gone, and the claim guard verifies 448 shipped marks across 55 rows. Commits `d889f8e` (cpp) and `2e01b4e` (spec, CI green 31294673470), 10 sibling README syncs, superproject at `f9e29f2c09`.

**Three shells, not one, because that is what the rows claim.** The item asked for byte replay; the fixture-level guard would have accepted a single-threaded runner and restored all three marks, since it resolves fixtures per binding and cannot see the shell axis — the limitation I flagged one cycle ago. So the replay lives once in `tests/test_materialization_replay.hpp`, parameterised over the map shell, and `Context` / `ThreadSafeContext` / `AsyncContext` each replay the same bytes. They cannot share a binary: the two threaded shells force the `-pthread` wasm tier, so the core-tier runner stays single-threaded and the shells get their own target — the split `test_collections_fixture` / `_family` already uses for exactly this reason.

**The six mirrors are retired.** `entry_kind`'s mirrors stay, since that fixture was already byte-replayed and they are ordinary unit tests over synthetic input. Every expectation now reaches an `AssertionKeys` tracker, so a key the corpus grows fails by name — the mirrors could not fail that way at all, carrying no reference to the block. Proven: adding an `eager_present_count` key to the fixture reddens with "present in the fixture but was never consumed by this runner."

**My first `default_mode` assertion was vacuous, and the perturbation is what caught it.** I branched on the fixture's own value and then asserted the present-set that branch implies — which tests the `if`, not the library. Flipping the corpus to `"lazy"` left the suite green while five other perturbations reddened. It now follows the Rust reference: the named mode **selects the build**, and the asserted fact is that a map built that way is fully materialized at build time. Flipping the key reddens both binaries. Writing the probe before I thought I needed it is the only reason this is not shipped.

**Six fixture perturbations and two library mutants.** Dropped `eager_present` key, altered `present_after_each_read`, a longer `reads` sequence, a wrong `observe` value, an unread new key, a flipped `default_mode` — all red. Then `materialize_all` skipping a key in both the sync and thread-safe maps — both red. The fixture probes ran against a scratch corpus under `LAZILY_SPEC_CONFORMANCE_DIR`, not the shared one, since eight other bindings read those bytes.

**One thing the wasm work from two cycles ago paid for immediately.** The new runner reached for `<lazily/lazily.hpp>` out of habit and the wasm build rejected it — the umbrella drags in `transport.hpp` and `reliable_sync.hpp`, whose guards refuse wasm32. That is the same habit that made six runners native-only in `#lzcppwasm`, and it failed at compile time instead of quietly becoming a native-only target. `make wasm` green on both tiers: **89 core + 46 threaded** distinct fixtures, up from 87 + 44.

**A cross-binding gap this turned up, filed rather than folded in.** `default_mode` gets **five different treatments** across nine bindings. Rust and C# assert the behaviour the mode implies. Python, Kotlin and JS **excuse** the key as "no mode flag to read back" — a reason C++ and C# disprove, since neither has a mode flag either. Dart and Zig compare it against the string literal `"eager"`, which is the exact vacuity my own first draft had. Go deserializes it into a struct field. Filed as `#lzdefaultmodeuniform`.

`make check` green in lazily-cpp: 58 tests, **137 canonical fixtures across 23 areas, 5 known-uncovered — down from 7**, and 144 of 144 declared scenarios replayed. Re-adding one entry to lazily-cpp's own ledger reddens exactly the three restored marks. lazily-cpp CI (`31294667985`) is still running — the wasm job downloads emsdk — so it is recorded as pending, not claimed.

Queue: `#lzgomergeops`.

### Re: do [#lzgomergeops] — opus-5

> **Queue prompt:**
>
> do [#lzgomergeops]


`reactiveGraphUnsupported` in lazily-go is now **empty**, and the drift assertion was never loosened to get there. All six fixtures — the five merge-feed ones plus `feedback_drain_bound_reports_exhaustion` — replay on **both** the sync and async planes. Go is back to ✅ on merge-algebra; 449 verified marks. Commits `e9ab14d` (go) and `094c6a4` (spec), 10 README syncs, superproject `89b769c`.

**The item called this a runner op gap. Two of the six were library gaps, and that is the more interesting half.**

`Context.flushEffects` was an **unbounded loop**, and `AsyncContext`'s rerun chain was unbounded too. An effect writing into its own dependency cone hung the process — not hypothetically: removing the bound again makes the suite hang until the test timeout, which is the fourth mutant below. That loop closes through the **scheduler**, not the graph, so acyclicity checks never fire; and because a nested `flushEffects` returns at the re-entrancy guard, the drain runs *flat at depth 1*, so a recursion-depth bound could never fire either. Both planes now bound iterations and record a `DrainExhaustion` — iterations, budget, busiest effects — leaving the worklist as-is, because exhaustion is not convergence and discarding the pending work would make a cut-short drain indistinguishable from a finished one.

`AsyncSource` also had no merge surface at all: no policy, no `Merge`. It gained both, mirroring the sync `Source`. The fold runs inside **one loop command**, so it is atomic against every other graph mutation — which is precisely why it belongs in the library and not as a `Peek`-then-`Set` at the call site. A runner-side fold would have passed these fixtures while leaving the binding without the primitive.

**The runner half:** the `merge_cell` / `merge` / batch-`merges` ops, the `merges_into` and `writes_own_cone` effect flavours, and the `merges_of` / `drain_exhausted` keys. The merge counter ticks from **inside** the effect body, so it moves only when the runtime actually folds. Ticking at the call site would report the runner's intent rather than the library's behaviour — and that is the single thing these fixtures discriminate on.

**Three details the corpus forced, each a real distinction I had wrong first:**

- `computes_of` on an **effect** is its run count, not a compute counter. `merge_folds`'s `watch` is an effect observing the accumulator, so the compute counter reads 0 forever.
- A feed effect's step asserts `value` on the cell it **feeds**, not on the effect — the effect has no value. The corpus names the target in `merges_into`.
- The divergent body must hold **0 as a fixed point**. The dependency edge registers the instant the tracked read runs, so a plain `v+1` reschedules itself mid-creation and exhausts the drain before the external kick ever lands — and the fixture pins `drain_exhausted: false` at creation. Writing 0 back lets the store's equality guard suppress the invalidation; `set_cell(counter, 1)` then moves off the fixed point and it diverges.

**Four mutants, and one of them is the whole argument for the feature.** Making `Merge` ignore its policy reddens the value assertions. Ticking the merge counter at construction instead of per fold reddens the counts. Making the feed effect track the accumulator — modelling the feed as a real edge *into* the merge cell — reddens `dependents_of.acc` and `dependencies_of.feed`, which is exactly what `merge_cell_acquires_no_dependency_edge.json` says is the only assertion that can catch a fourth node kind that is both writable and dependent. And removing the drain bound hangs the suite. A test that only *passes* faster with the feature would not have been evidence; a test that **hangs** without it is.

`make check` green in lazily-go: 604 tests, race detector, **141 of 149 canonical fixtures opened (up from 135), 8 known-uncovered (down from 14)**, 144/144 scenarios replayed. Re-adding one merge fixture to lazily-go's own ledger reddens exactly the restored mark. CI is running on both repos (`31296523543`, `31296529788`) and is recorded pending rather than claimed.

Queue: `#lzzigcanonrunner`.

### Re: do [#lzzigcanonrunner] — 9 of 12 landed; the collections half remains — opus-5

> **Queue prompt:**
>
> do [#lzzigcanonrunner]


lazily-zig now replays all **nine canonical lossless-tree fixtures** from the corpus bytes. Zig is back to ✅ on the three lossless-tree rows; 452 verified marks. Commits `7823540` (zig) and `db98b8d` (spec), 10 README syncs, superproject `e59661a`. I narrowed the item to the three collections CRDT fixtures rather than closing it.

**"Implements it" and "conforms to it" were two different claims here.** `lossless_tree.zig` carries the whole `LosslessTreeCrdt` op vocabulary and its own tests, and *opened none of the corpus* — all nine sat under "No runner at all in this binding." That is the inline-mirror failure one step earlier: a mirror at least agrees with something, and retiring this binding's eight of them turned up three real wire defects. A missing runner has nothing to disagree with at all.

**Two assertions are deliberately stronger than the fixture's letter.** `converged` compares rendered text **pairwise between replicas**, not each against the expected string — two replicas that both match the string while disagreeing about anything `render` elides would otherwise pass. And `live_nodes` is checked on **every** replica, not just `a`; after a converging exchange the counts must agree, so checking one would miss a replica that resurrected or dropped a node.

**The harness caught my first attempt at `render_on`.** I swept the object key by key, and the rung-6 guard refused it: sub-fields checked one by one and the key *set* by nothing, so a replica the corpus adds later is compared by nothing and the runner stays green. It descends through a child tracker now.

**Two mutants, and one of them survived — which I am reporting rather than rounding off.** Reading `at_byte` as a **char** offset instead of a byte offset — the exact multi-byte trap these fixtures exist for — reddens six of the nine. But making `diff` **ignore the partner frontier entirely** reddens *none* of them, including `non_contiguous_anti_entropy`, whose whole subject is a replica with a gap. So the runner currently pins that the replicas converge, and does not pin that anti-entropy ships only what the partner lacks. Filed as `#lzzigdiffmutant`; a mutant that lives is a statement about what the suite pins, not a detail to leave implied.

**What remains, and why it is not done.** The three collections CRDT fixtures — `textcrdt_convergence`, `textcrdt_delta_sync`, `seqcrdt_convergence` — are a distinct runner over two other models (`TextCrdt`, `SeqCrdt`) with their own vocabulary: nine ops, fifteen expect keys, plus `clone` / `snapshot` / `delta` / `exchange` / `stable` / `gc`, versus the five ops and four expect keys the tree fixtures use. They score the three CRDT rows, which stay at `~`. I narrowed `#lzzigcanonrunner` to exactly that group rather than closing it on half the work.

`make check` green in lazily-zig: **129 of 149 canonical fixtures opened (up from 120), 20 known-uncovered (down from 29), 128/128 scenarios replayed (up from 114)**. CI is running (`31296905681`) and recorded pending. One note on the local suite: bare `zig build test` prints the scenario-identity guard's own self-test to stderr and the build runner reports that as a failed command — the Makefile's `test` target is what tolerates it. I checked that against HEAD before touching anything, since a red-looking suite I did not cause is exactly the thing to verify rather than assume.

Queue: `#lzregistersfixture`.

### Re: status — five heads remain, drain paused by the binary — opus-5

No new instruction arrived this turn: the only change is the `resume:` frontmatter UUID, which is managed state, not a prompt. So this is a status turn rather than a work turn.

**The binary is holding the drain, and I am not overriding it.** `queue_continuation_required` is `false` even though `queue_drainable_head_count` is 5, and the queue head `#lzregistersfixture` carries a `⏭️` deferral marker with `#lzgdvocab` marked `🚧`. The in-session loop rule is explicit that this signal is computed in the binary and must not be re-derived from the queue prose, so I am reporting rather than looping.

**Remaining heads:** `#lzregistersfixture` (author the missing canonical Registers fixture — the row is marked shipped in nine bindings and verified in none), `#lzgdvocab` (lazily-gd's last two op families: `merge_cell` needs kernel work, `fanout`/`churn` needs real id recycling), `#lzboundaryingressrow`, `#lzdefaultmodeuniform`, `#lzzigdiffmutant`.

**Where the four drained items left things.** All repo work is committed and pushed with CI green or pending on each repo: `#lzegressrow` (three EgressCore rows; Egress roll-up honestly dropped to partial in eight columns), `#lzcppmatreplay` (byte replay in all three C++ shells), `#lzgomergeops` (bounded effect drain on both Go planes — both were unbounded and hung on a scheduler-closed loop — plus an async merge-cell surface), and `#lzzigcanonrunner` (nine lossless-tree fixtures; narrowed to the three collections CRDT fixtures rather than closed on half the work).

Two open threads I would not want lost in the summary: **a surviving mutant** — making `LosslessTreeCrdt.diff` ignore the partner frontier reddens none of the nine Zig fixtures, so the runner pins convergence but not the anti-entropy filter (`#lzzigdiffmutant`) — and **`default_mode` getting five different treatments across nine bindings**, two of which are the exact vacuity my own first draft had (`#lzdefaultmodeuniform`).

One note without a request attached: the exchange is at 403 lines across 19 response sections, well past the accretion threshold. Compaction needs an explicit `agent_doc_auto_compact` opt-in, so I am recording the size rather than acting on it.
<!-- agent:boundary:c6ef3fb1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzegressrow]~~
- ~~do [#lzcppmatreplay]~~
- ~~do [#lzgomergeops]~~
- ~~do [#lzzigcanonrunner]~~
- ⏭️ do [#lzregistersfixture]
- 🚧 do [#lzgdvocab]
- do [#lzboundaryingressrow]
- do [#lzdefaultmodeuniform]
- do [#lzzigdiffmutant]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzzigcanonrunner] lazily-zig has no canonical runner for the three collections CRDT fixtures, so Zig stays at ~ on the three CRDT rows (free-text-character-crdt, textcrdt-delta-sync, move-aware-sequence-crdt). text_crdt.zig and seq_crdt.zig exist with their own tests; what is missing is corpus replay. Add it for collections/{textcrdt_convergence,textcrdt_delta_sync,seqcrdt_convergence}.json — a distinct runner over two models (TextCrdt, SeqCrdt) with 9 ops (insert, insert_str, delete, gc, insert_back, insert_front, move_after, set_value, remove), 15 expect keys (text, len, texts_equal, a_starts_with/a_ends_with, tombstone_count, version_vector_on, text_on, order, get, orders_equal, contains_all, order_on, get_on, not_contains_on) and the clone/snapshot/delta/exchange/stable step forms. Follow src/lazily/lossless_tree_conformance.zig, which landed the other nine fixtures of this item. DONE already: all nine lossless-tree fixtures replay and Zig is shipped on those three rows.
- [ ] [#lzregistersfixture] The Registers row (LWW / MV + PnCounter + CellCrdt) is marked shipped in nine bindings and has NO canonical fixture anywhere in the corpus, so check-coverage-claims.mjs cannot verify a single one of those marks. seqcrdt_convergence.json exercises LWW registers only incidentally, as the substrate for move-aware sequence order, and backs the SeqCrdt row instead. Author a canonical crdt fixture pinning LWW resolution (including the equal-timestamp tiebreak), MV concurrent-value retention, PnCounter increment/decrement convergence, and the CellCrdt reactive projection; then cite it on the row.
- [ ] 🚧 [#lzgdvocab] Finish the lazily-gd conformance runner: 7 of the 21 canonical reactive-graph fixtures remain, in two op families. (a) merge_cell (5 fixtures: exact_fold_paths_stay_exact, merge_cell_acquires_no_dependency_edge, merge_feed_through_a_formula_coalesces, merge_folds_synchronously_in_batch, merge_per_settled_cone_not_per_write) — this is the one group needing KERNEL work, a folding Source with a merge policy (source_with/merge), plus the merges_of observable. (b) fanout/churn/dispose_fanout/dispose_stale_handle bulk ops plus real id recycling (2 fixtures: churn_returns_to_baseline, recycled_id_inherits_nothing) — recycling needs explicit work beyond Godot instance ids, which are monotonic and never alias. Each is named in KNOWN_UNCOVERED with its reason; removing an entry there is the definition of done for that group. Landed already: signal/dispose_signal (3), fail_next (1), shape=scenarios (1) — 14/149 replayed, 7 named excuses.
- [ ] [#lzboundaryingressrow] conformance/ingress/boundary_ingress_adapter.json declares "model": "BoundaryIngressAdapter" — a distinct model from IngressCell, with its own bootstrap/subscribe/cursor phases — and NINE bindings replay it (lazily-rs tests/boundary_ingress_conformance.rs, py, kt, js, dart, zig, go, cpp, cs) against NO coverage.json row at all. Same gap class as #lzegressrow: a canonical fixture nine bindings replay scores nothing, so nine real implementations get no credit and any future regression in the adapter moves no cell. It must not be cited on the reactive-ingress rows — that would claim IngressCell conformance from an adapter fixture, the misattribution refused for RelayCell/egress in #lzcoveragerowfixtures. Add a BoundaryIngressAdapter row (family ingress, or its own), cite the fixture, and score the nine bindings from their own ledgers.
- [ ] [#lzdefaultmodeuniform] The materialization corpus's default_mode key gets FIVE different treatments across nine bindings, so the default_mode_eager clause is not uniformly proved. Rust (tests/materialization_conformance.rs) and C# (MaterializationConformanceTests.cs) assert the BEHAVIOUR the named mode implies — the mode selects the build, and a map built that way must be fully materialized at build time — and lazily-cpp now does the same via tests/test_materialization_replay.hpp. Python (_excuse_default_mode in tests/test_reactive_family.py), Kotlin and JS EXCUSE the key as 'no mode flag to read back', a reason C++ and C# disprove since neither has a mode flag either. Dart (materialization_conformance_test.dart) and Zig (reactive_map.zig) compare it against the string literal 'eager', which asserts only that the fixture equals itself (#lzconsumednotasserted). Go deserializes it into a DefaultMode struct field. Converge all nine on the behavioural assertion and delete the three excuses; a corpus that flips default_mode must redden every binding, which today it does not.
- [ ] [#lzzigdiffmutant] lazily-zig's lossless-tree replay does not pin the anti-entropy FILTER, only convergence: mutating LosslessTreeCrdt.diff to ignore the partner frontier (ship every op in the log) reddens NONE of the nine fixtures, including non_contiguous_anti_entropy.json whose subject is a replica with a gap. deliver.only indexes into the diff, so a diff whose contents change should select different ops and change what the gapped replica renders (hole_before_repair expects b='013') — it does not, and the reason is not yet established. Either the runner's deliver path is not exercising the frontier as intended, or applyUpdate's idempotence absorbs the difference; find out which, then make the fixture discriminating (or record honestly that convergence is all it can pin). Contrast the at_byte char-offset mutant, which reddens six of the nine.
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
