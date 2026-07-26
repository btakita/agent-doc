# Haiven SDK — Milestone Proposal

Prepared for Luke Richey and Craig Fryar

Prepared by Brian Takita

Date: July 26, 2026
Status: Discussion draft; commercial terms become binding only in a signed statement of work

## Executive summary

Haiven can make safe chat, presence, community, moderation, and social features
materially easier for indie studios to adopt. I propose a milestone-scoped
engagement to define the SDK contract, ship a production-minded Unity vertical
slice, validate it with a pilot studio, and leave Haiven with the code,
documentation, tests, and operating knowledge needed to expand across engines.

The key design choice is **one stable Haiven client contract with thin,
idiomatic engine adapters**. That keeps authentication, WebSocket lifecycle,
reconnect/resume behavior, message envelopes, safety events, and telemetry
consistent while allowing each engine to feel native to its developers.

The repository model should mirror that boundary: a thin integration
superrepo pins dedicated repositories for contracts, conformance, every
shipping SDK, and the agent harness. Each SDK keeps independent ownership,
CI, permissions, and releases while the superrepo provides a reproducible
workspace and compatibility evidence. The complete design is in the
[Haiven SDK superrepo design](sdk-superrepo-design-2026-07-26.md).

I would lead architecture and implementation in Haiven-owned repositories,
working in a bounded 10–15 hour/week band with weekly demos. Haiven approves
each milestone before the next begins, so cost, scope, and technical fit remain
visible throughout.

## What this engagement should accomplish

1. Reduce a studio's path from “interested” to a working Haiven-powered sample.
2. Make safety and moderation capabilities first-class SDK concepts, not a
   dashboard-only afterthought.
3. Prove the client contract under unreliable networks, reconnects, duplicate
   sessions, application pause/resume, and engine main-thread constraints.
4. Create a conformance suite so later SDKs behave consistently instead of
   becoming independent implementations.
5. Give Haiven a credible engine roadmap without committing capital to every
   platform before developer demand is demonstrated.

## Agentic development method

I propose an agent-assisted delivery process modeled on what I learned building
Lazily's cross-language library family. Lazily's relevant proof is not a runtime
dependency; it is the development method: define behavior once, establish a
reference implementation, encode conformance scenarios, then use agents to
accelerate idiomatic adapters, tests, documentation, and review evidence.

The operating loop would be:

1. A human approves the product behavior, protocol contract, acceptance tests,
and architecture decision.
2. An agent receives a bounded work packet with those exact constraints and
   implements one reviewable change.
3. A separate test/review pass challenges the change with negative cases,
   protocol fixtures, lifecycle faults, and engine-specific constraints.
4. CI proves compilation, unit behavior, conformance, packaging, and sample
   installation; a green label is not accepted unless the intended fixture
   corpus actually executed.
5. A human reviews the code, API, security/privacy implications, and developer
experience before merge or release.

The same process can be made easier to use through a small, Haiven-owned agent
harness:

- A **read-only MCP server** gives supported coding agents version-scoped access
to Haiven SDK documentation, protocol schemas, ADRs, examples, compatibility
tables, and troubleshooting guidance. Answers cite the exact source/version,
separate required behavior from recommendations, and expose an opinionated
"paved road" without hiding alternatives or unresolved decisions.
- A **thin verification CLI** gives humans and agents the same deterministic
commands for environment checks, package validation, protocol/conformance
fixtures, sample-project smoke tests, documentation links, and evidence
bundling. It does not merge, release, deploy, or handle production credentials.
- An **agent-request evaluation suite** exercises representative tasks such as
scaffolding a client, implementing reconnect behavior, adding a moderated chat
flow, diagnosing a version mismatch, and migrating an SDK version. Each case
asserts expected artifacts and commands, source grounding, conformance results,
and forbidden behaviors—not merely whether the generated answer sounds useful.

Milestone 0 would define this harness contract, its source-of-truth and access
policy, the initial CLI surface, and a scored evaluation catalog. Milestone 1
would implement a narrow internal prototype alongside the Unity SDK, with final
scope and price confirmed after discovery. The harness stays an aid rather than
a required runtime dependency: studios can still integrate Haiven with ordinary
documentation and tooling.

Agents can parallelize later engine adapters only after the reference contract
and Unity implementation are stable. They do not receive production credentials,
approve their own architecture, merge their own work, deploy to production, or
turn generated code into an unsupported claim. Every change remains traceable
from requirement to decision, code, tests, review, and release artifact.

This gives Haiven the speed benefit of agentic development while preserving the
control, auditability, and behavioral parity that a safety-sensitive SDK needs.

## Proposed first engagement

### Milestone 0 — contract and integration design

**Calendar:** 1–2 weeks

**Budgetary range:** $1,500–$2,500 fixed fee

Deliverables:

- SDK product brief: target developer, supported workflows, non-goals, and
  version-support policy.
- Versioned client contract for REST + WebSocket behavior.
- Authentication/session lifecycle and token-refresh design.
- Connection state machine covering connect, authenticate, heartbeat,
  reconnect, resume, backoff, logout, and terminal failure.
