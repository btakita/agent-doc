# Plan: signal-space

## Thesis

`signal-space` is the shared surface layer for live signal graphs. It should
render, inspect, edit, and route signals regardless of whether a node is
deterministic, learned, or agentic.

The family should sit inside the lazily ecosystem, but not inside core lazily.
The dependency direction should be strict:

```text
lazily
  -> signal-space
    -> agent-doc
    -> Signal Loops / Patchboard
    -> other apps
```

Meaning:

- `lazily-*` owns reactive computation, state, sync, and cell/signal runtime.
- `signal-space-*` owns graph/timeline/inspector view models and UI intents.
- `agent-doc` consumes `signal-space`; `signal-space` does not depend on
  agent-doc.
- Agent-doc-specific integration belongs in agent-doc, preferably as
  `agent-doc-signal-space`.
- Agents are optional deliberative decision nodes, not the center of the model.

## Naming And Repository

Recommended family and repository prefix: `signal-space`.

Recommended initial repositories:

- `lazily-hub/signal-space-spec`
- `lazily-hub/signal-space-rs`
- `lazily-hub/signal-space-js`
- `lazily-hub/signal-space-py`
- `lazily-hub/signal-space-zig`
- `lazily-hub/signal-space-kt`

Do not create a new GitHub org yet. Start under the lazily public namespace so
the relationship to `lazily-spec`, `lazily-rs`, `lazily-py`, and `lazily-js` is
obvious. Create a separate `signal-loops` org later only if the product identity
needs its own home for hosted app code, docs, examples, or commercial adapters.

Initial repo responsibilities:

```text
signal-space-spec/
  schemas/
  fixtures/
  conformance/
  docs/

signal-space-rs/
  Cargo.toml
  src/
  tests/conformance/

signal-space-js/
  package.json
  src/
  examples/
    local-inspector/
  tests/conformance/

signal-space-py/
  pyproject.toml
  src/signal_space/
  tests/conformance/

signal-space-zig/
  build.zig
  src/
  tests/conformance/

signal-space-kt/
  settings.gradle.kts
  build.gradle.kts
  src/
  tests/conformance/
```

Keep the spec repo canonical. Implementation repos should pin a version of
`signal-space-spec`, run the shared conformance fixtures, and publish through
their native package managers. Split the repos from the start so each language
binding can keep its own release cadence, tooling, issue tracker, and packaging
metadata without forcing unrelated version bumps.

Implementation repos should not start as monorepos. Each should start as one
native library package at the repo root. Introduce an internal workspace only
after the repo owns multiple publishable artifacts, such as a runtime package,
CLI, WebAssembly package, code generator, or dedicated test harness.

Language support should mirror lazily over time:

- `signal-space-rs` depends on `lazily-rs`
- `signal-space-js` depends on `lazily-js`
- `signal-space-py` depends on `lazily-py`
- `signal-space-zig` depends on `lazily-zig`
- `signal-space-kt` depends on `lazily-kt`

There should not be a separate `signal-space-lazily` package unless a real
modularity problem appears. The lazily dependency is the runtime contract for
each implementation. The spec should still make the dependency explicit by
describing which fields are writable cells, which fields are deterministic
derived projections, and how Snapshot/Delta mirroring is expected to work.

## Boundaries

Extract from agent-doc into `signal-space` only where the concept is generic:

- typed surface descriptors and validation
- source-to-state projection primitives
- stable node, edge, span, and event ids
- graph, timeline, inspector, and table view models
- structured UI intents such as `select_node`, `edit_edge`, `approve_action`,
  `label_event`, `promote_candidate`, and `request_agent_review`
- advisory schedule and decision projections
- realtime mirroring hooks over lazily Snapshot/Delta

Keep in agent-doc:

- markdown document format
- typed `agent:*` element parsing and markdown-specific validation
- document-to-signal-space projection for `agent:*` elements
- read-only/advisory renderers for `agent:graph`, `agent:signals`,
  `agent:perceptron`, and `agent:schedule`
- git-native audit trail
- `finalize`, `session-check`, write, and commit boundaries
- binary-owned mutation authority
- queue, backlog, workers, jobs, and done semantics
- markdown-specific merge and repair policy
- agent-doc CLI and harness integration

The rule: `signal-space` may describe possible state changes as structured
intents. It must not bypass the owning system's authority boundary.

## Core Model

The core abstraction is a typed signal graph.

```text
source -> transform -> feature window -> decision node -> gate -> action
```

Node families:

- source: webhook, file, editor, git, CI, test, log, email, calendar, sensor,
  app event, model output
- transform: normalize, debounce, threshold, join, split, resample, rank,
  aggregate
- memory: rolling window, session state, entity history, label history,
  operator profile
- decision: deterministic rule, logistic model, tiny MLP, anomaly detector,
  small agent, large agent
- gate: confidence threshold, cooldown, approval required, budget limit,
  safety lock, single-writer effect authority
- output: alert, route, create task, write document, trigger agent, call
  webhook, update dashboard

Every node should expose:

- stable id
- type tag
- input and output schemas
- ownership and authority metadata
- current state summary
- recent events
- explainability packet
- allowed intents
- audit/provenance hooks

