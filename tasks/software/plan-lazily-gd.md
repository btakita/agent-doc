# Plan: `lazily-gd` — GDScript binding (`#lzgdbinding`)

**Status:** Phases 0–2 landed 2026-08-08 (harness + engine floor + CI; cell
kernel; conformance runner replaying 9/21 `reactive-graph` fixtures), and the
`GDScript` column is in `lazily-spec/coverage.json` — `~` on Reactive graph, `—`
on the other 28 families. Adding it required fixing the spec-side claim guard
first: it read `KNOWN_UNCOVERED`/`REQUIRED_AREAS` but not this binding's
`IMPLEMENTED_FAMILY_PREFIXES`, so 128 family-excused fixtures counted as
replayed. Phase 3 (row 16, stable-id alignment) is next.

**Note on row numbering.** This plan says "rows 0 and 15"; in the current
`coverage.json` stable-id alignment is row **16** (row 15 is `SemTree`). Rows
shift as the matrix grows — go by row `id` (`reactive-graph`,
`stable-id-alignment`), not by index.
**Created:** 2026-07-30
**Session:** `tasks/software/lazily.md`
**Backlog:** `#lzgdbinding`
**Related:** `tasks/software/plan-lazily-cpp-wasm.md` (`#lzcppwasm`), `tasks/software/lazily-cs-parity-plan.md` (the 9th-binding precedent this plan copies)

## Goal

Add **`lazily-gd`**, a pure-GDScript binding, as the 10th column in
`lazily-spec/coverage.json`. Ship as a Godot 4.x addon consumable by third-party
game projects, starting from coverage rows 0 (reactive graph) and 15 (stable-id
alignment) — the same staged entry `lazily-cs` used.

## Why GDScript and not a GDExtension over `lazily-cpp`

Measured: `lazily-cpp` is header-only C++17 and its reactive core is wasm-clean
(`cell.hpp` closure = 8 headers, zero POSIX, zero threads), so a GDExtension is
technically viable. It was rejected anyway:

- **Consoles are the blocker, not web.** Switch/PS/Xbox Godot builds come from
  third-party porting houses. You cannot produce those GDExtension binaries
  yourself. GDScript ships wherever Godot ships, with no per-platform binary
  matrix, no godot-cpp pinning, no debug/release pairs tracking Godot minors.
- **The wasm-hostile `lazily-cpp` pieces are the ones a Godot SDK should not use
  anyway.** `transport.hpp` (`shm_open`/`mmap`) and `reliable_sync.hpp`
  (`unistd.h`) would be replaced by Godot's own `WebSocketPeer` regardless. What
  the SDK actually wants from lazily is the layer with zero native deps.
- **Web GDExtension** needs a dynamic-linking-enabled export template and is the
  least-exercised engine platform.

`lazily-godot` is deliberately **not** used as the repo name. It is reserved for a
future engine-integration layer (`WebSocketPeer` transport glue, node/resource
helpers, optional desktop GDExtension fast path) — the same relationship
`lazily-react` / `lazily-html` have to `lazily-js`, neither of which is a
conformance binding. Repo token is the language extension (`.gd`), matching
`rs`/`py`/`kt`/`go`/`cs`/`js`/`cpp`; the coverage column displays as `GDScript`,
the way `lazily-cs` displays as `C#`.

## The one novel engineering problem: no cycle collector

`RefCounted` has no cycle collector. No other binding faces this — rs/cpp use
`Weak`/`weak_ptr`; py/js/dart/kt/cs/go have GC. The corpus requires bidirectional
edges (`conformance/reactive-graph/dispose_detaches_edges_both_directions.json`),
so a `Computed` holding its deps while each dep holds a dependents set is a leak.

**Decision: WeakRef back-edges.** Forward `Computed → deps` strong, back
`source → dependents` weak. Three consequences, all corpus-observable:

1. **`ctx.scope()` becomes the sole strong owner.** An `Effect` whose only inbound
   reference is a weak back-edge is collectable the moment the caller drops it,
   unlike every GC binding. The reverse-creation-order member list **is** the
   ownership record. An `Effect` created outside any scope must be an **error**,
   not a silently-collected no-op.
2. **Prune dead `WeakRef`s before any edge-count read.** GDScript has no finalizer
   hook, so pruning must be amortized into propagate/attach. The O(1) dedup work
   asserts edge counts; deferred pruning reads them high.
3. **Key the edge index on `get_instance_id()`** — monotonic in Godot, so ids never
   alias. Orthogonal to `recycled_id_inherits_nothing.json`, which is about
   lazily's own id recycling and still needs explicit implementation.

