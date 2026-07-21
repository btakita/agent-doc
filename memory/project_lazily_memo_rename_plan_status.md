---
name: project_lazily_memo_rename_plan_status
description: "Cell kernel v2 (Source/Computed/Effect handles, .eager()/.lazy(), all-cells-guarded) EXECUTED across all 11 lazily repos 2026-07-21 — committed, not pushed, pending push/deploy go-no-go"
metadata: 
  node_type: memory
  type: project
  originSessionId: 5fa6f804-041f-4471-ae01-08506e5c78b4
---

`tasks/software/lazily-memo-rename-plan.md` was SUPERSEDED by
`tasks/software/lazily-cell-kernel-design.md`, EXECUTED as **v1** 2026-07-20 then
refined to **v2** 2026-07-21 (user drove the naming interactively). Both were
user-authorized. The old "do NOT run the ~12k rename" guidance is retired.

**The v2 kernel (shipped, final):** **no `Cell<T,K>` genus** — two concrete handle
structs **`Source<T, M = KeepLatest>`** (get/set/merge) and **`Computed<T>`** (get
only), plus **`Effect`** (bare, renamed from `EffectHandle`; sibling handle, the
value-less sink). `Cell` is the **value-node concept only** (a `SourceCell` /
`ComputedCell`), never a handle. `computed.set()` is a compile error wherever the
language allows (rs inherent / kt ext-fn / zig comptime / cpp spec / **go now via
distinct structs — no genus needed, §4 exception dissolved**); py/js/dart = method
presence. Eager = **`computed(f).eager()` / `.lazy()` / `is_eager()`** (declarative,
idempotent; `eager` bit + `eager_by` side table). **ALL cells guarded, always** —
Source `==` store-guard on write, Computed equality guard on recompute (matches
TC39 `Signal.Computed`); `T: PartialEq` the uniform bound; **`memo` REMOVED**
(folded into guarded `computed`); NO unguarded mode — escape is a `PartialEq`-
distinct value or a merge policy. Non-`PartialEq` values use the lower-level
unguarded `slot` STORAGE primitive (not a cell). `Reactive` is an umbrella
adjective only (no trait uniting Cell+Effect). `SlotId`/`SlotValue`/wire unchanged.

**v2 commits (committed-not-pushed):** spec 666835d, formal 32f2e39, rs 51b2e27,
py 051b717, js e42e93e, kt e04b6706, react ce8314c, dart 6224c47, cpp d36130b,
zig e3d8035, go 6ca7526. Design doc fully v2 (working tree, fold into superproject
commit). Blog 9ea681a still says `formula().drive()` — **needs a v2 pass**
(computed().eager()) before deploy, PENDING AUTHOR REVIEW.

**Per-binding divergences (all language-forced, benign):** kt keeps a single-param
`Cell<T>` READ interface (JVM erasure needs one `get` signature — no write surface);
dart's reverted-to-lazy `Computed` is unguarded (push-cascade + clause-4); go/kt use
policy-as-field not `<T,M>` (no default type params); py/js/dart/go/zig/cpp keep
old names as deprecated aliases (rs hard-renamed) — see [[feedback_discuss_naming_before_applying]].

**KNOWN RED (pre-existing, from Step 3, NOT v2):** 6 mergefeed/feedback fixtures
(`#lzmergefeed`) landed in lazily-spec but the non-rs binding runners' FIXTURES
lists don't know them → fixture-set-drift red in py/go/zig/js/kt/dart. **cpp fixed
it in-repo** (added the 6 + skips on unsupported `merge_cell`); the other 6 need
the same. NOTHING pushed/released.

**Remaining (see task board / follow-ups):**
- Step 3 (§9.1): merge on ThreadSafe/Async + land 5 parked mergefeed fixtures — an
  agent was executing this at session end; verify it committed. [[project_lazily_mergefeed_fixtures_parked]]
- Superproject submodule-pointer bump (local) + PUSH/DEPLOY — needs explicit user
  go/no-go (outward, shared-superproject hazard). [[feedback_concurrent_sessions_force_push_shared_superproject]]
- Family-wide cleanup: delete compat aliases + finish call-site migration (incl.
  computed→guarded formula); generation-tagged SlotId (§9.3.5) deferred in every
  binding (N/A for GC langs); ThreadSafe/Async signal families still separate in
  rs/kt/cpp/js. async.md + relaycell-backpressure-analysis.md need a semantic
  rework (they use the pre-kernel trait model), NOT a rename.

**Pre-existing, not caused by this work:** `lazily-rs` `make check` is red on
`spec_compliance`'s `#lzbenchver` gate (BENCHMARKS.md does not name 0.46.0); kt &
zig have a flaky AsyncContext `recycled_id`/`computes_of` divergence on a clean
tree. All confirmed baseline, unrelated to the kernel change.
