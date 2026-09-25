# Plan: Lazily deterministic simulation and property testing

**Status:** Provenance gate recorded; implementation admission remains blocked by
the `lazily-spec` license decision described below
**Created:** 2026-09-25
**Session:** `tasks/software/lazily.md`
**Backlog:** `#lzsimipgate`, followed by `#lzsimlicense`

## Goal

Define the public-pattern, dependency-license, fixture-provenance, and ownership
boundary for Lazily's deterministic simulation work before implementation. This
gate covers the backend-neutral contracts, Go `SimWorld`, generation and
shrinking, fault plans, oracles, and eventual consumer testkit.

The gate separates learning from public ideas from copying protected expression.
Lazily may implement the abstract testing techniques below, but its APIs, type
names, schemas, traces, examples, fixtures, and implementation must be authored
independently from Lazily's own semantics.

## Gate decision

- Public-source research and private architectural planning may proceed.
- Do not implement or publish simulation contracts or fixtures in
  `lazily-spec` until `#lzsimlicense` records an authorized repository-license
  decision and a fixture provenance policy.
- `lazily-go` implementation must remain independently authored and stdlib-only
  unless a later review adds a dependency to this inventory before its use.
- Do not begin consumer- or employer-related simulation work until the written
  prior-work/employer boundary below is either linked by date and location or
  explicitly marked not applicable by the owner. Never place confidential
  agreement text in a public repository.
- A later phase may proceed only after its reviewer confirms that the applicable
  checklist items in this plan are complete. Recording this gate does not itself
  waive an open condition.

## Public-pattern provenance register

These sources are conceptual references, not dependencies or sources to port.