Fixtures that catch a wrong weak/strong split:
`teardown_runs_members_in_reverse_creation_order`,
`disposal_does_not_run_surviving_effects`, `cross_scope_teardown_hazard`,
`dispose_detaches_edges_both_directions`, `scope_teardown_equals_fold_of_disposals`,
`disarm_disposes_nothing`.

## Corpus reality (measured 2026-07-30)

`lazily-spec/conformance/`: **134 fixtures, 27 `kind`s, 73 distinct ops**, against
64 coverage rows × 9 language columns. Largest families: ReactiveGraph 21,
Collection 20, LosslessTree 9, Command 8, ReliableSync 8, Ingress 7, StateChart 7,
RateShape 6. Full parity is not a single push; this plan is explicitly staged.

## Loading model (the "efficient loading" requirement)

Two separate axes, and only one of them is lazy:

- **Runtime loading is lazy and per-file.** A `.gd` file is a Resource, loaded and
  compiled only when referenced. There is no "load the library" step.
- **Distribution is not lazy.** Every `.gd` under `res://` is packed into the PCK
  whether used or not. Godot has no tree-shaking or dead-script elimination.
  GDScript source is small enough that PCK size is a non-issue; the real costs are
  parse/compile time and reference fan-out.

What creates a load edge:

| Construct | When it loads |
|---|---|
| `preload("res://...")` | **Eager and transitive** — resolved when the *referencing* script compiles |
| `load("res://...")` | Runtime, lazy, on the call |
| `class_name Foo` | Registers name→path in the global class cache; **does not load**, resolved on first use |
| `var x: Foo` (static type) | Parse-time dependency on `Foo`'s script |
| `extends Foo` | Pulls the base script |
| autoload / singleton | Loads at startup unconditionally, plus everything it preloads |

`preload()` and autoloads are the two ways to drag the whole library in. An
umbrella `lazily.gd` that preloads every family reproduces the `lazily.hpp`
umbrella problem in a different language with a different mechanism.

**Rules, enforced by a test rather than a convention (see Phase 1 exit criteria):**

- No umbrella script. Each family independently reachable via its own `class_name`.
- Zero cross-family `preload()`; zero cross-family static type annotations in the
  kernel (`var q: TopicCell` inside a cell-kernel file welds queue to kernel at
  parse time).
- No autoload as the SDK entry point.
- Runner + fixtures export-excluded so consumers do not pack the test corpus.

The layering that keeps `cell.hpp` free of POSIX and threads is the same layering
that keeps the GDScript kernel free of preload edges into the queue family. One
dependency discipline, two enforcement mechanisms.

## Phases

### Phase 0 — Scaffold

- Repo `lazily-gd`, laid out so **the repo root is the addon**: `addons/lazily/`
  holds shipping code; tests and fixture runner live outside it. Consumer-facing
  folder is `addons/lazily/`, not `addons/lazily-gd/` — a project only ever has one.
- `AGENTS.md` canonical, `CLAUDE.md` symlink to it (matches `lazily-cs`).
- `Makefile` with `check` (every binding has one).
- CI: `godot --headless` under GitHub Actions with a pinned Godot download.
- **Minimum Godot version: 4.4** — DECIDED 2026-08-08 (operator). The floor is
  set at typed dictionaries (`Dictionary[K, V]`, landed in 4.4), which is where
  the edge index keyed on `get_instance_id()` and the keyed-collections family
  read as declared types rather than untyped `Dictionary` with runtime key
  discipline. Deliberately several releases behind the 4.7.1 the operator runs
  locally: an addon's floor decides who can consume it, not what its author runs.
  Consequence to hold to throughout — 4.5's `@abstract` is NOT available, so the
  base kind that `Source`/`Computed`/`Effect` share is a conventional base class
  with erroring stubs, not an abstract one.
- **Test framework: gdUnit4** — DECIDED 2026-08-08, confirmed against the 4.4
  floor — and the confirmation had to be done twice, because the first pass got
  it wrong. **Pinned at v5.1.1.** Upstream's compatibility row listing
  `v4.3, v4.4, v4.4.1` covers the **v5.x** line; pinning v6.2.0 off that row
  produced a build that fails to resolve its own classes on 4.4.1 and then HANGS
  rather than exiting, burning the CI job limit. Two lessons worth keeping: read
  the released tag rather than `plugin.cfg` on `main` (which reads `6.2.1`,
  unreleased, 404s), and verify a framework pin against the FLOOR engine, not the
  engine you happen to run — 4.7.1 hid this completely. v5.1.1 verified passing on
  both 4.4.1 and 4.7.1. Raising the Godot floor is what unlocks v6, not a
  framework bump alone. The deciding argument for gdUnit4 over GUT is stronger
  than "it has a CI action":
  gdUnit4 reports **orphan nodes** per test, and this binding's one novel problem
  is that `RefCounted` has no cycle collector. Orphan detection turns "`ctx.scope()`
  is the sole strong owner and teardown actually frees" from an assertion nobody
  writes into a property the harness checks on every test. GUT has no equivalent.

