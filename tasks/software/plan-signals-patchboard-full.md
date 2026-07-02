# Plan: Signals Patchboard — Full Implementation (Slint UI)

## Thesis

Build the full Signals Patchboard product on the existing substrate:

```text
lazily-rs (reactive runtime)
  -> signal-space-rs (surface contract + lazily-runtime mirror)
    -> patchboard (product adapter: mixers, gates, labels, lifecycle, runtime)
      -> patchboard-app (Slint UI + realtime I/O host)
```

The UI is **Slint** (https://slint.dev/), not Flutter. Slint is Rust-native,
compiles `.slint` markup at build time, renders via Skia/FemtoVG on
winit, and also targets WebAssembly — so the desktop-first product keeps a
browser demo path without a second UI codebase. The Rust core (lazily-rs,
signal-space-rs, patchboard) links directly into the app process; there is no
FFI boundary between the runtime and the UI.

**Feel:** a music patchboard for signals. Nodes are modules with typed
input/output jacks. Edges are patch cables. Every cable is observable — live
value, sparkline, rate, freshness — like plugging a scope into a modular
synth. Deterministic logic nodes and learnable mixers (logistic + MLP) sit in
the same rack. State charts (Harel statecharts, mirroring lazily's
`StateMachine`) are first-class visible state on nodes.

**I/O:** the patchboard is not a closed simulation. Ingress connectors bring
live signals in (webhooks, WebSocket, file tail, timers, service adapters);
egress connectors notify other systems in realtime (webhook POST,
WebSocket Snapshot/Delta stream, desktop notification, exec, agent trigger).
All external effects pass deterministic gates and the signal-space authority
boundary.

`signal-space` stays the domain interface: the UI renders
`SignalSpaceDocument`/`SignalGraph` and emits `SurfaceIntent`s; the
`PatchboardRuntime` owns mutation authority and revalidates every change.

## Current State (what already exists)

- `signal-space-spec` 0.2.0 — schema, fixtures (`agent_doc_supervisor`,
  `patchboard_attention_router`), conformance manifest, `state_chart` on
  `SignalNode` (states, initial, current, transitions `{from, event, to}`;
  events conventionally match intent types).
- `signal-space-rs` — full type set, `parse_document`/`validate_document`,
  `runtime` module over lazily-rs: `SignalSpaceRuntime`, `InspectorSummary`,
  `MirrorExport`, Snapshot/Delta export.
- `patchboard` (Phase 9 MVP adapter) — `builders` (attention-router graph),
  `Mixer` trait + `LogisticMixer` (ndarray), `GatePolicy`
  (confidence/cooldown/approval), advisory `small_agent` trigger, `LabelKind` +
  `TrainingSet`, `CandidateRecord` lifecycle, `PatchboardRuntime`
  (revalidate-and-revert, rolling window, timeline, gated promote/rollback,
  shadow replay), `state_chart` module. No UI, no live I/O.
- `patchboard-mcp` — MCP control plane (list/describe/train/replay/shadow/
  promote/rollback, gated `approve_intent`), `signal-model-designer` skill.
- `lazily-rs` — reactive substrate with Harel state-chart semantics; lazily-js
  for later browser mirrors.

Gaps this plan closes: MLP mixer, typed ports (jacks), stream telemetry,
I/O bindings in the spec, a realtime connector host, and the entire Slint UI.

## Dependency & Authority Rules (unchanged)

- `patchboard-app` depends on `patchboard` + `signal-space-rs` + `slint`.
  Nothing in signal-space depends on patchboard; nothing here depends on
  agent-doc.
- No node holds `direct` authority. External effects are `gated`; agent
  triggers are `advisory`. Promotion/rollback require explicit approval
  through the UI or `patchboard.approve_intent` (MCP).
- The UI never mutates the graph directly: every edit is a `SurfaceIntent`
  validated by `PatchboardRuntime` (revalidate-and-revert).

## Phase 1 — Spec 0.3.0: Ports, Stream Telemetry, I/O Bindings (#spb1)

Build out `signal-space-spec` + `signal-space-rs` for the patchboard's needs.

Schema additions (all optional; 0.2.0 documents keep validating):

- `PortSpec` on `SignalNode`: `{ id, name, direction: in|out, dtype:
  scalar|vector|event|window|decision|label, required }` — typed jacks.
  `SignalEdge` gains optional `from_port`/`to_port`.
- `StreamTelemetry` on `SignalEdge`: `{ rate_hz, latency_ms, freshness_ms,
  last_value_preview, distribution_hint, missing_data }` — the live-cable
  readout, declared as deterministic derived state (never writable cells).
- `IoBinding` on `SignalNode`: `{ direction: ingress|egress, transport:
  webhook|websocket|file_tail|timer|stdin_jsonl|exec|notify|mcp, endpoint,
  format, schema_ref, auth_ref }` — declares how a source/output node binds
  to the outside world. Secrets stay out of the document (`auth_ref` is a
  name, resolved by the host).
- Fixture updates: extend `patchboard_attention_router.json` with ports,
  telemetry, and one ingress (webhook) + one egress (webhook POST) binding;
  add a `patchboard_io_rack.json` fixture exercising all transports.

`signal-space-rs`: matching types, validation (port dtype compatibility on
edges, ingress-only on sources, egress-only on outputs, no `direct`
authority on egress), round-trip tests against new fixtures.

Acceptance:

- spec 0.3.0 published with fixtures + conformance manifest updates
- signal-space-rs validates port/dtype/edge compatibility and authority rules
- 0.2.0 fixtures still validate (backward compatible)

## Phase 2 — MLP Mixer + Training Loop (#spb2)

Add the multi-layer perceptron beside the logistic mixer in `patchboard`.

- `MlpMixer` implementing the existing `Mixer` trait: ndarray feedforward,
  1–2 hidden layers (16–256 units), ReLU/tanh, mini-batch SGD (optionally
  momentum/Adam), CPU-only, tiny/small model shapes from the vision doc.
- Explanation packet: per-feature contribution via input×first-layer-weight
  attribution plus nearest labeled examples — operator-grade, not
  perfect interpretability.
- `Trainer`: batch train from `TrainingSet`, deterministic seed, train/val
  split, early stopping; emits `CandidateRecord` metrics (precision/recall on
  labeled windows) compatible with the existing replay/shadow/promote
  lifecycle.
- Deterministic nodes and MLP nodes share the `DecisionEnvelope` — mode
  `deterministic` vs `model` — so the UI treats them uniformly.

Acceptance:

- MlpMixer trains on synthetic fixtures and beats LogisticMixer on a
  nonlinear labeled fixture (XOR-style windowed pattern)
- replay/shadow/promote/rollback work unchanged with MLP candidates
- inference stays microseconds–low-milliseconds for tiny/small shapes on CPU

## Phase 3 — Realtime I/O Host (#spb3)

New `patchboard-io` module (or crate in the patchboard workspace): a tokio
host that executes `IoBinding`s around `PatchboardRuntime`.

Ingress connectors:

- `timer` (interval clock), `stdin_jsonl`, `file_tail` (log/JSONL tail),
  `webhook` (axum HTTP intake), `websocket` (subscribe to a remote feed)

Egress connectors (all gate-guarded):

- `webhook` POST with bounded JSON packet
- `websocket` server broadcasting signal-space Snapshot/Delta — this is the
  "other systems can observe/be notified in realtime" spine, and the same
  stream a future browser mirror consumes
- `notify` (desktop notification), `exec` (allowlisted command), `mcp`
  (advisory agent trigger packet, e.g. an agent-doc session)

Host rules:

- backpressure + debounce owned by the runtime graph, not connectors
- every egress action records a `TimelineEvent` (action taken/suppressed,
  gate result)
- connector failures are signals too (source freshness/missing-data flips)

Acceptance:

- a demo rack ingests a webhook + timer, routes through window → mixer →
  gate, and POSTs an egress webhook end-to-end with latency < 100ms locally
- Snapshot/Delta WebSocket stream is consumable by a second process
- kill/restart of a connector degrades to `missing_data` without wedging the
  runtime

## Phase 4 — Slint App Skeleton: Load, Render, Observe (#spb4)

Create `patchboard-app` (Slint binary crate; patchboard repo becomes a
workspace — the "multiple publishable artifacts" threshold is now met).

- slint 1.x, `slint-build` compiling `.slint` files; winit backend, Skia
  renderer (FemtoVG fallback).
- Load a `SignalSpaceDocument`, validate, hand to `PatchboardRuntime` +
  `patchboard-io` host on a tokio runtime thread.
- UI thread receives runtime updates via `invoke_from_event_loop` +
  `Weak<AppWindow>`; graph state projects into Slint `VecModel`s
  (`NodeVM`, `CableVM`, `TimelineVM`). No polling — lazily deltas drive UI
  invalidation.
- Read-only first: rack view with node modules (name, family badge, state
  summary, jack rows) and bezier patch cables (Slint `Path`), live cable
  telemetry (current value + sparkline + freshness dot), pan/zoom via
  `Flickable` + wheel/pinch handlers.
- Three-pane layout from the vision doc: rack/graph left, event timeline
  middle, inspector right.

Acceptance:

- opens the Phase 3 demo rack and shows live-updating cables at 60fps with
  10+ streams on laptop CPU
- clicking a cable opens the scope panel: raw stream + derived feature
  history (oscilloscope-style rolling plot)
- clicking a node fills the inspector (ports, state fields, decision
  envelope, allowed intents)

## Phase 5 — Patch Canvas Interactions: the Music-Patchboard Feel (#spb5)

Make it feel like patching a modular synth.

- Drag modules on the rack; drag from an out-jack to a compatible in-jack to
  patch a cable (dtype-checked live — incompatible jacks visibly reject).
- Cable aesthetics: slack/bezier hang, per-dtype color, animated pulse when a
  signal fires, dimmed when stale.
- Unpatch, repatch, add node from a module palette (source/transform/
  feature/memory/learnable/gate/output families), delete node.
- Every edit emits a `SurfaceIntent` (`add_node`, `connect_edge`,
  `remove_edge`, …) → runtime revalidate-and-revert; a rejected intent
  animates the cable snapping back with the validation reason as a toast.
- Scope panel dockable: pin multiple cables to a multi-trace scope
  (oscilloscope metaphor); VU-style level meters on output nodes.
- Keyboard: arrow-nudge, cmd/ctrl+Z undo (intent journal replay).

Acceptance:

- an operator can build the attention-router rack from a blank document in
  under 5 minutes using only the palette + jacks
- invalid patches (dtype/authority) are impossible to commit and visibly
  explained
- undo/redo replays through the intent journal, never bypassing validation

## Phase 6 — State Chart UI (#spb6)

Surface `state_chart` (spec 0.2.0, mirroring lazily `StateMachine<S, E>`) as
a first-class panel.

- Node badge shows `effective_current()` state; state-bearing nodes get a
  chart glyph.
- Inspector "State" tab renders the statechart: states as rounded nodes,
  transitions as arrows labeled with events, current state highlighted;
  transition animates when the runtime applies an event.
- Since events conventionally match intent types, the panel offers gated
  "fire event" buttons that emit the corresponding `SurfaceIntent`; the
  owning adapter decides whether the transition applies (authority boundary
  unchanged).
- Timeline records state transitions as `TimelineEvent`s; scrubbing the
  timeline replays chart state.

Acceptance:

- the gate node's statechart (armed → cooldown → approval_pending → released)
  is visible live during the Phase 3 demo
- firing an event without authority is rejected and rendered as advisory
- lazily-rs remains the single source of statechart semantics (no UI-side
  state machine reimplementation)

## Phase 7 — Training, Labeling, Lifecycle UI (#spb7)

The `review -> correct -> train candidate -> replay -> shadow -> promote`
lifecycle in the UI.

- Timeline labeling: scrub history, mark events (mattered / false alarm /
  should-have-alerted-earlier / ignore / good / bad) → `LabelKind` +
  `TrainingSet`.
- Train-candidate dialog: pick mixer type (logistic | MLP), semantic knobs
  first (sensitive↔quiet, adapt↔stable, faster↔accurate), advanced drawer for
  hidden layers/width/lr/batch (Phase 2 Trainer).
- Replay view: candidate vs production over history; false positives/negatives
  plotted on the timeline.
- Shadow mode: candidate runs live without controlling outputs; side-by-side
  "would have done" column in live mode.
- Promote/rollback: gated approval dialog writing the immutable
  `CandidateRecord` (graph version, weights hash, metrics, operator,
  rollback pointer). Uncertainty sampling: "label these next" queue.

Acceptance:

- full lifecycle runs end-to-end in the UI on the demo rack without touching
  a terminal
- promotion is impossible without the approval dialog; rollback is one gated
  action; both land in the timeline + `CandidateRecord`
- shadow metrics visibly diverge from production before promotion (the sales
  demo from the vision doc)

## Phase 8 — Service Integration Wedge: Agent Supervisor (#spb8)

First commercial wedge from the vision doc, using our own stack as the
signal source.

- Ingress adapters (via Phase 3 transports, no new spec surface): git/CI
  status (`gh` poll or webhook), test results (digest-runner JSONL),
  agent-doc cycle state (session/queue state file tail), tsift evidence
  packets, corky follow-up signals.
- Supervisor rack recipe: repeated-failure detector, runaway-loop detector,
  continuation scorer (MLP), gates for "wake agent" / "ask human" /
  "reroute to stronger model".
- Egress: desktop notify, webhook, advisory agent-doc trigger with a bounded
  packet (never direct).
- Ship as a bundled recipe (Workflow 1 from the vision doc) + fixture.

Acceptance:

- the supervisor rack runs against this repo's live agent-doc sessions and
  produces at least continue/stop/ask-human decisions in shadow mode
- one week of dogfooding labels retrains a measurably better candidate
- `patchboard-mcp` exposes the same rack to agents (list/describe/replay)

## Phase 9 — Packaging, Docs, Demo (#spb9)

- Release builds for Linux/macOS (Windows best-effort); `cargo dist` or
  equivalent; app icon, `.desktop`/bundle metadata.
- WASM spike: compile the read-only rack view to WebAssembly consuming the
  Phase 3 WebSocket stream (browser observer, no authority).
- Docs: README rework across patchboard/patchboard-app, authority model,
  connector how-to, recipe gallery; screencast-able demo patch.
- Update `realtime-signals-patchboard.md` open questions with what got
  decided (done alongside this plan: Slint posture, music-patchboard feel,
  realtime I/O section).

Acceptance:

- a new user goes from download → demo rack → shadow-mode result without
  reading source
- browser observer renders the same rack read-only from the delta stream

## Sequencing & Parallelism

Phases 1→2→3 are substrate and can proceed serially (2 can start during 1
review). Phase 4 starts as soon as 1 lands (it renders 0.3.0 documents);
5–7 are UI-serial on 4. Phase 8 needs 3 + 7; Phase 9 is last. The critical
path is 1 → 4 → 5 → 7.

## Risks

- **Slint node-canvas ceiling**: Slint has no built-in node editor; custom
  canvas (Flickable + Path + TouchArea) is proven feasible but complex
  interactions (cable dragging + zoom + 60fps telemetry) need an early spike
  — front-load in Phase 4/5; fall back to reduced cable animation before
  falling back on architecture.
- **UI-thread pressure from high-rate streams**: coalesce deltas (frame-rate
  batching) at the mirror boundary; lazily debounce nodes own semantic
  throttling.
- **Spec creep**: ports/telemetry/io are the only 0.3.0 additions; UI layout
  state stays app-local, never in the spec.
- **MLP scope**: no GPU, no model zoo; ndarray CPU only until replay volume
  proves otherwise (vision-doc posture).

## Open Questions

- Does `patchboard-io` live as a module in the `patchboard` crate or a
  workspace sibling? (Decide at Phase 3 by dependency weight — axum/tokio
  should not leak into the pure adapter.)
- Slint license: GPL/commercial/community — confirm the community license
  fits the intended distribution before Phase 9 packaging.
- Should the WASM observer reuse signal-space-js instead of Slint-wasm?
  (Defer; both consume the same delta stream.)
- Where does undo history live — app-local intent journal only, or should
  the spec gain an intent-journal envelope? (Start app-local.)
