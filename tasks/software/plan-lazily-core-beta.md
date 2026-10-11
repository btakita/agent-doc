# Plan: Lazily Core Beta profile and capability modules

Status: planned; candidate-profile work may proceed, but Beta promotion remains
gated by `#lzbetascope`, release publication readiness, and a reproducible cohort.

## Goal

Define **Lazily Core** as the small compatibility-managed product profile while
keeping one Lazily brand, one semantic model, and one primary package per
language. Treat collections, statecharts, synchronization, durable/distributed
features, transports, codecs, FFI, and adapters as independently maturing
modules or capabilities.

The Beta promise covers only the profile's declared symbols, execution flavors,
fixtures, support ranges, and package entry points. A broad umbrella package does
not make every export Beta.

## Non-goals

- Do not fork the repository family or renumber every binding in lockstep.
- Do not generate or replace the hand-written reactive kernel.
- Do not split registry artifacts merely for symmetry.
- Do not move existing public types when an allow-list can define Core in place.
- Do not promote GDScript or any other binding without the same evidence required
  from the rest of the cohort.

## Product and dependency rules

1. Core contains `Context`, `Source`, `Computed`, `Effect`, batching,
   scopes/disposal, typed contexts, and explicitly supported execution flavors.
2. A module may depend on Core; Core must not depend on a module.
3. Modules may extend behavior but may not redefine Core semantics.
4. Existing umbrella/root imports remain compatible. Add a focused Core facade
   only where it is idiomatic; Kotlin, Go, and C# should keep their existing
   canonical locations rather than introduce duplicate type identities.
5. Split an artifact only for a real external-dependency, security, platform, or
   independent-release boundary. React and `Lazily.R3` remain valid examples.
6. `core_api_generation` is independent of the wire-protocol version. An
   independently shipped module declares and tests its supported Core generation
   range.
7. Release evidence uses exact tags, commits, artifact digests, and spec revisions,
   never floating sibling `main` branches.

## Phase 1: canonical candidate profile

Create a machine-readable profile in `lazily-spec`, plus a schema, validator, and
generated human-readable view. The registry should carry:

- profile id and status (`candidate`, then `beta` only in Phase 7);
- `core_api_generation`;
- abstract Core symbols and per-binding public symbol/import mappings;
- maturity and supported execution flavors per binding;
- required fixture/scenario ids and proof classes for fixtureless claims;
- minimum/current toolchain ranges;
- introduced, deprecated-since, replacement, and earliest-removal metadata;
- module-to-Core compatibility ranges.

Likely paths:

- `src/lazily-spec/profiles/core-v1.json`
- `src/lazily-spec/schemas/core-profile.schema.json`
- `src/lazily-spec/scripts/check-core-profile.mjs`
- `src/lazily-spec/docs/core-beta.md`
- `src/lazily-spec/Makefile`

Acceptance:

- schema and semantic validation fail closed on unknown/missing mappings;
- generated views reproduce byte-for-byte;
- every candidate binding has explicit symbols, imports, fixtures, support range,
  and proof requirements;
- dependency checks reject Core-to-module edges, unregistered module-to-module
  edges, adapter back-edges, and cycles; a Core-only build stays green with every
  optional module disabled;
- capability-only symbols cannot enter Core through the existing broad coverage
  table or umbrella exports.

## Phase 2: truthful claims and closed fixture set

Make qualification consume what runners actually execute. Standardize a small,
machine-readable exception/skip declaration under each binding's `scripts/`
surface so `lazily-spec` sparse verification can compare declared exceptions with
observed runner skips.

Correct the known merge-algebra overclaims for Python, Kotlin, and JavaScript;
require executable proof for fixtureless shipped rows such as C-ABI; and close any
missing Core scenarios for batching, disposal/scopes, typed contexts, and declared
execution flavors. Pin the Core fixture ids and corpus digest in the profile.

Acceptance:

- an unreported runtime skip, stale declared skip, uncovered Core fixture, unbound
  assertion block, or unsupported fixtureless claim makes CI red;
- Kotlin cannot claim a native C-ABI without building and invoking a native export;
- each Core assertion has consumption and discriminability evidence;
- every candidate binding replays its complete declared Core closure with zero
  unexplained skips.

## Phase 3: additive idiomatic Core facades

Fan out by binding after Phase 1 stabilizes. Add focused Core exposure only where
it helps consumers:

- Rust/Python/JavaScript/Dart/Zig: additive Core modules or subpaths while retaining
  existing root re-exports;
- C++: freeze `include/lazily/core.hpp` and add install/export configuration;
- Kotlin/Go/C#: keep existing canonical public locations and define Core through
  the registry allow-list and snapshots;
- React and `Lazily.R3`: remain separate adapters with declared Core ranges.

Acceptance:

- old and new imports resolve to the same public identities where the language
  permits;
