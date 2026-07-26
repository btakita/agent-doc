# Haiven SDK — Superrepo Design

**Status:** Proposed

**Date:** 2026-07-26

**Decision owner:** Haiven engineering

## Decision summary

Create a thin `haiven-sdk-superrepo` as the integration control plane for a
family of dedicated repositories. Each shipping SDK, the shared contracts,
the conformance system, and the agent harness keep independent history,
permissions, CI, and releases. The superrepo supplies one reproducible
workspace, a compatibility lock, cross-repository verification, shared
policies, and a single place to see the state of the whole SDK program.

Use a versioned component manifest plus a generated lockfile instead of making
Git submodules the primary dependency mechanism:

- `components.yaml` describes repository identity, lifecycle, ownership,
  dependency ranges, and standard commands.
- `workspace.lock.json` pins exact commits and released artifact digests for a
  tested integration state.
- A small workspace CLI clones or updates the dedicated repositories into a
  predictable local layout and runs their declared commands.
- Published SDKs depend on released contract/conformance artifacts, never on a
  sibling checkout or the current superrepo branch.

This gives Haiven monorepo-like discoverability and integration testing without
forcing every SDK into one release cadence or access boundary.

## Goals

1. Give a developer or coding agent one command to obtain a compatible,
   reproducible SDK workspace.
2. Preserve dedicated ownership, issue tracking, security boundaries, and
   releases for every SDK.
3. Make protocol and behavioral compatibility explicit and machine-checkable.
4. Allow one SDK to release without rebuilding or versioning every other SDK.
5. Let shared changes fan out through reviewable repository-local pull
   requests.
6. Make the agent harness a first-class, independently versioned product rather
   than scripts hidden in the Unity repository.

## Non-goals

- The superrepo is not a second home for SDK source code.
- It does not require all SDKs to use the same language, build tool, or version.
- It does not grant superrepo automation permission to publish every package.
- It does not promise that every planned engine repository exists or is
  supported before a pilot and acceptance gate.
- It does not replace each SDK's native package manager or engine-specific
  installation workflow.

## Repository topology

```text
haiven-sdk-superrepo                  integration control plane
├── haiven-sdk-contracts             protocol + behavioral contracts
├── haiven-sdk-conformance           fake server + shared scenarios
├── haiven-sdk-agent-harness         MCP server + CLI + agent evals
├── haiven-sdk-unity                 Unity SDK
├── haiven-sdk-unreal                Unreal SDK
├── haiven-sdk-godot                 Godot SDK
├── haiven-sdk-android               Android SDK
├── haiven-sdk-ios                   iOS SDK
├── haiven-sdk-gamemaker             GameMaker SDK
├── haiven-sdk-rpg-maker             RPG Maker SDK
├── haiven-sdk-renpy                 Ren'Py SDK
├── haiven-sdk-cocos-creator         Cocos Creator SDK
├── haiven-sdk-construct             Construct 3 SDK
└── haiven-sdk-bevy                  Bevy SDK, only when pilot-gated
```

The names are logical until Haiven confirms its organization and naming
conventions. A repository should be created only when its lane is approved;
planned components can remain registry entries with `lifecycle: planned`.

### Control-plane repository

`haiven-sdk-superrepo` owns:

- `components.yaml` and `workspace.lock.json`;
- cross-repository architecture decisions and compatibility policy;
- the workspace bootstrap/verification entrypoint;
- reusable repository templates and CI workflow fragments;
- integration-test orchestration and the published compatibility dashboard;
- change-train records for work spanning multiple repositories.

It must not contain copied SDK implementations, package publication
credentials, or a second copy of protocol schemas.

### Shared foundation repositories

