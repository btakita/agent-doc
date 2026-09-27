---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
  codex: 01a0da83-7762-7810-8ddf-49eda4e9a807
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
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->
 
## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 do [#dependenciesgatedfeature]
- do [#lzktoptionaldeps]
- do [#lzgooptionalpgx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzktoptionaldeps] lazily-kt declares four UNCONDITIONAL Gradle `implementation` dependencies in build.gradle.kts — `net.java.dev.jna:jna:5.15.0`, `org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0`, `org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3`, `com.google.protobuf:protobuf-kotlin:4.31.1` — so every consumer gets all four on its runtime classpath. protobuf-kotlin is needed only by the protobuf codec and kotlinx-serialization-json only by the JSON codec; JNA and coroutines-core are arguably core. Gate the codec deps behind Gradle feature variants (`java-library` + `registerFeature` + capabilities) or separate published artifacts, then pin the PUBLISHED POM's `<dependencies>` with a test asserting the set in BOTH directions, so a new unconditional dep and a pin nothing uses both redden — mirroring lazily-rs `make default-build-dependency-check` (scripts/default-build-dependency-guard.py, wired into `make check` and ci.yml) and lazily-js `test/package-manifest.test.js`. Mutation-check the pin before trusting it. Run `make check` green, then commit and push per lazily-kt's standing rule.
- [ ] [#lzgooptionalpgx] lazily-go's root go.mod declares `github.com/jackc/pgx/v5 v5.7.6` plus six indirects (pgpassfile, pgservicefile, puddle, golang.org/x/crypto, x/sync, x/text) in the main `require` block, but pgx is imported by exactly ONE file — postgres_projection_barrier_integration_test.go — and by ZERO non-test files. Go has no optional dependencies, so the fix is a nested module: move the Postgres integration test into its own directory with its own go.mod (a `replace` directive back to the root), leaving the published module graph requiring nothing. Add a guard asserting the root go.mod `require` block is EMPTY and that the nested module still builds the test, with the second half as the anti-vacuity control — a guard that only checks emptiness passes over a deleted test. Mutation-check it. Run `make check` green, then commit and push per lazily-go's standing rule.
- [ ] 🚧 [#dependenciesgatedfeature] Are dependencies gated on feature flags? Such as postgres, temporal, nats, etc? If not, can we make the dependencies optional?
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