## Decision Node Contract

All decision modes should share one envelope:

```yaml
decision:
  id: string
  mode: deterministic | model | small_agent | large_agent
  inputs: []
  output_schema: {}
  confidence: number?
  explanation: {}
  proposed_intents: []
  authority: advisory | gated | direct
```

Mode-specific rules:

- deterministic: pure function over current inputs; replayable and testable
- model: fixed artifact plus feature schema; replay, shadow, promote, rollback
- small_agent: bounded packet, focused prompt, structured output, short run
- large_agent: explicit escalation path for novel or high-context work

Small and large agents are deliberative nodes. They should never be hidden
inside the realtime propagation path. A signal graph can trigger them, but
deterministic code owns launch, pause, resume, approval, timeout, and closeout.

## 12-Factor Agent Fit

Use the HumanLayer 12-factor style as the agent contract:

- own the context window: agents receive compact signal/evidence packets, not
  raw graph dumps
- tools are structured outputs: agents emit intents, not direct side effects
- unify execution and business state: durable events and decisions are visible
  in the same thread/projection
- launch, pause, resume: every agent node has a resumable run id
- own control flow: deterministic controllers decide whether to continue,
  wait, approve, retry, or stop
- small focused agents: keep small agents to one narrow decision or projection
- trigger from anywhere: signals, webhooks, editors, and documents can all wake
  a run through the same envelope
- stateless reducer: agent output is a function of an explicit packet plus
  instructions; durable state lives outside the model

## Agent-Doc Integration

Agent-doc should own its signal-space integration. Prefer the package name
`agent-doc-signal-space` if it becomes a distinct crate/package.

`agent-doc-signal-space` should project typed markdown elements into the
shared graph model, while preserving agent-doc's authority boundary.

Initial projections:

- `agent:exchange` -> conversation/event timeline
- `agent:queue` -> runnable prompt heads
- `agent:backlog` -> durable work pool
- `agent:workers` -> worker capacity and leases
- `agent:jobs` -> job packets and result summaries
- `agent:graph` -> work graph and dependencies
- `agent:signals` -> deterministic observations
- `agent:perceptron` -> advisory scoring graph
- `agent:schedule` -> recommended or accepted plan

Initial intents:

- select queue item
- inspect worker
- inspect job packet
- evaluate perceptron
- render advisory schedule
- request route
- request compact
- request human approval

All mutating intents must round-trip through agent-doc's existing closeout and
session-check boundary.

## Patchboard Integration

Patchboard or Signal Loops should own product-specific integration against
`signal-space`. The `signal-space-spec` repo can keep fixtures and conformance
examples, but product-specific adapters should follow the same rule as
agent-doc: apps depend on `signal-space`; `signal-space` does not depend on
apps.

A patchboard integration should project realtime signal applications into the
same graph/timeline/inspector model.

Initial projections:

- signal sources and schemas
- rolling feature windows
- decision nodes and confidence
- model candidates
- labels and overrides
- replay and shadow-mode runs
- promotions and rollbacks
- agent calls triggered by graph gates

Initial intents:

- add source
- connect edge
- label event
- train candidate
- replay candidate
- start shadow mode
- promote candidate
- rollback model
- request agent explanation

Promotion and external effects should require explicit approval unless the scope
is a sandbox.

## Package Phases

### Phase 0: Spec Skeleton

Create `signal-space-spec` as the canonical spec-first repository with schemas
for:

- `SignalGraph`
- `SignalNode`
- `SignalEdge`
- `SignalEvent`
- `TimelineEvent`
- `InspectorPanel`
- `SurfaceIntent`
- `DecisionEnvelope`
- `Authority`

Acceptance:

- JSON schemas exist
- example graph fixtures exist for agent-doc and patchboard
- conformance fixtures have stable names and documented version semantics
- schemas distinguish observation, recommendation, and action
- schemas distinguish writable cell state from deterministic derived
  projections

### Phase 1: Multi-Language Surface Contract

Define the language-binding contract shared by Rust, JavaScript, Python, Zig,
and Kotlin implementations in `signal-space-spec`.

Acceptance:

- each implementation repo declares the supported `signal-space-spec` version
- each binding depends on the matching lazily binding
- each binding serializes to the same spec fixtures
- validation catches invalid ids, edges, schemas, and authority transitions
- no binding depends on agent-doc internals

### Phase 2: Rust Implementation

Create `signal-space-rs` and implement it on top of `lazily-rs`.

Acceptance:

- graph state can be represented as lazily cells/signals
- derived inspectors update from cell changes
- Snapshot/Delta export supports read-only mirroring
- deterministic derived state stays separate from writable cells

### Phase 3: Agent-Doc Integration

Implement `agent-doc-signal-space` inside the agent-doc ownership boundary.

Acceptance:

- an agent-doc file can project `agent:*` elements into a `SignalGraph`
- `agent:perceptron` can render advisory `agent:schedule` output
- mutating intents remain advisory until agent-doc applies them
- tests prove projection is non-mutating

### Phase 4: JavaScript Surface Package

Create `signal-space-js` and implement it on top of `lazily-js` for
browser/editor UIs.