- Initial feature inventory for chat, presence, notifications, community,
  moderation/safety events, and telemetry.
- Agentic development playbook: work-packet template, human approval gates,
agent roles, evidence requirements, secret/data boundaries, and merge/release
policy.
- Agent-harness design: versioned MCP documentation/guidance contract, content
provenance and access policy, verification CLI command surface, and an initial
agent-request evaluation catalog with pass/fail rubrics.
- Superrepo design and bootstrap: component manifest, reproducible lock,
dedicated-repository ownership/access map, cross-repository change train, and
integration CI contract.
- Unity technical design, package layout, sample-scene plan, and acceptance
test plan.
- Sequenced delivery backlog and refined fixed price for Milestone 1.

Acceptance: Luke, Craig, and the designated technical reviewer approve the
contract, Unity scope, acceptance tests, and pilot success criteria.

### Milestone 1 — Unity prototype and SDK foundation

**Calendar:** 4–6 weeks

**Budgetary range:** $6,000–$9,000 fixed fee after Milestone 0

Deliverables:

- Unity Package Manager package with a small, idiomatic C# API.
- Authentication and connection lifecycle.
- Core send/receive flows for the agreed chat, presence, notification, and
  safety/moderation surface.
- Main-thread event dispatch and game/application lifecycle handling.
- Reconnect, retry, cancellation, bounded buffering, and structured error
  behavior.
- Runnable reference scene showing login, connection state, chat, presence,
  moderation/safety feedback, and diagnostic output.
- Automated unit tests plus protocol conformance tests shared with future SDKs.
- CI packaging and install-from-repository validation.
- Desktop plus Android/iOS build validation; console compilation is a separate
access-gated lane.
- Developer quick start and API reference.
- If authorized after Milestone 0: a read-only MCP documentation/guidance
prototype, thin verification CLI, and CI-runnable agent-request eval suite
covering the agreed Unity vertical slice.

Acceptance: a clean sample project installs the package, connects to the Haiven
test environment, completes the agreed flows, survives scripted network
interruptions, and passes the conformance suite. If the harness prototype is
authorized, its acceptance also requires version-correct cited answers,
deterministic CLI evidence, and passing good-path, ambiguity, and unsafe-request
evals across the supported agent harnesses.

### Milestone 2 — pilot hardening and beta release

**Calendar:** 2–3 weeks

**Budgetary range:** $3,500–$5,000 fixed fee

Deliverables:

- Integration with one designated pilot game or studio.
- Fixes and usability improvements from observed developer friction.
- SemVer/version-compatibility policy and release checklist.
- SDK telemetry limited to agreed operational signals, with privacy boundaries
  documented.
- Troubleshooting guide, migration notes, and support handoff.
- Signed beta tag/package and prioritized recommendations for the next adapter.

Acceptance: the pilot completes the agreed integration checklist, no
release-blocking SDK defects remain, and Haiven can reproduce the build,
package, tests, and sample without contractor-only infrastructure.

### Pilot envelope

The expected first-engagement total is **7–11 calendar weeks** and
**$11,000–$16,500**, subject to Milestone 0 findings. Each milestone is
independently authorized. No later engine or console work is implied by approval
of the Unity pilot.

## Engine and runtime strategy

Engines/frameworks and runtime platforms should be planned separately. Unity,
Unreal, Godot, GameMaker, Cocos Creator, Construct 3, Bevy, Clickteam Fusion,
MonoGame, and Source are integration surfaces; Android, iOS, Windows, macOS,
Linux, web, Nintendo, PlayStation, and Xbox are runtime/certification targets.

| Priority | Integration surface | Recommendation |
|---|---|---|
| 1 | **Unity / C#** | First vertical slice. One adapter reaches a broad range of indie desktop and mobile projects, with closed-console validation added only after access is available. |
| 2 | **Godot** | Strong indie fit and open ecosystem. Choose GDScript, C#, or a native extension only after pilot-customer discovery. Validate desktop, Android, iOS, and web constraints explicitly. |
| 2 | **Unreal / C++ + Blueprint** | High-value second major-engine candidate. Expose a Blueprint-friendly facade over a testable C++ core; validate desktop and mobile first. |
| 2 | **Native mobile** | Add Swift/iOS and Kotlin/Android clients if Haiven expects native companion apps or non-engine mobile titles. Consider React Native only when a concrete product requires it. |
| 3 | **GameMaker** | Stronger market-balancing candidate for commercial 2D indie games. Start with a GML-facing package plus platform extensions where required; validate desktop, mobile, and web before any access-gated console work. |
| 3 | **Cocos Creator / TypeScript** | Adds a mobile- and web-oriented 2D/3D ecosystem beyond the Unity/Godot/Unreal cluster. Prefer a TypeScript client/component package with native bridges only where the runtime requires them. |
| 3 | **Construct 3** | Reaches low-code and small-studio web/mobile/desktop projects. Package Haiven as a JavaScript/TypeScript addon with actions, conditions, expressions, and a sample project; proceed after a named pilot confirms demand. |
| 4 | **MonoGame / FNA** | Treat this as the modern XNA-family target. Do not invest in a new Microsoft XNA SDK, which was discontinued; serve current .NET indie projects instead. |
| 4 | **Bevy / Rust** | Technically aligned but currently too small to anchor market coverage. Keep a clean Rust crate/plugin design available for a named pilot or unusually strong pipeline evidence. |
| 4 | **Clickteam Fusion 2.5** | This is the current name for the Multimedia Fusion lineage. Build an extension only after a named studio validates demand and exporter constraints. |
| 4 | **Source SDK / mods** | Treat as a game/mod-specific C++ integration, not a general modern engine channel. Scope licensing and commercial distribution before implementation. |
| Access-gated | **Nintendo Switch/Switch 2, PlayStation 5, Xbox Series X\|S** | These are validation and certification lanes across selected engine adapters. They require Haiven or a pilot partner to provide approved developer-program access, platform SDKs, hardware, and NDA-compliant CI. |

