# Haiven SDK — Implementation Plan

Prepared for technical review by Haiven and Brian Takita

Date: July 26, 2026
Status: Shareable planning draft; details are finalized during proposal Milestone 0

## 1. Product boundary

The SDK is the client-side contract between a game and Haiven's REST/WebSocket
services. It should make the safe path the easy path:

- authenticate a player and manage session renewal;
- connect once and multiplex chat, presence, notifications, community, and
  moderation/safety events;
- translate unreliable network behavior into explicit, testable client states;
- expose idiomatic APIs in each engine without changing protocol meaning;
- give developers runnable examples, diagnostics, and recovery guidance;
- preserve privacy boundaries and avoid leaking secrets or production data.

The SDK does not own Haiven's moderation policy, game UI, console certification,
or a studio's account-linking/business rules. It provides dependable primitives
and integration points for those systems.

## 2. Architecture

### 2.1 Layers

1. **Protocol contract**
   - versioned message envelope;
   - service/action routing;
   - request, response, event, and error schemas;
   - authentication and capability negotiation;
   - compatibility and deprecation rules.
2. **Behavioral contract**
   - connection/session state machine;
   - heartbeat, timeout, retry, backoff, and resume rules;
   - ordering, idempotency, and duplicate handling by message class;
   - bounded queues, cancellation, and slow-consumer behavior.
3. **Conformance harness**
   - deterministic fake server and scripted failure cases;
   - golden protocol fixtures;
   - behavior tests reusable by every language/engine adapter;
   - compatibility matrix across server and SDK versions.
4. **Engine adapters**
   - idiomatic public API;
   - engine lifecycle and main-thread integration;
   - packaging, dependency, and build-system integration;
   - platform-specific secure storage and network configuration hooks.
5. **Developer experience**
- quick start;
- runnable reference scene/app;
- API documentation and troubleshooting;
- structured diagnostics with secrets redacted.
6. **Agent harness assistance**
- read-only, version-aware MCP resources for docs, schemas, ADRs, fixtures,
examples, compatibility data, and troubleshooting;
- opinionated guidance tools that return a cited paved road, alternatives, and
explicit unresolved decisions;
- a thin CLI that runs the same package, sample, protocol, conformance, and
evidence checks locally and in CI;
- an eval harness that scores representative agent requests, tool use,
grounding, artifacts, verification, and safety boundaries.

### 2.2 Suggested repository shape

```text
sdk/
  protocol/                 # schemas, fixtures, compatibility policy
  conformance/              # fake server + cross-SDK scenarios
  unity/                    # UPM package, tests, sample scene
  unreal/                   # later: C++ module + Blueprint facade
  godot/                    # later: plugin/extension
  bevy/                     # later: Rust crate/plugin
  mobile/
    android/                # later: Kotlin
    ios/                    # later: Swift
samples/
docs/
decisions/                # short ADRs
tools/
  mcp/                    # versioned documentation + guidance server
  cli/                    # deterministic verify/evidence commands
evals/
  agent-requests/         # requests, fixtures, rubrics, expected evidence
```

Haiven may split these into repositories for release/access reasons. The logical
separation and shared conformance assets matter more than the physical layout.

### 2.3 Starting protocol assumptions to validate

The existing Haiven WebSocket design assumes a single persistent connection
multiplexing chat, presence, notifications, and moderation through a versioned
JSON envelope. It also treats reconnects as routine and separates delivery
semantics by message class. Milestone 0 must confirm which of those assumptions
remain authoritative before the SDK freezes an API around them.

## 3. Agentic development operating model

### 3.1 Why Lazily is relevant

Lazily is evidence of a repeatable cross-language delivery process: define a
shared semantic model, build one reference implementation, translate it into
language-native APIs, and prove parity with conformance scenarios. Haiven does
not need to adopt Lazily as an SDK dependency. It can adopt the development
discipline that made a multi-language family tractable.

### 3.2 Human-owned inputs

Before an agent writes implementation code, a human owner approves:

- the product behavior and non-goals;
- protocol/schema version and compatibility rule;
- security, privacy, and child-safety constraints;
- public API and engine/runtime support boundary;
- acceptance tests and evidence required to claim completion;
- the repository, files, and credentials the work may access.

Ambiguous behavior returns to the owner as a decision request. It is not filled
in by model preference.

### 3.3 Bounded agent roles

- **Planner/architect:** converts an approved requirement into a small work
  packet, identifies affected contracts, and names verification.
- **Implementer:** changes only the scoped adapter, test, sample, or docs.
- **Conformance reviewer:** tries to falsify parity with protocol fixtures,
  lifecycle faults, edge cases, and negative tests.
- **Engine reviewer:** checks idioms, packaging, threading/main-loop behavior,
  AOT/native constraints, and sample usability.
- **Release reviewer:** verifies versions, changelog, artifact provenance,
  compatibility matrix, and secret-free packages.

One agent may perform multiple roles on low-risk work, but it may not be the only
evidence for its own completion. Human code review and CI remain merge gates.

### 3.4 Work packet and evidence chain

Every agentic task should record:

1. requirement and acceptance criteria;
2. source protocol/schema and relevant ADRs;
3. allowed write scope and explicit non-goals;
4. implementation and public-API impact;
5. tests/fixtures that must fail before the fix and pass after it;
6. build, package, sample, and conformance commands;
7. changed paths, verification results, remaining risks, and reviewer.

The pull request links that packet to code, test output, review decisions, and
the eventual release. A task is not complete merely because a runner returned
zero; the expected tests and fixtures must be shown to have executed.

### 3.5 Reference-first parallelization

Milestone 1 stays intentionally sequential: protocol contract, conformance
harness, Unity reference behavior, then pilot hardening. After those stabilize,
agents may work on Godot, Unreal, mobile, or other adapters in parallel from the
same fixtures. Cross-adapter failures feed back into the shared behavioral
contract instead of spawning per-language exceptions by default.

### 3.6 Safety and access boundaries

- no production credentials, private player data, or unrestricted production
  access in agent workspaces;
- synthetic identities/events and isolated test environments by default;
- least-privilege repository and CI tokens;
- generated dependency changes require license and supply-chain review;
- no autonomous merge, release, deploy, or console-portal action;
- protected console documentation and SDKs stay inside NDA-compliant,
  access-controlled environments;
- prompts, logs, fixtures, and artifacts follow the same retention/privacy
  rules as source code.

### 3.7 Evaluation metrics

- lead time from approved packet to reviewed change;
- first-pass conformance rate and regression escape rate;
- number of adapter-specific semantic divergences;
- percentage of packets with complete requirement-to-release evidence;
- human review time and rework by category;
- developer time to complete the quick start;
- defects attributable to generated code versus manually authored code.

The process earns wider autonomy only when these measures show faster delivery
without weaker conformance, security, or maintainability.

### 3.8 Agent harness assistance

The harness is a developer-enablement product around the SDK, not part of the
game runtime. It should remain useful from multiple coding-agent clients and
should degrade cleanly to ordinary documentation and CLI commands.

#### MCP documentation and guidance server

Start with a read-only MCP server whose corpus is built from version-controlled
Haiven sources. It should expose:

- versioned SDK and protocol documentation, schemas, ADRs, support matrices,
examples, conformance scenarios, and troubleshooting entries;
- lookup by SDK version, engine, runtime platform, feature, error code, and
protocol capability;
- opinionated guidance that clearly labels **required contract**,
**recommended paved road**, **supported alternative**, and **open decision**;
- citations containing repository path/document identity, version or commit,
and relevant section so a developer can audit every recommendation;
- explicit conflict and stale-content results rather than silently choosing
between incompatible sources.

Initial tools/resources should answer questions and assemble bounded work-packet
context. Write-capable tools, issue creation, code mutation, release actions,
and production access stay out of the first version. Protected console material
must be served only from an access-controlled corpus to authorized users and
must never be copied into public eval fixtures or logs.

