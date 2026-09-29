---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
  codex: 01a0e8bc-26bd-7993-a500-468e9bd767ab
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
prompt_presets:
  '#actionable': Add actionable tasks into backlog + queue
  '#actionable-review': Add actionable review items into backlog + queue
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260927-104050.md`*

Compacted content:
- Archived 15 response topic(s): durable runtime admission closeout; root instruction-files target paths; actionable review extraction; 12 more
- Prior summary/context: prior compacted content: Archived 37 response topic(s): Archived 11 response topics; #actionable-review; Codex attrib...

### Re: are the dependencies feature-gated — opus-5 · 2026-09-27T07:04-04:00

> **Queue prompt:**
>
> do [#dependenciesgatedfeature]


> **Queue prompt:**
>
> Are dependencies gated on feature flags? Such as postgres, temporal, nats, etc? If not, can we make the dependencies optional?

I audited all fifteen `lazily-*` manifests. **Postgres and NATS were already gated everywhere they appear. There is no `temporal` dependency in the family at all** — the only match is `test_temporal_conformance` in lazily-cpp, which is the time/clock conformance test, not temporal.io.

But the question found two real leaks, both in the *ungated* direction, and both are fixed and pushed.

**lazily-rs: the default build pulled 45 crates, of which 36 were a protobuf toolchain.** Every backend feature was correctly optional — `postgres` behind `durable-postgres`, `async-nats` behind `durable-jetstream`, `rusqlite`, `str0m`, `tokio-tungstenite`, `leptos_reactive`, all of it. What was not gated was `[build-dependencies]`:

```toml
[build-dependencies]
prost-build = "0.14.4"
protoc-bin-vendored = "3.2.0"
```

Cargo resolves, downloads and compiles build-dependencies for **every** build of the crate regardless of which features are on — `build.rs` guarding its body on `CARGO_FEATURE_PROTOBUF` stops the codegen from *running*, not the toolchain from being *built*. So the default graph of a reactive-signals library carried prost-build, petgraph, regex, syn, prettyplease, tempfile, rustix and **all nine `protoc-bin-vendored-*` platform crates**, each embedding a full protoc binary.

Both are now `optional = true` and named in the feature (`protobuf = ["dep:prost", "dep:prost-build", "dep:protoc-bin-vendored"]`), with `build.rs`'s body behind `#[cfg(feature = "protobuf")]`. **Measured: 45 crates → 9** (`arc-swap`, `lazily`, `lazily-macros`, `proc-macro2`, `quote`, `rustversion`, `smallvec`, `syn`, `unicode-ident`). A cold `cargo build --features protobuf` in a separate target dir still compiles 13 prost/protoc crates and still emits `lazily.graph_boundary.v1.rs`, so the opt-in path is intact.

**lazily-js: `koffi` and `protobufjs` were hard `dependencies`.** The koffi one was a manifest-versus-code contradiction: `src/ffi-node.js` requires it inside a try/catch and its own error message says `npm i koffi`, while `package.json` made it mandatory — so every consumer of the reactive core installed a native FFI addon. `protobufjs` is imported only by `build/generated/graph-boundary.js`, reachable solely through the `./protobuf-graph-boundary` export subpath. Both are now optional peers (`peerDependenciesMeta.optional`, which npm does not auto-install) and stay in `devDependencies` so this repo's own suite still exercises them. Proven against an **empty `node_modules`**: `src/index.js` imports with 108 exports, `src/ffi-node.js` with 11 (its try/catch holds), and only `src/protobuf-graph-boundary.js` fails, with `ERR_MODULE_NOT_FOUND` naming protobufjs.