### Phase 1 — Cell kernel (coverage row 0) + the loading invariant

Row 0 surface, verbatim from `coverage.json`: two cell kinds (nodes
`SourceCell`/`ComputedCell`; handles `Source<T, M>`/`Computed<T>`) + `Effect` sink
+ eager `Computed` (`computed().eager()`) / all cells guarded / batch.

- `Source` / `Computed` / `Effect`, `Context`, **explicit ctx threading (no
  ambient — it is gone in all 9 bindings)**, reads via `get`. Not `signal` /
  `get_signal`: that is the deprecated pre-kernel shape.
- WeakRef back-edges + scope-as-sole-owner, per the decision above.
- Edge index on `get_instance_id()`, O(1) dedup, prune-on-propagate.
- `ctx.scope()` / `disarm()`, teardown in reverse creation order.
- **No observers — only Effects.** Banned on every reactive.

**Exit criteria include a load-graph test**, not just green fixtures: assert that
loading the kernel loads no other family. This belongs in Phase 1, not late —
retrofitting layering after six families exist is the expensive order. It grows
one assertion per family thereafter.

### Phase 2 — Runner + anti-vacuous-green guard

Do not defer past Phase 1. Copy `lazily-cs`'s mechanism, which already solved this:

- Fixture interpreter for `shape: steps`, starting `kind: ReactiveGraph` /
  `model: Context`. Load via `FileAccess` + `JSON.parse_string`.
- **Runtime manifest recorder** → `build/conformance-fixtures-loaded.txt`,
  recording every corpus file the run actually reads.
- Port `scripts/check-conformance-coverage.sh` with `EXTS=(".gd")`. Its own
  comments name the failure it exists to catch: a fixture named in a comment but
  **hand-transcribed** — the drift found in `lazily-cpp`'s queue tests — which a
  source grep cannot see. A **missing manifest fails**: that is missing evidence,
  not "no fixtures were read."
- `KNOWN_UNCOVERED` starts populated with everything outside rows 0/15 and shrinks
  per phase. Explicit and reviewed, never a silent skip.
- Assertions target **the library's own state** (dependent/dependency set sizes,
  which effects ran), never runner bookkeeping counters.
- **Mutation-check the runner** before trusting any green: break the kernel
  deliberately, confirm red.

### Phase 3 — Stable-id alignment (coverage row 15)

Manufactured identity. Second row `lazily-cs` shipped, so the reference
implementation and its fixtures already exist.

### Phase 4 — Loading discipline as shipped contract

- Verify empirically that `class_name` registration alone does not trigger a load.
  Load-bearing for the whole layering; cheap to probe, do not assume.
- Export-exclude runner + 134 fixtures from the shipped addon.
- Extend the load-graph test to every family added so far.

### Phase 5+ — Families, one coverage row per phase

Order by fixture count and dependency: Collections (20) → StateChart (7) →
Command (8) → Topic/WorkQueue → Crdt/LosslessTree (9) → distributed set
(ReliableSync 8, Ingress 7, RateShape 6, Coordination 5, …). Thread-safe family
last; be prepared to park it given GDScript threading constraints.

Each phase: shrink `KNOWN_UNCOVERED`, extend the load-graph test, flip the
`coverage.json` mark.

## Constraints

- Adding the `GDScript` column to `lazily-spec/coverage.json` is **one commit,
  GDScript column only** — that file is hot across concurrent sessions.
- Do not touch the parked `pending/mergefeed-fixtures` branch.
- Release mechanics: tag + `gh release`, like cpp and zig. No central registry;
  Godot Asset Library is optional and separate.
- Interpreted performance is expected; per-binding thresholds already differ across
  the 9, so calibrate rather than inherit.

## Resolved questions

1. **Minimum Godot 4.x version — 4.4.** Decided 2026-08-08 by the operator. Was
   the blocker on Phase 0; see Phase 0 for the reasoning and its consequences.
2. **gdUnit4 vs GUT — gdUnit4.** Decided 2026-08-08, contingent on (1) and
   confirmed against it. See Phase 0.
3. **Thread-safe family — out of scope for a first release.** It was already
   being treated as parked elsewhere: `#lzcoveragelayout` Phase 2 introduced the
   `⊘` not-applicable mark specifically noting it "waits for GDScript's parked
   thread-safe family". Recorded here so the plan stops carrying it as open.