Acceptance:

- graph, timeline, inspector, and intent types are available in TypeScript
- fixtures round-trip with the Rust model
- a minimal web example can inspect an agent-doc graph

### Phase 5: Python Surface Package

Create `signal-space-py` and implement it on top of `lazily-py`.

Acceptance:

- core graph, timeline, inspector, intent, and authority types are available in
  Python
- fixtures round-trip with `signal-space-spec` conformance data
- validation catches invalid ids, edges, schemas, and authority transitions
- package metadata and typing policy are documented before publishing

### Phase 6: Zig Surface Package

Create `signal-space-zig` and implement it on top of `lazily-zig`.

Acceptance:

- core graph, node, edge, event, intent, and authority types are available in
  Zig
- fixtures round-trip with `signal-space-spec` conformance data
- validation catches invalid ids, edges, schemas, and authority transitions
- writable cell state remains separate from deterministic derived projections

### Phase 7: Kotlin Surface Package

Create `signal-space-kt` and implement it on top of `lazily-kt`.

Acceptance:

- core graph, timeline, inspector, intent, and authority types are available in
  Kotlin
- fixtures round-trip with `signal-space-spec` conformance data
- validation catches invalid ids, edges, schemas, and authority transitions
- JVM or multiplatform packaging is documented before publishing

### Phase 8: Patchboard MVP Adapter

Build the first realtime patchboard adapter against the same surface schema.

Acceptance:

- deterministic nodes, feature windows, and gates render in the same graph UI
- one tiny model node can expose replay/shadow/promotion metadata
- one agent node can be triggered by a gated signal packet
- labels and overrides appear on the timeline

### Phase 9: Productization Split

Only after the shared surface model is stable, decide whether Signal Loops needs
its own repo or org.

Acceptance:

- product identity or hosted app cadence diverges enough to justify a separate
  Signal Loops repo or org
- public docs can explain lazily vs signal-space vs Signal Loops cleanly
- adapter APIs are stable enough for external examples

## Near-Term Backlog

1. Create `lazily-hub/signal-space-spec` with schemas, fixtures, conformance
   docs, and README.
2. Draft `schemas/signal-space.schema.json` and two example fixtures:
   `agent_doc_supervisor.json` and `patchboard_attention_router.json`.
3. Add a design note documenting authority boundaries and advisory intents.
4. Create `lazily-hub/signal-space-rs` and implement structs plus schema
   validation on top of `lazily-rs`.
5. Add an agent-doc-owned projection spike, `agent-doc-signal-space`, that
   reads `agent:graph`, `agent:signals`, `agent:perceptron`, and
   `agent:schedule`.
6. Create `lazily-hub/signal-space-js` with a small inspector prototype using
   fixture data only.
7. Create `lazily-hub/signal-space-py` with core types and conformance fixture
   round-trips on top of `lazily-py`.
8. Create `lazily-hub/signal-space-zig` with core types and conformance fixture
   round-trips on top of `lazily-zig`.
9. Create `lazily-hub/signal-space-kt` with core types and conformance fixture
   round-trips on top of `lazily-kt`.
10. Decide whether the patchboard MVP should start as browser-first or Rust-first
   after the schema spike.

## Open Questions

- Should the public spec call the main object `SignalGraph`, `SurfaceGraph`, or
  `PatchGraph`?
- Should `agent:perceptron` project as a generic deterministic decision node or
  keep a dedicated perceptron node type?
- How much editor state belongs in `signal-space` versus an editor-specific
  adapter?
- Should agent-doc name its integration `agent-doc-signal-space`,
  `agent_doc_signal_space`, or keep it as an internal module until publishing?
- Should model artifacts live behind a generic `DecisionArtifact` contract from
  day one, or wait until the first patchboard MVP?
- Should small-agent and large-agent nodes share one `AgentDecision` envelope
  with a `model_tier` field, or remain separate modes?
- What is the minimum approval model for non-agent-doc patchboard effects?

<!-- agent:exchange -->
### Re: repo layout — gpt-5

No. The signal-space implementation repos should not start with a monorepo shape.

The intended shape is separate repos plus native single-package roots: `signal-space-rs` as one Cargo crate at the root, `signal-space-js` as one npm package at the root, `signal-space-py` as one Python package, `signal-space-zig` as one Zig package, and `signal-space-kt` as one Kotlin package. The canonical shared repo is `signal-space-spec`, which owns schemas, fixtures, conformance data, and docs.

The reason is release/tooling separation without unnecessary internal nesting. Each language binding has different package metadata, CI, publishing, and version pressure, so separate repos make sense. But a monorepo-style `crates/` or `packages/` layout inside each implementation repo adds indirection before there are multiple artifacts to coordinate. The plan now says to introduce an internal workspace only after a repo owns multiple publishable artifacts, such as a runtime package, CLI, WebAssembly package, code generator, or dedicated test harness.

I updated the plan accordingly and also made `signal-space-py`, `signal-space-zig`, and `signal-space-kt` explicit implementation phases with conformance fixture requirements.
<!-- agent:boundary:0c2b295b -->
<!-- /agent:exchange -->