**The other thirteen are clean, and two are exemplary.** lazily-py ships `mypy-extensions` alone with attrs/msgspec/pydantic/prometheus_client as extras, and its own comment already explains they are install ergonomics rather than dependencies. lazily-dart, lazily-zig and lazily-cpp have **zero** runtime dependencies (zig's `.dependencies` table is entirely commented out; cpp is a header-only INTERFACE target with no `find_package`/`FetchContent`). lazily-cs scopes `System.Text.Json` to `netstandard2.1` only, correctly, since net8/net10 carry it in-box. lazily-react and lazily-html depend on `@lazily-hub/lazily-js` as a peer. lazily-formal, lazily-spec, lazily-interop, lazily-gd carry nothing relevant.

**Each fix carries a guard, and I attacked both rather than reasoning about them.**

- `make default-build-dependency-check` (`scripts/default-build-dependency-guard.py`, now a `make check` dependency) measures the graph **cargo resolves**, not the manifest prose, and pins the default set as an exact equality in both directions. Its second half is the control that keeps the first honest: all 17 gated crates must be **present** under `--all-features`, because a `cargo tree` that silently produced nothing would otherwise satisfy the absence check vacuously and report green over an empty set. Restoring the pre-fix manifest reddens it with the 36 crates named; seven mutations of its own paths (comparison neutered, control neutered, empty-output refusal dropped, exit-status refusal dropped, tree-parse proof dropped, and both directions of the pin) each redden `--self-test`.
- `test/package-manifest.test.js` in lazily-js asserts the set of bare package specifiers the shipped code imports **equals** the set the manifest declares optional, in both directions, so a new undeclared import and a pin nothing imports both fail. It strips comments first (`state-projection.js` carries a JSDoc import example that would otherwise read as a dependency) and carries two floors so a walk that lost the tree cannot report "no undeclared imports" over nothing. Eight mutations each reddened the intended assertion.

One control worth naming because it was the load-bearing one: half-fixing lazily-rs — build-deps optional but *not* added to the feature — makes `cargo build --features protobuf` fail outright, so the feature wiring cannot be quietly omitted.

**The first full lazily-rs run came back RED, and correctly so.** `check-ci-reach` refused the new gate because it was not pinned in `EXPECTED_CLOSURE_TARGETS` — that guard exists to make retiring a gate an incomplete edit, and adding one is meant to touch the pin. Fixed properly rather than loosened: the target joins `EXPECTED_CLOSURE_TARGETS`, `ci.yml` gains a step running both its commands, `EXPECTED_GATE_STEPS` pins that step name, and the closure-size comment is refreshed to the measured 57 (it already read a stale 52). The workflow YAML parses and the step carries both commands verbatim.

**Verification.** lazily-js `make check` green at exit 0, **691/691 tests pass, 0 fail**, zero `✖` lines, and the four new tests ran inside that run — committed `82e5d1e`, pushed. lazily-rs `make check` green at exit 0 on the re-run: **908 test-result blocks, 7195 passed, 0 failed, zero `test result: FAILED`**, including `test-durable-postgres` 5/5 and `test-durable-jetstream` 1/1, with the new gate running as the run's second step — committed `045f99e`, pushed. I did not touch either repo's `AGENTS.md`: both are currently modified by another session, so documenting the gate there would entangle this commit with theirs; the Makefile comment, the script docstring and the CI step name carry it instead.

**Two leaks remain, both queued at the top of the backlog** — I am not gating them as review items, they are agent-executable: `#lzktoptionaldeps` (lazily-kt's four unconditional Gradle `implementation` deps, of which protobuf-kotlin 4.31.1 and kotlinx-serialization-json are codec-only) and `#lzgooptionalpgx` (lazily-go's root `go.mod` requires `jackc/pgx/v5` plus six indirects for a dependency exactly **one test file** imports and **zero** non-test files do; Go has no optional deps, so it needs a nested integration module).

Exchange is at 365 lines across 14 response sections, over the warn thresholds; the queue is active so I am not pausing to compact.

### Re: #lzktoptionaldeps protobuf codec gated, published graph pinned — opus-5 · 2026-09-27T16:43-04:00

> **Queue prompt:**
>
> do [#lzktoptionaldeps]


Done and pushed to `lazily-hub/lazily-kt` main (`b2d15cb`, plus a docs follow-up `4a3efa7`).

**The leak is gone.** `ProtobufGraphBoundary.kt` and the `../lazily-spec/proto` source dir moved out of `main` into a `protobufCodec` source set, published as a Gradle feature variant with an explicit `io.github.lazily:lazily-protobuf-codec` capability. `protobuf-kotlin` is now `<optional>true</optional>` in the generated POM — which Maven does not resolve transitively — and lives only under the gated variant in Gradle Module Metadata. The main jar went 1.7M → 1.5M and contains zero protobuf classes.

The capability name is set explicitly rather than taken as Gradle's default. Gradle derives it from the **project** name (`lazily-kt`), not the published artifactId (`lazily`), so consumers would have been told to request `io.github.lazily:lazily-kt-protobuf-codec` while depending on `io.github.lazily:lazily`. The guard pins the string.

**One premise in the item was wrong, and I did not act on it.** `kotlinx-serialization-json` is not "only the JSON codec" — nine main sources use it, including `StateChart.kt`'s `ChartDef.fromJson`, the Harel chart parser this module's own POM description advertises, plus `Ipc.kt`, `Command.kt`, `Capability.kt`, `Receipt.kt` and `Signaling.kt`. Gating it means splitting the wire plane out of the library, which is a different change from removing a dependency nothing in the default surface uses. It stays core, and the reason is recorded next to the pin so the next reader does not re-derive it. Default graph is now four artifacts: `kotlin-stdlib`, `jna`, `kotlinx-coroutines-core`, `kotlinx-serialization-json`.

**The pin reads the generated documents, not `build.gradle.kts`.** `scripts/check-published-dependencies.py` parses the generated POM **and** the Gradle Module Metadata, because what a consumer resolves is not the text of a dependency block — the Kotlin plugin's implicit `kotlin-stdlib`, the `-jvm` artifacts Gradle substitutes for the multiplatform coordinates, and the `<optional>` marker a feature variant produces are all plugin behaviour, and reading the manifest would not have caught this leak in the first place. Four sets, each compared in both directions.

Both documents are measured because each closes a hole the other leaves open. Module metadata is invisible to Maven, so a POM-only regression passes a metadata-only check. And `<optional>true</optional>` is fifteen characters of XML: a POM-only guard is satisfied by a dependency gated in the POM that no feature variant stands behind — Maven protected, every Gradle consumer still downloading the codec — and satisfied just as happily by a capability renamed out from under the documented coordinate, because POMs do not carry capabilities. So the gated dependency must appear under a variant carrying the exact capability string, and that variant must publish at least one file, so an empty shell cannot stand in for the codec. Unparseable or empty measurements **refuse** (exit 2) rather than satisfying a set-equality over nothing, and the pins themselves are checked for vacuity before any document is read.

**Mutation-checked, six cases, green control before and after:**

| mutation | result |
|---|---|
| M0 control | exit 0 GREEN |
| new unconditional `implementation` | exit 1 — `com.squareup.okio:okio-jvm` published but not pinned |
| codec moved back into the default graph | caught (the gradle build breaks first) |
| codec declared in BOTH main and the feature | exit 1 — caught **three times over**: POM default gains it, POM optional loses it, module `runtimeElements` gains it |
| capability renamed | exit 2 — 0 runtime variants carry the pinned capability |
| pin names an undeclared dependency | exit 1 — `org.example:ghost` pinned but not published |
| gated pin emptied | exit 2 — refused as vacuous |

Restores came from scratch copies with a `touch`, never `git checkout --`, so the fix under test was never reverted mid-run, and every mutation was asserted applied before the guard ran.

**Wired and verified.** New `published-dependency-check` target in `make check`, a `Published dependency floor (#lzktoptionaldeps)` step in `ci.yml`, and both `EXPECTED_CLOSURE_TARGETS` and `EXPECTED_CI_STEPS` in `check-ci-reach.sh` updated — the reachability guard re-proves on every run that the gate is actually reached by CI. `make check` exit 0: **511 tests, 0 failures**, 149/161 canonical fixtures replayed per the runtime manifest, spotless, interop peer, both Lean models, assertion ordering, ci-reach all green.

README gained an `## Optional feature` section with the Gradle capability request and the Maven classifier form. It uses version placeholders, not `0.42.0`: **v0.42.0 is already tagged without this change**, so the split lands in the next release and the section says so rather than implying a published artifact carries it. Also corrected the stale `v0.41.1` coordinate line.

**CI:** run `36349017938` is pending — pushed minutes ago, not yet started. Local `make check` is the evidence above; nothing in CI differs except a fresh checkout of the siblings.

**Next:** `#lzgooptionalpgx` is the queue head. I also filed `#lzktoptionaljna` (appended, not preempting) — `jna` is now the largest thing left in kt's default graph at ~1.9 MB with exactly one importer, and the pattern to remove it is the one that just landed. Its real question is how much of `LazilyFfiBoundary` the conformance suite drives through JNA versus the pure-JVM channel, which is why it is a separate item rather than something I folded into this commit.

### Re: #lzgooptionalpgx pgx gated by relocation, published graph now empty — opus-5 · 2026-09-27T21:07-04:00

> **Queue prompt:**
>
> do [#lzgooptionalpgx]

Done and pushed to `lazily-hub/lazily-go` main (`b68667f`).

**The root `require` block is now empty** — not shortened, empty. `go mod edit -json` reports `Require: None`, and `go.sum` went to zero bytes. pgx plus its six indirects are gone from every consumer's resolution. The suite that needs a driver lives in `integration/postgres`, its own module with a `replace` back to the root, which no consumer reads.

**The item said "the Postgres integration test", and there were two.** `sim_projection_maintenance_integration_test.go` calls `sql.Open("pgx", ...)` twice and imports nothing — it was relying entirely on the blank `_ "github.com/jackc/pgx/v5/stdlib"` import in `postgres_projection_barrier_integration_test.go` to register the driver. Moving only the importing file would have left it opening a driver no longer registered in its binary, and the failure would have surfaced as a runtime `unknown driver "pgx"` rather than a compile error. Both moved. The registration now sits in a shared helper file with a comment, so the package's reason for being a separate module is stated once instead of being incidental to whichever file happened to import it.

**That forced one decision, and it was yours to make.** The sim test called `expectedSimProjectionOutcome`, unexported production logic in `sim_projection_maintenance.go`. Reaching it from another module meant exporting it or forking it. Exporting won, and it closes a real gap that predates this task: `(*SimConsumerTestkit).RunProjectionMaintenance` is **exported**, and it compares every adapter's applied outcome against that oracle — so no out-of-tree consumer could write a correct adapter for an exported entry point. It is now `ExpectedSimProjectionMaintenanceOutcome`, with a doc comment saying callers must not reimplement the switch, because a copy is free to drift from the behaviour it is meant to pin. The other four helpers are test scaffolding, not production logic, so the nested module builds its own purpose-built adapter rather than importing the root's counter-reducer factory — that one carries a reducer this suite immediately overwrites.

**The guard needs both halves to prove either.** `scripts/check-module-dependencies.sh` asserts the root `require` set is empty **and** that the relocated suite still exists, builds, and declares the tests pinned for each module, compared as set equality in both directions. The second half is not belt-and-braces: **deleting the suite satisfies the first check perfectly**, so an emptiness-only guard would report green on the worse regression. It pins the root's two tests as well as the nested module's four, so "move the database tests out" cannot quietly become "move everything out". Half A reads `go mod edit -json`, not go.mod's text — a grep for `^require` is defeated by a single-line require, by a `// require` comment, and by any reformatting `tidy` chooses. Every measurement refuses with exit 2 when it cannot be taken, rather than reading "nothing measured" as "nothing wrong".

**Mutation-checked, eight cases, green control before and after:**

| mutation | result |
|---|---|
| M0 control | exit 0 GREEN |
| pgx re-required in root | exit 1 — `must require NOTHING` |
| relocated suite deleted | exit 2 — `ZERO matching tests` |
| nested module's go.mod gone | exit 2 — `relocated suite is gone` |
| suite stops compiling | exit 2 — `does not build` |
| pinned nested test renamed | exit 1 — `missing pinned test` |
| root unit test dragged out | exit 1 — `missing pinned test` |
| pin emptied | exit 2 — `pins nothing` |
| observation filter neutered | exit 2 — `ZERO matching tests` |

Restores came from scratch copies with a `touch`, never `git checkout --`, every mutation was asserted applied before the guard ran, and all seven mutated files were verified byte-identical to their pristine copies afterwards.

**Wired and verified.** New `module-dependency-check` target in `make check`, a `Published module dependency floor (#lzgooptionalpgx)` step in `ci.yml`, and both `EXPECTED_CLOSURE_TARGETS` and `EXPECTED_GATE_STEPS` extended so `check-ci-reach` re-proves CI reaches it — it now reports 12 closure targets set-equal, 11 gates step-mapped. `make check` exit 0, zero `FAIL` lines.

**The coverage did not shrink, and I checked rather than assumed.** `go test ./...` does not descend into a nested module, so relocating the suite without updating the runner would have silently stopped running it — green, with the tests gone. `scripts/test-postgres-projection-barrier.sh` now runs both modules. Against a real PostgreSQL 18.6: **6 PASS, 0 SKIP**, the same four integration tests as before the move plus the two unit tests that correctly stayed in the root package. `ok` alone would not have shown this — those tests skip when `LAZILY_POSTGRES_URL` is unset, so the tally is read from `--- PASS` / `--- SKIP` lines, not the package summary.

**CI is pending, not green.** Run `36364741036` was queued seconds after the push; the two prior runs on main are green. Local `make check` is the evidence above.

I left `AGENTS.md` alone — it is modified by another session, and documenting the gate there would entangle this commit with theirs. The Makefile comment, the script header, `integration/postgres/doc.go` and the CHANGELOG carry it instead.

With this and `#lzktoptionaldeps` landed, `#lzktoptionaljna` (lazily-kt's 1.9 MB JNA in the default published graph) is the last dependency leak in the queue.

### Re: #lzktoptionaljna JNA gated, the pin became a map — opus-5 · 2026-09-28T11:43-04:00

> **Queue prompt:**
>
> do [#lzktoptionaljna]

Done and pushed to `lazily-hub/lazily-kt` main (`4909788`).

**The last leak is closed.** `net.java.dev.jna:jna` — **2,001,532 bytes**, measured from the resolved jar in the Gradle cache, not quoted from the item — now lives behind an `io.github.lazily:lazily-ffi` feature variant. The default POM graph went from four artifacts to three: `kotlin-stdlib`, `kotlinx-coroutines-core`, `kotlinx-serialization-json`.

**The item's open question has a clean answer: none of `LazilyFfiBoundary.kt` needs JNA.** It asked how much of that file genuinely needs JNA "rather than the pure-JVM channel", flagging it as the hard part. It is all pure JVM. Despite the name, `LazilyFfiNative` is the entry-table *shape* a native peer links against — Graal `--export-symbols`, or a JNI shim — and it delegates to `LazilyFfiChannel`, which encodes and decodes `IpcMessage` in Kotlin. The file has **no imports at all** and never references `LazilyFFI`. `LazilyFfiBoundaryTest` exercises `Channel`/`Native`, never the JNA interface, and nothing outside its own file referenced `LazilyFFI` — no test, no conformance fixture, no other main source. So the conformance surface stays in `main`, compiled and replayed exactly as before, and only the 63-line binding moved.

**The guard change is the substantive part, and it is structural rather than additive.** The pin held one `REQUIRED_FEATURE_CAPABILITY` and one `GATED_MODULE_RUNTIME`. Adding a string to a set would have been the wrong fix: with a single capability pinned, a publication that gated jna under a variant nobody named — or under the codec's own capability — satisfies every comparison, because nothing asks *which* variant carries *which* graph. `GATED_MODULE_RUNTIME_BY_CAPABILITY` now measures each capability against its own runtime graph, and a new floor refuses a coordinate pinned under two features, since neither variant's pin could then fail on it.

**Mutation-checked, eight cases, green control before and after:**

| mutation | result |
|---|---|
| M0 control | exit 0 GREEN |
| pin names an undeclared dependency | exit 1 — `org.example:ghost` pinned but not published |
| a gated pin emptied | exit 2 — empty runtime graph |
| the capability map emptied | exit 2 — names no capability |
| one dep pinned under BOTH capabilities | exit 2 — neither capability's graph can fail on it |
| jna ungated in main only | exit 2 — REFUSED, the Gradle build breaks first |
| the `lazily-ffi` capability renamed | exit 2 — 0 runtime variants carry the pinned capability |
| jna declared in BOTH main and ffi | exit 1 — caught **three times over** |
| a new unconditional dependency | exit 1 — `okio` published but not pinned |

**My first harness run was the instrument failing, not the guard working.** It reported `exit=127` for all four mutations — and for the M0 control. zsh does not word-split an unquoted command variable, so nothing had executed; read naively, four non-zero exits look like four catches. A non-zero exit is not a catch. Re-run with the command inline, the control went green and the mutations separated into 1s and 2s.

**Verified against the generated artifacts, not `build.gradle.kts`.** The POM carries `<optional>true</optional>` for jna; `ffiRuntimeElements` carries the pinned capability with jna + kotlin-stdlib and publishes a real `lazily-0.42.0-ffi.jar`; the main jar contains **0** `LazilyFFI` classes and all **4** `LazilyFfi` boundary classes. My first size reading came off a stale `lazily-kt-0.40.0.jar` still sitting in `build/libs` and was discarded — the current jar is `0.42.0`.

**`make check` exit 0, and the test count is read from the Gradle XML results rather than console output** — `511 tests, 0 failures, 0 skipped` across 79 result files, matching the count before the split. That distinction matters here: the RTK filter strips Gradle task lines, so a skipped test task reads as a passing one on stdout.

**CI.** lazily-kt run `36445762527` is queued; the previous run on main is green. And the pending run from last cycle has landed: **lazily-go `36364741036` completed successfully** (1m57s), so `#lzgooptionalpgx` is CI-green as well as locally verified.

The README gained an `## Optional feature — JNA FFI binding` section with the Gradle capability and Maven classifier forms, using version placeholders and stating that the split lands in the release after v0.42.0, since v0.42.0 is already tagged without it. I left `AGENTS.md` alone again — still modified by another session.

With this, all three dependency-gating items are done: protobuf codec (`#lzktoptionaldeps`), lazily-go's pgx (`#lzgooptionalpgx`), and JNA. Every published graph in the family that had a leak now requires only what its default surface actually uses.
### Re: recovered response

Review work is complete, but document closeout is not yet committed.

- All four review gates remain freshly blocked or intentionally unprobeable; none has stale evidence.
- No artifacts were deleted, accounts changed, credentials resolved, or source changes retained.
- The scoped review and credential proofs passed; latest agent-doc CI is green.
- Finalization failed closed because the live editor authority diverges from disk and the shell pane is not the registered owner.
- A reopen is queued in owner pane `%188`; the controller owns recovery. No force-write or resubmission was attempted, and nothing was pushed.
1. Prepare clean version-bump commits and run every repository's full `make check` (plus registry dry-runs where available).
2. Release `lazily-spec v0.40.0` first.
3. Release `lazily-formal`, then the independent core/bindings. Ensure `lazily-js` precedes `lazily-react`; do not tag Rust until crates.io Trusted Publishing is configured.
4. Treat the source releases for Godot and the eventual Asset Library submission as separate deliverables. Keep HTML out of this train until its public-repository bootstrap is complete.
5. Run `make -C src/lazily-interop check` and `make -C src/lazily-interop network-suite-merge` against the chosen release revisions.
6. Verify each registry/artifact after publication: Maven, Go proxy, npm, both NuGet packages, PyPI's full wheel matrix, pub.dev, crates.io, and all GitHub releases. Only then update and push the superproject pins.
> **Queue prompt:**
>
> What needs to be done to release + publish?

### Re: Lazily release and publication readiness — GPT-5 · 2026-09-29T00:10-04:00

A full Lazily release train is warranted: every standalone repository has commits beyond its latest remote release. No release action was taken in this audit—no files, tags, pushes, credentials, registry entries, or releases were changed.

The immediate rule is: **do not release from the current checkouts**. They all contain user-owned instruction/agent state, and some local tag refs are stale. Prepare each release in a clean worktree or fresh clone, fetch tags, and stage only intentional version/release-note files.

#### Proposed release scope

| Project | Published baseline | Proposed release | Publication path / required work |
|---|---:|---:|---|
| `lazily-spec` | `v0.39.0` | `v0.40.0` | Run `make check`; tag and create a GitHub release. It is not a registry package. Release this first because binding CI consumes the published corpus. |
| `lazily-formal` | `v0.38.1` | `v0.39.0` | Local `make check` is the only gate; there is no CI workflow. Create a source GitHub release. |
| `lazily-rs` | `0.58.0` | `0.59.0` | Bump the root crate and generated/versioned references, then `make check`. **Blocked:** configure crates.io Trusted Publishing for `lazily-hub/lazily-rs` / `release.yml`; recent tag workflows fail closed because this publisher is absent. The separate `@lazily/signaling` package is unpublished and outside the Rust release workflow. |
| `lazily-kt` | `0.42.0` | `0.43.0` | Bump Gradle version, interop-peer version, and README; tag-triggered workflow publishes Maven artifacts to GitHub Packages. |
| `lazily-go` | `v0.27.0` | `v0.28.0` | Update the interop-peer version, check, tag, create GitHub release, then warm/verify the Go proxy. |
| `lazily-js` | `0.33.0` | `0.34.0` | Bump package/lock, interop peer, and generated README; tag-triggered OIDC workflow publishes npm. Publish before React. |
| `lazily-react` | tag `v0.2.0`; npm absent | `0.3.0` | Bump package/lock, check, and `npm pack --dry-run`. **Operator decision/blocker:** the package has never been published; confirm npm org permission/authentication and whether tagged `0.2.0` should be back-published before `0.3.0`. |
| `lazily-cs` | NuGet `0.5.0` | `0.6.0` | Bump both package projects and interop peer; tag workflow publishes both NuGet packages. Separately create the missing GitHub Release object for the already-published/tagged `v0.5.0`. |
| `lazily-py` | `0.41.1` | `0.42.0` | Bump `pyproject.toml`, `__init__.py`, and changelog; tag workflow publishes PyPI wheels. Preserve the hosted `macos-14` matrix and both x86_64/arm64 deliverables; the current release has all six expected Darwin wheels. |
| `lazily-dart` | `0.31.0` | `0.32.0` | Bump pubspec/changelog and run `dart pub publish --dry-run`; publication is a manual authenticated `dart pub publish`. |
| `lazily-cpp` | `v0.29.0` | `v0.30.0` | Bump CMake version, check, and create a source-only GitHub release. The documented durable-owner capability gap should remain explicit. |
| `lazily-zig` | `v0.35.0` | `v0.36.0` | Bump `build.zig.zon`, check, and create a source-only GitHub release. |
| `lazily-gd` | `v0.1.0` | `v0.2.0` | A source-only GitHub release is ready after checks. **Godot Asset Library publication is blocked** on addon metadata/package structure and an operator submission; no registry entry currently exists. |
| `lazily-html` | none | initial `0.1.0` | **Not release-ready:** the declared public GitHub repository does not exist, npm returns 404, and the private superproject copy tracks `node_modules`. Extract/create the public repo, preserve history, remove tracked dependencies/add `.gitignore`, add CI and trusted npm publication, then run `npm ci`, tests, and `npm pack --dry-run`. |
| `lazily-interop` | private `0.1.0` | no publication | This is an integration gate, not a package. Run it last against exact released revisions, then update only the intended superproject pins. |

#### Recommended order

1. Prepare clean version-bump commits and run every repository's full `make check` (plus registry dry-runs where available).
2. Release `lazily-spec v0.40.0` first.
3. Release `lazily-formal`, then the independent core/bindings. Ensure `lazily-js` precedes `lazily-react`; do not tag Rust until crates.io Trusted Publishing is configured.
4. Treat the source releases for Godot and the eventual Asset Library submission as separate deliverables. Keep HTML out of this train until its public-repository bootstrap is complete.
5. Run `make -C src/lazily-interop check` and `make -C src/lazily-interop network-suite-merge` against the chosen release revisions.
6. Verify each registry/artifact after publication: Maven, Go proxy, npm, both NuGet packages, PyPI's full wheel matrix, pub.dev, crates.io, and all GitHub releases. Only then update and push the superproject pins.

The currently published `main` heads and latest CI are green for all audited standalone repositories except `lazily-formal`, which has no hosted CI and relies on its local check. The root Lazily network workflow is intentionally manual/disabled, so its local interop result is required release evidence.

This audit used the repository's `tsift` navigation workflow to bound source and release-workflow inspection. A separate advisory remains: the live JetBrains agent-doc plugin is newer than the binary-bundled plugin; it does not block this release audit and should not be reinstalled during this cycle.

### Re: Lazily release train executed — GPT-5 · 2026-09-29T01:49-04:00

> **Queue prompt:**
>
> do the necessary steprs to release + publish

The release train is complete everywhere the existing publication authority could be proven. All release work used isolated clean clones/worktrees; none of the dirty user-owned checkouts was staged or overwritten.

#### Published and verified

| Project | Release | Verification |
|---|---:|---|
| `lazily-spec` | `v0.40.0` | Full check green; [GitHub release](https://github.com/lazily-hub/lazily-spec/releases/tag/v0.40.0). |
| `lazily-formal` | `v0.39.0` | Full check green; [GitHub release](https://github.com/lazily-hub/lazily-formal/releases/tag/v0.39.0). |
| `lazily-cpp` | `v0.30.0` | Commit `39f14f4`; full check and CI `36526156096` green; [GitHub release](https://github.com/lazily-hub/lazily-cpp/releases/tag/v0.30.0). |
| `lazily-zig` | `v0.36.0` | Commit `65cdc62`; full check and CI `36526158130` / `36526157274` green; [GitHub release](https://github.com/lazily-hub/lazily-zig/releases/tag/v0.36.0). |
| `lazily-kt` | `v0.43.0` | Commit `0ecfdfe`; CI and Release workflows green; Maven artifacts verified in GitHub Packages; [GitHub release](https://github.com/lazily-hub/lazily-kt/releases/tag/v0.43.0). |
| `lazily-go` | `v0.28.0` | Commit `af4d845`; CI green; Go proxy resolves the exact tag/commit; [GitHub release](https://github.com/lazily-hub/lazily-go/releases/tag/v0.28.0). |
| `lazily-js` | `v0.34.0` | Commit `d0131ea`; CI/Release green; npm `latest` is `0.34.0`; [GitHub release](https://github.com/lazily-hub/lazily-js/releases/tag/v0.34.0). |
| `lazily-cs` | `v0.6.0` | Commit `3c2d03d`; CI/Release green; both `Lazily` and `Lazily.R3` `0.6.0` verified on NuGet; [GitHub release](https://github.com/lazily-hub/lazily-cs/releases/tag/v0.6.0). The missing [v0.5.0 release object](https://github.com/lazily-hub/lazily-cs/releases/tag/v0.5.0) was also repaired. |
| `lazily-py` | `0.42.0` | Commit `94177ac`; workflows green; PyPI has 21 wheels plus one sdist, including all six required CPython 3.12–3.14 macOS x86_64/arm64 wheels; [GitHub release](https://github.com/lazily-hub/lazily-py/releases/tag/v0.42.0). |
| `lazily-dart` | `0.32.0` | Commit `62e0910`; dry-run and authenticated pub.dev publication succeeded; CI green; [GitHub release](https://github.com/lazily-hub/lazily-dart/releases/tag/v0.32.0). |
| `lazily-gd` | `v0.2.0` | Full check passed with official Godot 4.4.1 and released Spec fixtures; [source GitHub release](https://github.com/lazily-hub/lazily-gd/releases/tag/v0.2.0). |

#### Prepared but intentionally not tagged

- `lazily-rs` `0.59.0`: prep commit `ea2910d942fca5498f811b7ec88248585fb9f5c3` is pushed. Full `make check` and `cargo publish --dry-run --locked` passed. No tag was created because crates.io still reports no Trusted Publishing configuration for `lazily-hub/lazily-rs` / `release.yml`; creating the tag would knowingly trigger a failed release. The dry-run also reports the locked `spin 0.9.8` as yanked, but packaging succeeds.
- `lazily-react` `0.3.0`: prep commit `dc2198713cdce925bccb5651ae6229ed27cf5347` is pushed, CI and `npm pack --dry-run` pass, and its Lazily JS peer minimum is now `^0.34.0`. No tag or npm publication was created because `npm whoami` failed and first-publication org authority is unproven.
- `lazily-html` `0.1.0`: not released. Its declared public repository does not exist, npm returns 404, and the private embedded copy still needs repository/history, dependency-hygiene, CI, and trusted-publication bootstrap.
- Godot Asset Library publication remains separate from the completed `lazily-gd` source release; addon metadata/package preparation and authenticated submission remain.

#### Cross-binding and closeout evidence

`make -C src/lazily-interop check` passed: 13 tests, zero failures, schema validation green, and all 11/11 required mutations caught. `make -C src/lazily-interop network-suite-merge` also passed at seed 29. This ran against the checked-out post-feature heads; the release-only version commits do not alter protocol behavior.

I did not update or push the superproject gitlinks: the superproject is 57 commits ahead of `origin/main` and has extensive unrelated user-owned changes. Folding release pins into that state would violate the scoped-commit boundary. A clean pin update plus exact-release interop rerun is captured as follow-up work.

The agent-doc stale-install warning was advisory and this cycle did not rebuild agent-doc. Its latest CI run `36527892614` is still in progress; local Lazily release verification is the evidence above.
<!-- agent:boundary:cfd052cf -->
<!-- /agent:exchange -->
 
## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 do the necessary steprs to release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzhtmlbootstrap] Bootstrap the public lazily-hub/lazily-html repository with preserved history, dependency hygiene, CI, trusted npm publication, and release 0.1.0.
- [ ] [#lzgdasset] Prepare Godot Asset Library addon metadata and package for lazily-gd 0.2.0, then complete the authenticated Asset Library submission.
- [ ] [#lzreleasepins] After the other superproject sessions settle, update only the Lazily gitlinks to released commits, rerun the exact-revision interop gate, and push the intended pin commit without unrelated workspace changes.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzreactpublish] [operator-decision] Establish npm org authorization for the first @lazily-hub/lazily-react publication; then publish, tag, and release prepared 0.3.0 commit dc21987. Blocked because npm whoami failed and the package is still absent.
- [/] [#lzrustpublish] [operator-decision] Configure crates.io Trusted Publishing for lazily-hub/lazily-rs workflow release.yml; then tag and publish the prepared v0.59.0 commit ea2910d and verify crates.io. Blocked solely on the missing publisher configuration.
- [/] [#lzmacosrunner] [operator-decision, agent-doc/lazily-py] Current agent-doc policy intentionally keeps macOS out of GitHub Actions and uses operator-built Darwin assets via `make release-macos-assets TAG=<chosen-current-release>` no more frequently than weekly. Measured by `make review-gate-status` (gate `#lzmacosrunner`), which pins the five known incomplete-pair releases — v0.35.399, v0.35.400, v0.35.401, v0.35.406, v0.35.417 — and goes `evidence_stale` if that measured list changes in EITHER direction, so a widening gap surfaces without a hand pass. Underlying probes are `make release-macos-cadence-check` (weekly window, open since 2026-09-22T16:41:49Z) and `make release-macos-coverage-check` (exit 2, names the five); neither is a `make check` dependency, because clearing them needs Mac hardware. Keep lazily-py hosted macOS wheels unchanged and never drop either deliverable. Blocked until the operator runs the Mac-only release command, or supplies the Mac SSH endpoint, username, architecture, and dedicated build-account confirmation for agent automation.
- [/] [#lzgithubbudget20] Set the btakita personal Actions product budget to $20/month with Stop usage enabled; retain $0 hard-stop budgets for Packages, Codespaces, Git LFS, and AI credits. NOT probeable from here by construction, and `make review-gate-status` now records it as `unprobeable` rather than re-measuring it every cycle: /users/btakita/settings/billing/actions and /users/btakita/settings/billing/usage both return 404 with a `user`-scope hint for this token, and the REST billing API exposes no payment-method field even with that scope. Actions execution itself is not blocked — recent agent-doc CI runs start and pass — so this is spend-limit hardening, not an outage. Blocked until a valid payment method is added to the personal GitHub account. No agent-executable slice.
- [/] [#lzagentdocartifactpurge] [operator-decision, btakita/agent-doc] Read-only audit with counterpart durability classification (commits 667eb4766, 434b32548) lives at src/agent-doc/tasks/agent-doc/artifact-purge-audit-2026-09-27/ and is confirmed the ONLY handoff on disk, so only one basis can be authorized. Run the plan from src/agent-doc with `python3 scripts/purge-actions-artifacts.py`; the destructive run is `--execute --authorize-deletion btakita/agent-doc --max-deletions 3000` and the 250 default cap otherwise aborts. TWO authorizable options, both re-proved by live dry run: (a) full plan — 2,666 artifacts (43.83 GB), 0 refused, 0 skipped; (b) `--durability durable` — 2,136 artifacts (34.83 GB), refusing exactly the 530 rows whose only counterpart is a PyPI file. Prefer (b): the prune floor is 0.35.113 and v0.35.119, the oldest PyPI-only-backed version, is only the THIRD live release above it, so option (a) additionally accepts that 9.01 GB rests on a thinner margin than version numbering suggests. 2,136 rows rest on GitHub release assets, which have not decayed. 571 artifacts are withheld at audit time and are never deletable. Hard deadline: the 7-day `--max-handoff-age-days` guard aborts the whole run after 2026-10-04T05:56:08Z unless the audit is regenerated or `--allow-stale-handoff` is passed; `make review-gate-status` imports that guard from the purge script and flips this gate to `evidence_stale` 24h BEFORE it expires rather than after. Blocked solely on explicit destructive authorization; no artifact has been deleted.
- [/] [#lzambienttokenresolve] [operator-decision, agent-loop superproject] The ready-to-apply point-of-use credential plan is complete at tasks/software/plan-ambient-publish-token-point-of-use.md, and its agent-executable prerequisite [#lzfakepublisherproof] is DONE: `make publish-credential-proof` statically verifies all eight preimages plus `git apply --check`, then exercises all eight production call sites in success, failure, and empty modes against fake passage/cargo/maturin/bun/twine recorders, resolving no credential and invoking no publisher. `make review-gate-status` now runs it as a gate probe and asserts from its JSON report, NOT its exit status, so a red exit over a clean report — or the reverse — reads `evidence_stale` instead of green. The preimage pin has now gone red FIVE times, most recently on 5cb4ee173a, the commit that added the gate target and therefore edited the root Makefile. Since [#lzpatchcontextpin] made the patches context-bearing (7fab345f18) only the recorded hash needs the hand refresh and the hunks relocate themselves — first live confirmation of that half. Expect the hash red on any edit to a patched file and refresh it by hand; do not add an auto-refresh mode. Remaining blockers are operator-only: a quiet window for application, rotation of the Cargo, npm, and PyPI credentials, and rerunning the plan step-5 name loop in a real INTERACTIVE shell after application, because a non-interactive shell does not fire mise activation and forcing it would resolve real credentials. Do not resolve or print credential values during preparation.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