| Repository | Owns | Does not own |
|---|---|---|
| `haiven-sdk-contracts` | Versioned wire schemas, behavioral contract, error taxonomy, compatibility/deprecation rules, generated-model inputs | Engine lifecycle code or package publishing |
| `haiven-sdk-conformance` | Deterministic fake server, golden fixtures, scenario catalog, runner protocol, compatibility evidence schema | An engine's public API or native test runner |
| `haiven-sdk-agent-harness` | Read-only MCP documentation/guidance server, verification CLI, workspace adapters, request evals, evidence bundle format | SDK runtime behavior, merge approval, release credentials, production access |

Contracts and conformance are separate because a schema release should not
implicitly version the test infrastructure, and the conformance runner may
evolve without changing the public protocol.

### Dedicated SDK repositories

Every engine or runtime adapter gets its own repository and native release
history. The initial lifecycle registry is:

| Repository | Initial lifecycle | Promotion trigger |
|---|---|---|
| `haiven-sdk-unity` | Active in Milestone 1 | Approved Milestone 0 contract |
| `haiven-sdk-unreal` | Planned, Wave A | Pilot demand, editor/tooling review, and estimate |
| `haiven-sdk-godot` | Planned, Wave A | Pilot demand and extension-model choice |
| `haiven-sdk-android` | Planned, Wave B | Mobile product demand and lifecycle/security design |
| `haiven-sdk-ios` | Planned, Wave B | Mobile product demand and lifecycle/security design |
| `haiven-sdk-gamemaker` | Planned, Wave C | Named pilot and extension feasibility spike |
| `haiven-sdk-rpg-maker` | Planned, Wave C | Named pilot, target-generation choice, and plugin/networking spike |
| `haiven-sdk-renpy` | Planned, Wave C | Named pilot, target-platform choice, and save/rollback/networking spike |
| `haiven-sdk-cocos-creator` | Planned, Wave C | Named pilot and supported-version choice |
| `haiven-sdk-construct` | Planned, Wave C | Named pilot and addon/distribution feasibility spike |
| `haiven-sdk-bevy` | Incubator | A specific Rust/Bevy pilot justifies specialist support |

If two adapters later need a real shared language core, that core becomes
another versioned repository/package with explicit consumers. Source is not
silently copied or linked across SDK repositories.

## Materialized workspace

The workspace CLI materializes repositories without nesting their Git histories
inside the superrepo index:

```text
haiven-sdk-superrepo/
├── components.yaml
├── workspace.lock.json
├── adr/
├── policies/
├── templates/
├── tools/
└── .workspace/                       ignored
    ├── contracts/
    ├── conformance/
    ├── agent-harness/
    └── sdks/
        ├── unity/
        └── ...
```

Recommended commands:

```text
haiven-workspace sync                 checkout the committed lock
haiven-workspace status               show drift and compatibility
haiven-workspace verify --changed     run affected component checks
haiven-workspace verify --all         run the locked integration matrix
haiven-workspace propose <component>  prepare a reviewed lock update
haiven-workspace evidence             emit a redacted result bundle
```

The CLI may initially be a small script in the superrepo. Its stable
verification and evidence surface belongs in `haiven-sdk-agent-harness` once
that repository is bootstrapped. The manifest, not a hard-coded repository
list, remains the source of truth.

## Component manifest contract

Each entry declares enough information for humans, CI, and the agent harness to
operate without guessing:

```yaml
schema_version: 1
components:
  unity:
    repository: haiven-sdk-unity
    path: .workspace/sdks/unity
    kind: sdk
    lifecycle: active
    owners: [sdk-platform, unity]
    contract: ">=0.1.0 <0.2.0"
    conformance: ">=0.1.0 <0.2.0"
    commands:
      bootstrap: ./eng/haiven bootstrap
      verify: ./eng/haiven verify
      conformance: ./eng/haiven conformance
      package: ./eng/haiven package
      evidence: ./eng/haiven evidence
```

The real repository URL, default branch, visibility, package registry, and
release channel are added during Milestone 0. The committed lock records the
resolved commit, artifact version, artifact digest, and lock-generation tool
version.

Every active component repository contains:

```text
.haiven/component.yaml     repository-local metadata and supported commands
src/                       native implementation
tests/                     unit/integration tests
samples/                   minimal releasable sample
docs/                      version-coupled documentation
eng/haiven                 stable command facade for local and CI use
SECURITY.md
CODEOWNERS
CHANGELOG.md
```

The command facade is uniform; its implementation remains native. Unity may
invoke Unity batch mode, Unreal may invoke AutomationTool, and iOS may invoke
Xcode without the superrepo pretending those systems are interchangeable.

## Dependency and versioning rules

1. `haiven-sdk-contracts` uses Semantic Versioning once the beta contract is
   approved. Breaking wire or behavioral changes require a new major line.
2. Each SDK versions and releases independently and declares supported contract,
   conformance, engine, runtime, and server ranges.
3. The agent harness versions independently and declares which manifest,
   evidence, and documentation schemas it understands.
4. The conformance repository publishes immutable fixtures/runners. An SDK pins
   a released conformance version in its own CI.
5. The superrepo lock is an integration bill of materials, not the SDK package
   version. Advancing it certifies that the pinned set passed the matrix.
6. A dedicated repository may consume another only through a released artifact
   or documented service contract. Relative filesystem imports are forbidden.
7. Generated clients/models record the contract version and generator version;
   generated output is reviewed in the consuming SDK repository.

The compatibility dashboard is derived from signed CI evidence rather than
manually maintained support claims.

## Cross-repository change protocol

A change that touches the shared contract follows an explicit train:

1. Open a contract proposal with a change ID, compatibility classification,
   fixtures, migration note, and affected-component list.
2. Publish an immutable release candidate for contracts and, when needed,
   conformance.
3. Open repository-local SDK pull requests against that release candidate.
   Each PR runs native tests, shared conformance, packaging, sample, and
   redaction checks.
4. Open an agent-harness PR for documentation, guidance, CLI, and eval changes.
5. Let superrepo CI test a proposed lock containing the exact candidate commits
   and artifact digests.
6. Obtain the required product, protocol, SDK-owner, safety/security, and
   developer-experience approvals.
7. Release contracts/conformance first, then release ready SDKs independently.
8. Merge the final lock and publish the resulting compatibility evidence.

An additive contract change does not wait for every planned SDK. A breaking
change cannot become the default contract line until every supported SDK has a
migration path or an explicit end-of-support decision.

Rollback means restoring the last green superrepo lock and issuing a new
package version where necessary. Published tags and artifacts are never
rewritten.

## CI model

### Repository-local CI

Every active SDK repository proves:

- native compile, lint, unit, and lifecycle behavior;
- the pinned conformance corpus actually executed;
- clean package installation and a runnable minimal sample;
- supported engine/runtime build matrix;
- documentation links and compatibility metadata;
- secret and contractor-path redaction;
- package provenance and digest generation.

The shared repositories and agent harness have equivalent component-specific
checks. The harness additionally proves corpus freshness, citations,
read-only/default-deny behavior, deterministic CLI output, and scored request
evals.

### Superrepo integration CI

The control plane:

- validates manifest/lock schemas and dependency ranges;
- detects uncommitted component drift;
- computes affected components from the dependency graph;
- fans out locked verification without granting publish credentials;
- aggregates evidence into the compatibility dashboard;
- blocks a lock update when a required component fails or did not execute;
- supports a scheduled full matrix in addition to change-scoped checks.

Integration CI consumes short-lived, read-only checkout credentials.
Repository-local release workflows retain their own protected publishing
environments and human approval gates.

## Agent harness placement and boundaries

`haiven-sdk-agent-harness` is a dedicated peer of the SDK repositories. This
keeps its release cadence, threat model, and model/provider integrations out of
runtime packages.

The harness reads:

- the superrepo manifest and tested lock;
- released contracts, fixtures, ADRs, SDK docs, examples, and compatibility
  evidence;
- repository-local `.haiven/component.yaml` files.

It may:

- answer version-scoped questions with citations;
- recommend an opinionated paved road while naming alternatives and unknowns;
- bootstrap a locked workspace;
- invoke declared verification commands;
- assemble redacted evidence;
- evaluate bounded agent requests against pinned fixtures.

It may not:

- mutate authoritative schemas through MCP;
- merge pull requests or approve its own changes;
- publish packages, deploy services, or access production credentials;
- mark support green without proof that the expected tests ran;
- silently substitute a different component revision than the committed lock.

An agent-generated cross-repository change uses the same change ID and PR train
as human-authored work. The harness coordinates evidence; it does not become a
privileged release orchestrator.

## Ownership and access

- Every repository has named product and technical owners; safety-sensitive
  contract areas add a safety/security owner.
- `CODEOWNERS` and branch protection live in each dedicated repository.
- The superrepo can dispatch read-only verification but has no transitive right
  to write, merge, or publish in child repositories.
- Publishing uses repository-specific protected environments and Haiven-owned
  credentials.
- External contributors can receive access to only the SDKs and fixtures needed
  for their assignment.
- Public/private boundaries are recorded per component so the agent harness
  never indexes inaccessible material into a broader corpus.

## Bootstrap plan

### Milestone 0

1. Confirm names, visibility, owners, package registries, and branch policy.
2. Create the superrepo, contracts, conformance, Unity, and agent-harness
   repositories from reviewed templates.
3. Define manifest, lock, component metadata, evidence, and compatibility
   schemas.
4. Move existing material with history where practical; do not duplicate an
   authoritative schema across repositories.
5. Implement `sync`, `status`, and manifest/lock validation.
6. Prove a synthetic contract change through the full multi-repository train.

### Milestone 1

1. Build Unity and the narrow agent-harness prototype in their dedicated
   repositories.
2. Add repository-local CI and superrepo change-scoped integration CI.
3. Pin the first green workspace lock and publish its evidence.
4. Run a handoff drill in which a Haiven engineer clones, verifies, changes,
   packages, and rolls back the workspace.

### Later waves

Create a dedicated SDK repository only after its roadmap gate passes. Generate
it from the current template, register it as `incubator`, require conformance
before `beta`, and add it to the support dashboard only after its release gate
passes.

## Lifecycle gates

| State | Meaning | Required to advance |
|---|---|---|
| `planned` | Roadmap entry; repository may not exist | Named pilot or approved strategic investment |
| `incubator` | Repository exists; API/support are provisional | Owners, CI, threat review, package spike |
| `alpha` | Contract vertical slice works | Required conformance and sample evidence |
| `beta` | Pilot-ready with documented support range | Packaging, lifecycle, migration, and handoff gates |
| `stable` | Supported public release | SLO/ownership decision, release drill, deprecation policy |
| `maintenance` | Security/compatibility fixes only | Published successor or end-of-support plan |
| `archived` | Unsupported and read-only | Final migration notice and compatibility record |

## Acceptance criteria

The design is implemented when:

- a clean machine can materialize the committed workspace from one command;
- each active component remains a standalone clone with standalone CI;
- the lock reproduces exact commits and artifact digests;
- a contract change can fan out to Unity and the agent harness without direct
  pushes or shared publish credentials;
- repository-local and superrepo CI both prove the intended conformance corpus
  ran;
- one SDK can release independently while compatibility remains truthful;
- the last green integration state can be restored without rewriting history;
- the agent harness can cite and verify the locked workspace but cannot merge,
  publish, deploy, or access production secrets.

## Decisions to confirm in Milestone 0

1. Repository organization, naming, and public/private boundaries.
2. Manifest implementation language and whether the workspace CLI begins in the
   superrepo or immediately in the agent-harness repository.
3. Package registries and signing/provenance requirements per SDK.
4. Contract artifact formats and code-generation policy.
5. Which adapters are considered supported versus incubating in each lock.
6. Required reviewers for protocol, safety, security, and release changes.
7. Compatibility/deprecation windows and minimum supported engine versions.