#### Verification CLI

Use one non-interactive CLI as the deterministic execution surface behind human
and agent workflows. The exact command names are a Milestone 0 decision; the
initial capabilities should cover:

- environment and dependency diagnostics;
- package structure and clean-install validation;
- protocol-schema and compatibility checks;
- conformance scenarios and sample-project smoke tests;
- documentation link/version checks;
- a machine-readable evidence bundle listing commands, fixture corpus, results,
versions, changed paths, and redacted diagnostics;
- the agent-request eval suite.

Commands should support stable exit codes, JSON output, explicit offline/test
environment selection, and secret redaction. The CLI may verify a release
candidate but may not merge, publish, deploy, or retrieve production credentials.

#### Agent-request evaluation suite

The suite treats the agent request and its evidence as the system under test.
Use versioned fixtures for good paths, ambiguous requirements, stale or
conflicting documentation, negative cases, and disallowed actions. Initial
scenarios should include:

- scaffold the smallest supported Unity client from the approved quick start;
- add reconnect/resume handling from the protocol contract;
- implement a chat/presence/moderation vertical slice;
- diagnose an SDK/server compatibility mismatch;
- migrate between two supported SDK versions;
- refuse or safely redirect a request for production secrets, autonomous
release, or unavailable console documentation.

Each eval records the request, allowed corpus and tools, expected source
citations, required/forbidden file changes, commands that must run, behavioral
fixtures that must pass, and a structured rubric. Score grounding, task
completion, API correctness, verification evidence, uncertainty handling,
write-scope compliance, and safety separately. A fluent response without the
required artifacts and executed fixtures fails.

Run deterministic checks on every harness change and a broader model/harness
matrix on a schedule or before widening support. Store prompt/tool transcripts
only under the agreed privacy and retention policy, and use synthetic accounts
and events. Track pass rate by scenario and failure category, grounded-citation
rate, unsafe-action refusal, false-confidence rate, cost/latency, and regression
against the last approved harness release.

## 4. Milestone work breakdown

### Milestone 0 — contract and design

#### Discovery

- inventory current REST and WebSocket endpoints, auth, schemas, and service
  ownership;
- identify server behavior that is specified, implemented, or still provisional;
- choose the first Unity versions and target build platforms;
- interview one or two representative indie developers if available;
- define the smallest valuable vertical slice;
- identify privacy, child-safety, audit, and data-retention constraints that
  affect client behavior;
- confirm Haiven's console developer-program status and pilot-studio access.

#### Design outputs

- protocol and public API draft;
- state diagrams for auth and connection lifecycle;
- error taxonomy and retry table;
- event delivery/ordering table by service;
- schema/version strategy;
- package/release/version-support policy;
- agentic work-packet template, role boundaries, review gates, and evidence
policy;
- harness content map and source-of-truth rules;
- read-only MCP resource/tool contract, citation schema, and access boundaries;
- verification CLI command/exit-code/JSON-output specification;
- initial agent-request fixtures, rubrics, forbidden-behavior cases, and
baseline results against the selected harnesses/models;
- sample-scene storyboard;
- deterministic conformance test catalog;
- risk register and refined estimate.

#### Exit gate

- no unresolved ambiguity in the vertical-slice acceptance tests;
- named technical reviewer and product approver;
- test environment and synthetic credentials available;
- IP/security/access prerequisites confirmed;
- fixed Milestone 1 scope and price approved.

### Milestone 1 — Unity foundation and prototype

#### Package foundation

- create UPM package layout, assemblies, sample, and test assemblies;
- define public API naming and asynchronous model;
- avoid runtime reflection/dynamic code where IL2CPP/AOT compatibility matters;
- provide dependency injection seams for transport, clock, scheduler, and token
  provider;
- add secret-redacting structured logging.

#### Authentication and transport

- token acquisition/refresh interface;
- WebSocket connect/authenticate flow;
- application pause/resume and network reachability hooks;
- heartbeat and timeout handling;
- exponential backoff with jitter and bounded retry policy;
- explicit connection-state events;
- bounded outbound queue and cancellation;
- clean shutdown/logout and token removal.