Defold, Phaser, GDevelop, Roblox, Discord, and other creator/community channels
can be evaluated alongside this matrix. They should enter the roadmap through a
named studio, qualified pipeline evidence, or a low-cost reuse path rather than
displacing the first engine pilot.

## Commercial and operating model

- **Control:** work occurs in Haiven-owned repositories, issue tracking, and CI.
- **Cadence:** 10–15 committed hours/week, written weekly update, and one live
  demo/review.
- **Change control:** work outside an accepted milestone becomes a priced change
  or a later milestone; it is not silently absorbed.
- **Payment:** 50% at milestone start and 50% at acceptance, net 10, with an
  agreed acceptance window.
- **Ownership:** Haiven owns paid-for project deliverables. My pre-existing
  tools, libraries, and general know-how remain background IP unless separately
  licensed. Lazily may be shown as relevant prior work; it is not a required SDK
  dependency.
- **Continuity:** design notes, decisions, tests, samples, and build procedures
  live with the code from day one.
- **Security/privacy:** no production user data enters local samples or tests;
  test identities and synthetic events are used by default.
- **Availability:** start date and hours are subject to written
  conflict-of-interest and invention-assignment clearance from my primary
  engagement.

## Success measures

Milestone 0 will set exact targets. The initial scorecard should include:

- time for a new developer to reach authenticated connect and first
  send/receive using the quick start;
- conformance pass rate across reference client and Unity adapter;
- reconnect/resume success across scripted failure cases;
- percentage of public SDK operations covered by runnable samples and API docs;
- pilot integration time and number/severity of SDK-blocking issues;
- percentage of agent-authored changes with requirement, test, independent
  review, and CI evidence linked before merge;
- correct delivery of agreed safety/moderation signals without requiring each
  studio to reinvent policy plumbing.

## Decision requested

Authorize Milestone 0 only. At its review, choose one of three paths:

1. proceed with the Unity prototype as scoped;
2. revise the first adapter based on pilot demand; or
3. stop with the design, contract, and conformance plan retained by Haiven.

That decision structure lets Haiven buy clarity first and implementation only
after the integration contract and business priority are concrete.

## Public platform references

- [Unity multiplatform support](https://unity.com/features/multiplatform) and
  [console access requirements](https://unity.com/solutions/console)
- [Unreal Engine packaging and target platforms](https://dev.epicgames.com/documentation/en-us/unreal-engine/packaging-your-project)
- [Godot supported export platforms and console constraints](https://docs.godotengine.org/en/stable/about/faq.html#which-platforms-are-supported-by-godot)
- [GameMaker extension packages](https://manual.gamemaker.io/lts/en/The_Asset_Editors/Extensions.htm)
and [export platforms](https://gamemaker.io/en/blog/export-with-gamemaker)
- [Cocos Creator TypeScript/JavaScript support](https://docs.cocos.com/creator/3.7/manual/en/scripting/language-support.html)
and [extension packages](https://docs.cocos.com/creator/3.8/manual/en/editor/extension/create-extension.html)
- [Construct 3 Addon SDK](https://www.construct.net/en/make-games/manuals/addon-sdk)
and [publishing targets](https://www.construct.net/en/make-games/manuals/construct-3/overview/publishing-projects)
- [Bevy platform-specific Android/iOS examples](https://github.com/bevyengine/bevy/blob/main/examples/README.md#platform-specific-examples)
- [MonoGame supported platforms](https://docs.monogame.net/articles/getting_started/platforms.html)
  and [console-access requirements](https://docs.monogame.net/articles/console_access.html)
- [Clickteam Fusion exporters](https://www.clickteam.com/clickteam-product-downloads)
- [Valve Source SDK 2013](https://github.com/ValveSoftware/source-sdk-2013)
- [Nintendo developer process](https://developer.nintendo.com/the-process) and
  [PlayStation Partners](https://partners.playstation.net/)