| Pattern | Primary or official source | Permitted learning | Independent-authorship boundary |
| --- | --- | --- | --- |
| Deterministic whole-system simulation | FoundationDB, [Simulation and Testing](https://apple.github.io/foundationdb/testing.html); source repository is [Apache-2.0](https://github.com/apple/foundationdb/blob/main/LICENSE) | A seeded, deterministic, single-threaded simulated world; logical time; reproducible schedules; injected machine, network, and storage faults | Do not copy Flow/runtime code, APIs, configuration vocabulary, fixture structure, test workloads, or implementation organization. Do not present Lazily as FoundationDB-compatible. |
| Property generation and shrinking | Claessen and Hughes, [QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs](https://doi.org/10.1145/351240.351266); official [`Arbitrary` API](https://hackage.haskell.org/package/QuickCheck/docs/Test-QuickCheck-Arbitrary.html); implementation is [BSD-3-Clause](https://github.com/nick8325/quickcheck/blob/master/LICENSE) | State-dependent generators, executable properties, and ordered simplification of failing inputs | Design Lazily generators and causal shrinkers independently. Do not copy combinator APIs, implementation code, examples, names, or fixtures. |
| Go fuzzing | Official [Go fuzzing documentation](https://go.dev/doc/security/fuzz/) and [tutorial](https://go.dev/doc/tutorial/fuzz); Go code/toolchain is [BSD-3-Clause](https://github.com/golang/go/blob/master/LICENSE) and site documentation terms are described by [go.dev copyright](https://go.dev/copyright) | `testing.F`, seed corpora, coverage-guided mutation, minimized failures, deterministic targets, and retained regression inputs | Use the standard toolchain API, but author all seeds, encodings, assertions, and examples independently. Do not copy documentation examples into the Lazily corpus. |
| Linearizability checking | Herlihy and Wing, [Linearizability: A Correctness Condition for Concurrent Objects](https://doi.org/10.1145/78969.78972) and the [author-hosted paper](https://www.cs.cmu.edu/~wing/publications/HerlihyWing90.pdf) | Invocation/response histories, legal sequential explanations, and preservation of real-time precedence | The paper is citation-only. Do not copy prose, figures, pseudocode, notation-heavy examples, or histories. Apply the criterion only to APIs that claim linearizability. |

The register records the ideas used and enough provenance to review them. It does
not import the sources' code licenses into independently authored Lazily code.
Any later code, data, or fixture import requires a separate row with its exact
version or commit, license, notice obligations, intended use, and approval.

## Dependency license inventory

### `lazily-go`

- Repository license: MIT, declared by `src/lazily-go/LICENSE`.
- Module: `github.com/lazily-hub/lazily-go`, Go 1.24 floor.
- `go.mod` has no `require` directives; `go list -m all` reports only the Lazily
  module.
- Planned simulation dependency posture: Go standard library and built-in
  `testing` fuzzing only. The Go toolchain and standard library are BSD-3-Clause.
- FoundationDB and QuickCheck are reference-only. They are not linked, vendored,
  translated, or runtime/test dependencies.

### `lazily-spec` development tooling

`lazily-spec` is a private/dev-only Python package for schema and corpus checks.
Its direct development dependencies are `jsonschema`, `referencing`, and
`pytest`; `uv.lock` resolves the following complete package set as of this gate:

| Package | Locked version | SPDX/license | Use |
| --- | ---: | --- | --- |
| attrs | 26.1.0 | MIT | `jsonschema`/`referencing` transitive dependency |
| colorama | 0.4.6 | [BSD-3-Clause](https://github.com/tartley/colorama/blob/master/LICENSE.txt) | Windows-only `pytest` transitive dependency |
| iniconfig | 2.3.0 | MIT | `pytest` transitive dependency |
| jsonschema | 4.26.0 | MIT | direct development schema validation |
| jsonschema-specifications | 2025.9.1 | MIT | `jsonschema` transitive dependency |
| packaging | 26.2 | Apache-2.0 OR BSD-2-Clause | `pytest` transitive dependency |
| pluggy | 1.6.0 | MIT | `pytest` transitive dependency |
| Pygments | 2.20.0 | BSD-2-Clause | `pytest` transitive dependency |
| pytest | 9.1.1 | MIT | direct development test runner |
| referencing | 0.37.0 | MIT | direct development JSON reference handling |
| rpds-py | 2026.6.3 | MIT | `jsonschema`/`referencing` transitive dependency |
| typing-extensions | 4.15.0 | PSF-2.0 | Python-version-conditional transitive dependency |

Versions and license expressions were checked against the versioned PyPI JSON
metadata (`https://pypi.org/pypi/<name>/<version>/json`); Colorama's incomplete
PyPI license field was resolved from its upstream license file. The Lean toolchain
is pinned to Lean 4 v4.30.0 and `lake-manifest.json` declares no Lean packages.
The existing fixture-generator scripts use only Python or Node standard-library
imports.

Before adding a dependency, update this table with its exact version/commit,
SPDX expression, source, notice/attribution requirements, purpose, and approval.
The future public repository should generate a machine-readable third-party
inventory from its lock files rather than relying only on this private plan.

## Fixture provenance and license inventory

### Current corpus

- `src/lazily-spec/conformance/` contains 156 tracked JSON fixtures. Repository
  history attributes the corpus and its local generators to Brian Takita, and
  the audit found no external-source URLs in the corpus or generator scripts.
  That is evidence of first-party authorship, not a substitute for a license.
- Only three fixtures currently declare a top-level `generator` field:
  `codec/blob_backend_discriminator.json`, `codec/nodeid_exact_range.json`, and
  `codec/nodekey_null_leniency.json`. Seven generated message-passing fixtures
  do not declare their generator. No fixture declares a license, copyright,
  origin, or external provenance field.
- `src/lazily-go/test/conformance/` contains nine vendored JSON fixtures, all
  byte-identical to their canonical `lazily-spec` counterparts at the time of
  this audit. The drift test and sync script record their operational origin,
  but there is no per-copy license or notice trail.
- No simulation fixture or external simulation corpus has been imported.

### Blocking license gap

`src/lazily-spec` has no tracked `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`
file, and its `pyproject.toml` has no project-license metadata. GitHub's repository
license endpoint also reports no license. Public readability is not a reuse
license. Therefore:

- do not publish new simulation specs or fixtures through `lazily-spec`;
- do not copy canonical fixtures into another binding on the assumption that
  the destination repository's license cures the source ambiguity;
- do not use the existing corpus as generated-action or fuzz seeds; and
- resolve `#lzsimlicense` before `#lzsimspec` begins repository implementation.

The authorized follow-up should choose and add the intended repository license,
add matching project metadata, preserve or add required notices, record the
provenance/license chain for all 156 canonical fixtures and nine vendored Go
copies, and define mandatory `generator` plus `origin` metadata for new generated
simulation fixtures.

## Independent-authorship rules

1. Derive only abstract requirements from the public references. Record the
   source, access date, concept used, and reviewer before implementation.
2. Design Lazily names, types, schemas, scheduling rules, trace encoding, and
   example domains from Lazily's existing `ManualClock`, `StateMachine`, and
   `ReplayHarness` semantics. Do not perform source-to-source ports.
3. Use synthetic, generic domains and independently authored data. A simulator
   must call the same pure reducers or decisions as production through narrow
   generic ports; it must not recreate consumer behavior in a test-only model.
4. Do not copy third-party code, tests, configuration, fixtures, corpus bytes,
   prose, diagrams, example histories, or API shape. Similar behavior is not
   permission to reuse expression.
5. Every generated fixture records its generator path/version, seed, schema
   version, and origin. Every imported artifact additionally records its exact
   upstream version/commit, license, preserved notices, and reviewer approval.
6. Keep a reviewable commit trail. If provenance is uncertain, stop and replace
   the material with a clean, independently authored equivalent or obtain an
   explicit license before continuing.

## Excluded material

Do not place any of the following in Lazily simulation code, plans intended for
publication, traces, fixtures, examples, commits, or history:

- consumer, customer, or employer names and identifiers;
- workflows, business logic, policies, rules, schemas, subjects, or event names;
- production topology, configuration, credentials, accounts, or data;
- incidents, postmortems, tickets, logs, traces, traffic captures, recordings,
  transcripts, benchmarks, or performance results;
- internal extensions, unpublished designs, code, tests, or fixtures; or
- copied API shape, naming, examples, or scenario structure.

The existing agent-doc/editor/plugin/tmux/project-controller fixtures in
`lazily-spec` are not eligible as consumer-simulation examples. New examples use
neutral synthetic actors and values with no semantic mapping to a consumer.

## Prior-work and employer boundary

Before any consumer- or employer-related implementation or publication, obtain a
written record that:

- identifies the existing Lazily repositories and releases as prior work;
- reserves generic simulation APIs, traces, conformance fixtures, generators,
  shrinkers, and backend-neutral specifications to the existing Lazily project;
- assigns product-specific workflows, private schemas, deployment configuration,
  policies, and confidential business logic separately;
- states whether generic improvements may be contributed upstream;
- states the policy for employer time, equipment, credentials, and accounts; and
- routes patentable or genuinely novel implementation through the agreed
  disclosure process.

Record only the determination, date, and private location of the governing
document. Do not copy confidential agreement text into Lazily. If no consumer or
employer relationship applies, the owner must explicitly record “not applicable”
with a date before the consumer-adapter phase. Signed agreements and applicable
law control; this project gate is not legal advice.

## Phase-admission checklist

- [x] FoundationDB, QuickCheck, Go fuzzing, and Herlihy-Wing primary/official
  sources are recorded and classified as reference-only.
- [x] Current Go and spec-tooling dependencies are inventoried with versions and
  licenses.
- [x] The existing canonical and vendored fixture sets are counted and their
  current provenance metadata is documented.
- [x] Independent-authorship and excluded-material rules are recorded.
- [x] The prior-work/employer determination required before consumer-related work
  is defined.
- [ ] An authorized owner has chosen and recorded the `lazily-spec` repository
  license and fixture reuse terms (`#lzsimlicense`).
- [ ] The repository metadata, third-party inventory, and fixture provenance
  chain match that decision (`#lzsimlicense`).
- [ ] Before each implementation phase, its reviewer and review date are added
  here and every applicable open condition is closed.

## Verification evidence

The gate was built from repository manifests, lock files, license files, fixture
counts, generator metadata, git authorship history, official repository license
endpoints, and the primary/official sources linked above. It intentionally adds
no runtime dependency and imports no third-party code or fixture bytes.