#### Feature vertical slice

- chat send/receive and agreed channel/community operations;
- presence updates and subscription behavior;
- notifications;
- moderation/safety event callbacks and user-facing response hooks;
- telemetry hooks for operational metrics, disabled or scoped by policy.

#### Developer experience

- reference scene on a stock Unity starter project, not a custom reference game;
- quick start from clean project to first message;
- annotated example for connection status, chat, presence, and safety feedback;
- API docs, troubleshooting, and common lifecycle recipes;
- package changelog and compatibility declaration.

#### Agent harness prototype

- generate a versioned, auditable documentation corpus from approved sources;
- implement the read-only MCP lookup and opinionated-guidance surface;
- implement the agreed verification/evidence CLI slice;
- connect CLI verification to the same protocol and conformance fixtures used by
the Unity package;
- add representative good-path, ambiguity, stale-source, and unsafe-request
evals for the Unity vertical slice;
- run the suite in CI and publish a redacted, machine-readable score/evidence
report.

#### Verification

- unit tests for state machines and error mapping;
- fake-server protocol tests;
- network-fault scenarios: drop, timeout, duplicate connection, stale token,
  server restart, resume rejected, slow consumer;
- clean package install in CI;
- desktop edit/play-mode tests;
- Android and iOS build validation, with device validation when hardware is
available;
- MCP corpus freshness/citation checks and access-boundary tests;
- CLI exit-code/JSON-schema/idempotence and secret-redaction tests;
- agent-request eval thresholds with proof that the intended request and fixture
corpus executed;
- no console claims until approved access and target hardware are present.

### Milestone 2 — pilot and beta

- onboard one pilot project;
- observe integration without silently doing the studio's work for it;
- record friction by category: docs, API shape, missing capability, server
  behavior, or engine/platform issue;
- fix release-blocking defects;
- establish issue templates and support escalation;
- publish beta package and immutable release notes;
- conduct handoff walkthrough and recovery drill;
- use pilot evidence to select Godot, Unreal, or another next adapter.

## 5. Expansion roadmap

### Wave A — next major indie engine

Choose between Godot and Unreal using named-pipeline evidence:

- number and strategic value of interested studios;
- target release dates;
- required Haiven capabilities;
- engine versions and runtime targets;
- availability of a pilot and technical contact;
- estimated support burden.

Godot emphasizes accessibility and indie reach. Unreal emphasizes higher-value
3D titles and requires a carefully designed Blueprint surface over C++.

### Wave B — mobile

Mobile has two forms:

1. **Engine-export validation:** Unity/Godot/Unreal packages must behave correctly
   on Android and iOS, including pause/resume, backgrounding, connectivity
   changes, secure token storage hooks, AOT, and platform network policy.
2. **Native clients:** Swift/iOS and Kotlin/Android SDKs are separate products
   for native apps or non-engine games. React Native becomes a separate adapter
   only when an actual product needs it.

Each mobile target gets device-level failure testing; a successful desktop build
does not count as mobile acceptance.

### Wave C — broader indie and creator ecosystems

Wave C should balance qualified market reach, integration cost, and reuse of the
shared protocol core. Bevy is not a market-coverage anchor; it belongs behind
engines with broader commercial 2D, mobile, web, and low-code pipelines.

#### Market-balancing candidates

- **GameMaker:** GML-facing package plus native/platform extension shims where
required. Validate Windows/macOS/Linux, HTML5, Android, and iOS first; console
exports stay in the access-gated lane.
- **Cocos Creator:** TypeScript client/component package for its mobile/web
workflow, with a native bridge only where the target runtime requires it.
- **Construct 3:** JavaScript/TypeScript addon exposing Haiven actions,
conditions, expressions, and events, backed by a sample project and web/mobile/
desktop export validation. Require a named pilot before productizing it.

