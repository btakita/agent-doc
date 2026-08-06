---
name: reference_lazily_corpus_perturbation_harness_per_binding
description: "How to point each lazily binding's suite at a SCRATCH corpus copy so a perturbation probe never touches lazily-spec in place"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0a3317a3-3f7b-4666-9d4c-3ec682ae6da6
  modified: 2026-08-03T06:17:10.793Z
---

Never edit `lazily-spec` in place — a probe there reddens every other binding
concurrently ([[reference_lazily_spec_shared_corpus_concurrent_mutation_probes]]).
Each binding resolves the corpus differently, so the scratch redirection differs:

- **py** — runners hardcode `Path(__file__).parents[2]/"lazily-spec"`, so
  `LAZILY_SPEC_CONFORMANCE_DIR` does NOT move them. Copy `tests/ src/ pyproject`
  into `SCRATCH/lazily-py/` beside `SCRATCH/lazily-spec/`, run the repo's
  `.venv/bin/python -m pytest`.
- **js** — `join(here,"..","..","lazily-spec")`. Copy `test/ src/ bin/ scripts/`,
  symlink `node_modules`, run `node --require ./test/support/... --test`.
- **zig** — `CONFORMANCE_ROOT` is CWD-relative. Don't copy the repo: collect the
  built binaries via `zig build test --verbose | grep -oE '\./\.zig-cache/o/[a-f0-9]+/test'`
  and run them with cwd = `SCRATCH/lazily-zig/` (an empty dir).
- **cs** — `SpecCorpus.Locate()` walks up from `AppContext.BaseDirectory`. Copy
  `tests/Lazily.Tests/bin/Debug/net10.0` to `SCRATCH/lazily-cs/bin` AND
  `src/Lazily` to `SCRATCH/lazily-cs/src/Lazily` (one test reads the library
  sources), then `dotnet test Lazily.Tests.dll` from that bin dir.
- **dart** — CWD-relative. Copy `lib test tool bin pubspec.* .dart_tool`.
- **kt** — CWD-relative, but gradle's test working dir can be redirected without
  copying the repo: an init script with
  `allprojects { afterEvaluate { tasks.withType<Test>().configureEach { workingDir = file("SCRATCH/lazily-kt") } } }`
  plus `./gradlew test --rerun --init-script <it>`. The `afterEvaluate` wrapper is
  required — without it the override is silently discarded and the probe reports a
  false clean. Symlink `src` into the scratch cwd; one test reads `src/main/kotlin`.

Always prove the redirection is live before trusting a clean result: perturb one
key the binding definitely asserts and confirm the run goes RED.