- default installation gains no optional module dependency;
- package-content checks prove the focused facade and required metadata ship;
- no artifact split or breaking namespace move is introduced.

## Phase 4: Core API compatibility and migration gates

Fan out by binding. Derive a committed normalized Core API snapshot from the real
public package surface, then pair it with the strongest practical native checker
(`cargo-semver-checks`/public-api, API Extractor, Griffe, `apidiff`, Kotlin binary
compatibility validation, Roslyn PublicApiAnalyzers, or compile/header manifests).

Acceptance:

- removing or incompatibly changing a released Core symbol fails CI;
- additive updates require an intentional snapshot/profile change;
- every gate has a negative self-test proving it turns red;
- every deprecation records introduction, replacement, compatibility window, and
  earliest removal generation;
- independently shipped modules test their oldest-supported and current Core
  generations.

## Phase 5: artifact consumers and support contract

For every candidate binding, build the distributable artifact and consume it from
a clean temporary project without checkout-relative files or development
dependencies. Exercise source -> computed -> effect, batch coalescing, scoped
disposal, and typed context through only the declared Core entry point.

Publish one generated support/deprecation policy that names candidate bindings,
execution flavors, minimum/current toolchains, the pre-1.0 compatibility promise,
the deprecation window (at least two compatible cohorts and 90 days), and explicit
Preview disclaimers for non-Core exports. Correct stale quickstarts and schema/wire
claims as part of this phase.

Acceptance:

- packaged Rust, npm, Python, Go, Kotlin, C++, C#, Dart, and Zig consumers pass on
  each binding's declared minimum and current toolchains;
- no consumer resolves source-tree files accidentally;
- examples execute against installed artifacts;
- unsupported bindings remain explicitly Preview rather than receiving vacuous
  Core credit.

## Phase 6: reproducible cohort and publication readiness

Define an immutable cohort/BOM schema and candidate manifest containing exact
package coordinates, versions, tags, peeled commits, artifact digests/provenance,
spec/conformance revision, `core_api_generation`, wire version, toolchain ranges,
Core imports, and module compatibility ranges.

Pin the exact spec revision in release jobs, require tag/manifest equality, and add
pre-publish plus post-publish installed-consumer checks. Repair public distribution
gaps without forcing a new registry artifact where signed source archives are an
honest supported contract. This phase depends on `#lzreleasepins`; Rust publication
also depends on `#lzrustpublish`.

Acceptance:

- the verifier rejects floating refs, tag/version mismatches, missing/yanked
  artifacts, wrong digests, wrong spec revisions, and incompatible module ranges;
- one report names every binding's profile, API, fixture, package, and published
  consumer status;
- a clean environment can reproduce the cohort from pinned evidence;
- a release cannot publish an artifact candidate that did not pass its gates.

## Phase 7: qualify and promote the first Beta cohort

Run the complete profile, claim, conformance, API, artifact, support, and cohort
gates against exact candidate artifacts. Require two release-candidate cohorts or
a four-week Core freeze without an unplanned break. With `#lzbetascope`,
`#lzreleasepins`, and all publication blockers green, change the profile from
`candidate` to `beta`, publish the immutable cohort, migration guide, and support
matrix, then announce:

> Lazily Core is Beta; Lazily Modules graduate independently.

Promotion is per binding. The announcement must link the exact supported-binding
list and must not imply that every Lazily package export or capability is Beta.

## Ordered delivery and fan-out

Phases 1 and 2 are serial because every later gate consumes their registry and
truth model. Phase 3 fans out by binding. Phase 4 can fan out behind each completed
facade, while Phase 5 can proceed in parallel where the public Core entry point is
already stable. Integrate each binding serially into its owning repository and
re-run the aggregate profile gate after every batch. Phase 6 begins only from a
settled set of exact binding commits; Phase 7 is evidence-only promotion plus
publication.

## Risks and mitigations

- **Two meanings of “Core”:** existing collection-family “Core surface” language
  is narrower than the product profile. Name the product artifact `Core Beta
  profile` and make the registry authoritative.
- **Umbrella-export ambiguity:** generated docs label only allow-listed symbols
  Beta and label remaining exports Preview/Experimental.
- **Snapshot theater:** every compatibility adapter includes a deliberately failing
  self-test and derives from built/package-visible APIs.
- **Fixture theater:** observed skips and fixture consumption must match committed
  declarations exactly; Core cannot use `untested` discriminability exemptions.
- **Registry races:** use a candidate manifest followed by immutable finalization
  only after registry propagation and installed-consumer probes pass.
- **Package explosion:** preserve umbrella packages and existing type identities;
  extract only across a proven independent boundary.
- **Dirty umbrella checkout:** use peeled tags, registry artifacts, and exact
  gitlinks for cohort truth; never qualify from incidental local submodule state.