These candidates still compete through Haiven's qualified pipeline rather than
receiving automatic implementation commitments. Milestone 0 should score named
studios, target platforms, launch timing, integration effort, support burden,
and protocol-core reuse.

#### Pilot-driven specialist candidates

- **MonoGame/FNA:** .NET library compatible with current XNA-style frameworks;
target MonoGame rather than discontinued Microsoft XNA.
- **Bevy:** Rust crate plus Bevy plugin/resources/events, backed by the shared
state-machine and conformance model. Proceed only for a named pilot or unusually
strong pipeline evidence.
- **Clickteam Fusion 2.5:** extension only for a named pilot because exporter and
extension constraints vary by runtime.
- **Source SDK:** project-specific C++ integration after license and commercial
distribution review.
- **Defold, Phaser, or GDevelop:** evaluate when a qualified studio appears or
when an existing JavaScript/Lua/native core makes the adapter unusually cheap.

### Console readiness lane

Console work starts only when Haiven or a pilot partner supplies:

- approved Nintendo, PlayStation, or Xbox developer access;
- platform SDK/toolchain and dev hardware;
- permission for the contractor to access protected materials;
- NDA-compliant repository, CI, artifact, and support processes;
- a named game/engine/version and acceptance environment.

The work then validates an existing engine adapter on that console. It is not a
single generic “console SDK” milestone.

## 6. Quality and release policy

- Semantic Versioning for public packages after the first beta contract is
  approved.
- Compatibility table covering server protocol, SDK, engine, and runtime.
- Every public behavior represented in tests or an explicit documented
  exception.
- Golden fixtures reviewed with server changes.
- Minimum supported engine versions revisited on a scheduled cadence.
- Deprecations announced before removal and accompanied by migration notes.
- Reproducible packages built by Haiven CI, signed/published from Haiven-owned
  credentials.
- Release artifacts contain no production secrets or contractor-local paths.

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Server contract changes faster than adapters | Version schemas, keep fixtures in CI, and gate breaking changes on compatibility tests. |
| “Support every platform” expands without a buyer | Require a named pilot and approval gate for each adapter/runtime lane. |
| Unity succeeds on desktop but fails under mobile AOT/lifecycle | Design for IL2CPP/AOT early and include Android/iOS builds in Milestone 1 acceptance. |
| Console work is promised before access exists | Keep console work access-gated and describe it as validation of selected adapters. |
| Safety features become optional integration trivia | Put moderation/safety events in the vertical slice and sample scene. |
| SDK embeds policy that belongs on the server | Expose policy results and safe interaction hooks; keep authoritative decisions server-side. |
| Contractor becomes the only person who can release | Use Haiven repos/CI/credentials, document decisions, and conduct a handoff drill. |
| Side engagement creates IP or schedule conflict | Obtain written primary-engagement clearance, isolate equipment/accounts, and cap weekly hours. |
| Agents generate plausible but behaviorally unproven parity | Require a reference implementation, shared fixtures, independent conformance review, and proof that the intended corpus executed. |
| Agent work exposes protected data or credentials | Use synthetic data, least privilege, isolated environments, redaction, and human approval for every access expansion. |

## 8. Working cadence and decisions

- Monday: written plan, risks, and decision requests.
- Midweek: implementation checkpoint when a decision is blocking.
- Weekly: live demo against acceptance tests.
- Every material design choice: short ADR in the repository.
- Every scope change: written impact on milestone, price, and schedule.
- Milestone close: acceptance report, known limitations, and next-gate options.

## 9. First review meeting agenda

1. Confirm the developer/customer Haiven wants to win first.
2. Confirm the minimum chat, presence, community, and safety workflows.
3. Walk the current REST/WebSocket contract and identify unstable areas.
4. Select Unity versions plus desktop/mobile acceptance targets.
5. Name the pilot project and technical/product approvers.
6. Confirm repository, test environment, credentials, and access boundaries.
7. Approve the agentic work-packet, evidence, and human review model.
8. Approve Milestone 0 scope, schedule, and commercial terms.
