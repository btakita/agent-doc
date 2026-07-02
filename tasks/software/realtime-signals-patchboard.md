# Realtime Signals Patchboard

## Thesis

Agent loops need a realtime nervous system.

LLM agents are good at interpretation, synthesis, planning, tool creation, and
handling novel symbolic situations. They are weak as continuous low-latency
perception loops: they are too slow, expensive, nondeterministic, and verbose to
evaluate every tiny signal change.

A realtime signals patchboard fills that gap. It lets people route many live
signals through inspectable reactive circuits, some deterministic and some
learnable. Small neural networks, especially MLPs, become accessible as
"learnable mixers" inside a larger signal graph. They combine signal features,
produce confidence-bearing decisions, and decide when an agent loop should be
called.

The product is not "MLPs for non-programmers." The product is:

> Turn many messy realtime signals into stable decisions, alerts, rankings, and
> agent triggers without becoming an ML engineer.

## Product Shape

The interface is a realtime signal patchboard with scopes.

The closest metaphors are:

- a modular synth patchbay: nodes are modules with typed input/output jacks,
  edges are patch cables, and patching a signal should feel like plugging a
  cable into a rack
- Max/MSP for signal flow
- an oscilloscope for live inspection
- a spreadsheet for approachable tabular state
- a model debugger for training and promotion
- a versioned document session for auditability and collaboration

It should feel like a music patchboard, but for signals, information, and
logic that ordinary operators can use. Every cable is observable in the
moment — live value, sparkline, rate, freshness — and every module exposes
its state, including Harel state charts on state-bearing nodes.

Users wire streams into a graph. Every wire carries typed data and can be
inspected in realtime. Deterministic nodes transform signals. Learnable nodes
combine features and produce decisions. Agent nodes run slower deliberative
loops only when the signal graph says reasoning is needed.

## Supported Devices And Runtime Posture

The product should be CPU-first and laptop-first. A GPU should improve replay,
batch training, and heavy feature generation, but it should not be required for
the core experience.

Supported device tiers:

| Device tier | Role | Expected support |
|---|---|---|
| Laptop CPU | Primary design, training, replay, and live runtime | Fully supported |
| Desktop / workstation CPU | Larger histories, more streams, more candidates | Fully supported |
| Apple Silicon / integrated GPU | Optional acceleration via Metal, Core ML, WebGPU, or native kernels | Opportunistic acceleration |
| NVIDIA GPU workstation | Faster batch replay, large histories, embeddings, audio/video features | Optional acceleration |
| Mobile phone / tablet | Sensor collection, live monitoring, labeling, override, light inference | Supported as client and edge runtime |
| Mobile on-device training | Small online updates, personalization, tiny models | Supported cautiously; not the first batch-training target |
| Browser-only runtime | Demo, light patchboards, local UI, WebGPU/WebAssembly inference | Supported where connectors allow |
| Cloud worker / edge runtime | Connector polling, webhook intake, shared dashboards, low-state scoring | Supported for production deployment |

Mobile devices should be included, but with a clear role boundary. Phones and
tablets are excellent for collecting life signals, showing alerts, labeling
events in the moment, running tiny inference models, and doing light
personalization. They are not the default place for heavy historical replay or
multi-candidate batch training because battery, thermal limits, background
execution, and connector permissions are constrained.

The default posture:

- Run small MLP inference on any supported device.
- Train small models on laptop CPU by default.
- Use mobile for live labels, overrides, sensor streams, and small personal
  adaptation.
- Use GPU or cloud only when feature generation or replay volume justifies it.
- Keep raw sensitive signals local when possible; sync derived features,
  decisions, labels, and model metadata only when the user opts in.

## Primary Users

### Founder / Operator

Signals include email, calendar, Stripe, GitHub, CRM, analytics, Search Console,
ads, support tickets, and customer activity.

The output is an attention router: what matters now, what can wait, what should
be delegated, and what deserves an agent-generated plan.

### Ecommerce Operator

Signals include checkout events, payment failures, fraud indicators, Search
Console, GA4, inventory, ad spend, rankings, conversion rates, support messages,
and margin data.

The output is a revenue sentinel: detect checkout breakage, card-testing attacks,
SEO drops, paid-search anomalies, inventory traps, and product-page problems.

### AI-Agent Operator

Signals include diffs, tests, CI, logs, session state, prompt queues, graph
context, model output quality, repeated failures, and human corrections.

The output is an agent supervisor: continue, stop, reroute, escalate, summarize,
or trigger a stronger agent.

### Personal Life Systems User

Signals include calendar, tasks, messages, sleep, movement, location, notes,
finances, and habit data.

The output is a personal rhythm model: attention, energy, commitments, and
follow-through. This is a compelling long-term direction, but it is not the best
first commercial wedge because health-adjacent claims create risk.

### Market Signals User

Signals include market prices, volume, news, filings, social indicators,
portfolio exposure, and strategy state.

The output is regime detection, anomaly alerts, and decision support. This is
also compelling, but the first version should avoid trading promises and stay in
"signal intelligence" rather than "automated financial advice."

## Core Interface Modes

### 1. Design Mode

Design mode is where users build the reactive graph.

Node families:

- Source nodes: email, Slack, calendar, GitHub, Stripe, GA4, Search Console,
  wearable feeds, logs, sensors, market data, webhooks, CSV, local files
- Transform nodes: normalize, window, rolling average, z-score, debounce,
  threshold, rate limit, join, split, sample, resample, bucket, correlate
- Feature nodes: embedding, token count, sentiment, category, entity extraction,
  count over window, time since event, volatility, moving rank
- Memory nodes: rolling window, session state, profile state, recent labels,
  entity history, contact history
- Learnable nodes: classifier, regressor, ranker, anomaly detector, autoencoder,
  tiny MLP
- Gate nodes: confidence threshold, cooldown, approval required, business hours,
  budget limit, safety lock
- Output nodes: alert, suppress, route, rank, create task, send draft, trigger
  agent, write document, update dashboard, call webhook

Every edge has a compact live preview: sparkline, current value, rate, latency,
missing-data indicator, and distribution shape. Clicking an edge opens the raw
stream and its derived feature history.

The design principle:

> The graph around the MLP matters more to most users than the hidden neural
> topology inside it.

### 2. Training Mode

Training mode should feel like labeling a timeline, not writing ML code.

Users scrub through history and mark:

- this mattered
- false alarm
- should have alerted earlier
- ignore this pattern
- route to this workflow
- good decision
- bad decision
- this was uncertain but acceptable

The system turns those labels into training examples.

Training features:

- Replay historical signals through the graph
- Train candidate models from labeled windows
- Compare old graph vs new graph
- Show false positives and false negatives on the timeline
- Ask for labels on the most uncertain examples
- Show what changed after training
- Support batch retraining and online adaptation
- Keep a full version history of labels, feature definitions, model weights, and
  promotion decisions

The user-facing lifecycle is:

```text
review -> correct -> train candidate -> replay -> shadow -> promote
```

### 3. Topology Mode

Topology mode exposes the learnable part without requiring neural-network
expertise.

The default representation of an MLP is:

```text
recent signals -> feature window -> learnable mixer -> confidence -> gate -> action
```

Users choose from recipes:

- attention router
- anomaly detector
- lead scorer
- market regime detector
- ecommerce revenue sentinel
- agent supervisor
- churn risk signal
- personal rhythm monitor
- fraud or checkout failure detector

The normal knobs are semantic:

- more sensitive vs quieter
- adapt quickly vs stay stable
- faster vs more accurate
- explainable vs expressive
- conservative vs aggressive
- cheap vs thorough
- local-only vs cloud-assisted

Advanced users can open the mixer and tune:

- input feature set
- history window size
- hidden layer count
- width
- activation
- dropout / regularization
- learning rate
- batch size
- retrain cadence
- online-learning policy
- latency and memory budget

Topology mode should include topology suggestions, but suggestions must be
reviewable. The system can propose adding a feature, changing a window, splitting
a classifier, or inserting a gate. It should not silently change production
behavior.

### 4. Live Mode

Live mode is the operational cockpit.

It shows:

- current signal state
- active decisions
- confidence
- top contributing signals
- what action happened
- what would have happened under shadow models
- drift warnings
- recent overrides
- unresolved uncertainties
- agent calls triggered by the signal graph

Recommended layout:

- Left: realtime topology graph
- Middle: event timeline
- Right: selected node inspector, explanation, labels, and override controls

The main live actions are:

- approve
- reject
- quiet this
- wake agent
- explain
- replay similar cases
- label as good or bad
- promote candidate
- rollback promotion

## User Workflows To Create Networks

The patchboard should let users create networks from workflows they already
understand. A blank neural-network editor is the wrong starting point.

### Workflow 1: Recipe-Guided Network Creation

1. User chooses a recipe: attention router, revenue sentinel, agent supervisor,
   anomaly detector, lead scorer, or personal rhythm monitor.
2. App asks for the operational decision to improve: alert, rank, route,
   suppress, trigger an agent, or recommend an action.
3. User connects source accounts or imports local files.
4. System previews available signals and suggests feature windows.
5. User accepts or removes signals from the first candidate graph.
6. System creates a default learnable mixer and deterministic gates.
7. User labels a small timeline sample.
8. System trains a candidate and replays recent history.
9. User reviews false positives, false negatives, and similar cases.
10. Candidate runs in shadow mode.
11. User promotes it when behavior is trustworthy.

This is the default creation flow for non-technical users.

### Workflow 2: Build From A Live Event

1. User sees an event: a suspicious checkout spike, failing CI loop, urgent
   email, missed follow-up, or anomalous market move.
2. User clicks "make this a signal."
3. System shows the surrounding timeline and contributing streams.
4. User marks the event as important, noisy, expected, or dangerous.
5. System proposes a small graph that could detect similar events earlier.
6. User chooses an output: alert, route, suppress, or wake an agent.
7. Candidate graph runs in shadow mode against future events.

This flow turns lived experience into training data without asking the user to
think in ML terms.

### Workflow 3: Design From An App Screen

1. User points at an app workflow: inbox triage, checkout health, CI run,
   support queue, trading watchlist, or habit dashboard.
2. Agent or skill identifies decisions embedded in that workflow.
3. Patchboard suggests signals already visible in the UI plus hidden backend
   signals that would improve the decision.
4. User selects which actions can be automated and which require approval.
5. System creates the signal graph, training labels, live view, and gates in one
   design pass.

This is the workflow for designing the app and the MLP network in tandem.

### Workflow 4: Agent-Assisted Topology Iteration

1. User asks an agent to improve a noisy or underperforming network.
2. Agent reads graph version, replay metrics, labels, false positives, false
   negatives, and operator notes through MCP.
3. Agent proposes topology edits: add a feature, change a window, split a model,
   add a gate, lower sensitivity, or create a shadow candidate.
4. User approves the topology change.
5. System trains and replays the candidate.
6. User promotes or rejects the candidate.

The agent can propose graph changes, but production behavior only changes
through the normal candidate -> replay -> shadow -> promote lifecycle.

### Workflow 5: Mobile Labeling And Personalization

1. User receives a mobile notification or sees a live signal card.
2. User labels the event in the moment: useful, noisy, later, urgent, route to
   someone, or wake an agent.
3. Mobile app performs a tiny local update when appropriate, or queues labels
   for the laptop/desktop trainer.
4. The next replay shows how those mobile labels changed behavior.

This makes mobile devices part of the training loop without requiring them to be
the main model-development environment.

## Shadow Mode

Shadow mode is the killer workflow.

A candidate graph or model runs live without controlling outputs. It records what
it would have done. The user compares:

- current production behavior
- candidate behavior
- human labels
- missed events
- noisy events
- latency
- action cost

Only after review does the user promote the candidate.

This avoids the dangerous pattern where training immediately changes live
behavior. It also creates a natural sales demo: connect signals, run shadow mode,
show what the model would have caught, then promote a bounded first automation.

## Core Abstractions

### Signal

A timestamped value or event from a source.

Examples:

- scalar: current conversion rate
- vector: embedded email thread
- event: checkout failed
- window: last 30 minutes of payment failures
- decision: route to support
- label: human marked false alarm

### Stream

An ordered flow of typed signals with rate, latency, freshness, and backpressure
metadata.

### Patchboard

A graph of nodes and edges. It is reactive: downstream nodes recompute when their
dependencies change.

### Scope

A boundary for data, permissions, history, and model state.

Examples:

- personal
- business
- client
- project
- mailbox
- ecommerce store
- agent session

### Feature Window

A structured slice of recent history converted into a model input vector.

### Learnable Mixer

The user-facing abstraction for an MLP. It learns how to combine features into a
score, class, rank, anomaly measure, or action recommendation.

### Gate

A deterministic boundary after a learnable node. Gates keep learned behavior from
directly causing unsafe actions.

Examples:

- confidence must be above 0.85
- no more than one alert per hour
- never send externally without approval
- only trigger a high-cost agent if estimated value is high

### Agent Node

A slow reasoning node that receives a bounded packet from the signal graph and
returns a plan, explanation, draft, or tool action.

The agent does not watch every signal. It is called when the patchboard detects a
situation worth reasoning about.

## Model Lifecycle

The product should make model state as inspectable as code state.

Lifecycle:

1. Collect signals
2. Define feature windows
3. Label history
4. Train a candidate
5. Replay historical data
6. Run in shadow mode
7. Promote to active
8. Monitor drift
9. Correct mistakes
10. Retrain
11. Roll back if needed

Promotion should create an immutable record:

- graph version
- feature definitions
- training labels and ranges
- model configuration
- model weights hash
- replay metrics
- shadow metrics
- promotion operator
- timestamp
- rollback pointer

This maps well to a git-native workflow.

## Explanation Model

The product does not need perfect neural interpretability at first. It needs
operator-grade explanation.

For each decision, show:

- input signals used
- feature window
- top contributing features
- nearby historical examples
- confidence
- gate result
- action taken or suppressed
- shadow-model alternatives
- human correction history

The best explanation is often not "inside the neural net." It is:

> Here are the recent signal patterns that made this look like earlier events
> you labeled as important.

## Relationship To Agent Loops

The signal patchboard is the perception and control layer. Agent loops are the
deliberative layer.

Patchboard responsibilities:

- watch continuous streams
- maintain rolling state
- detect changes
- filter noise
- classify repeated patterns
- trigger actions under deterministic gates
- wake agents only when needed

Agent responsibilities:

- interpret novel situations
- explain complex events
- write new connectors
- propose topology changes
- synthesize reports
- draft messages
- handle sparse symbolic tasks
- perform multi-step planning

Potential agent-doc integration:

- A patchboard node can trigger an agent-doc session with a bounded packet.
- tsift can provide graph/source/session context as features.
- corky can provide message, contact, topic, and follow-up signals.
- agent-doc can record topology changes, model promotions, and operator
  decisions in a persistent document session.

The long-term architecture:

```text
signals -> reactive patchboard -> learnable mixers -> gates -> agent loops -> audited actions
```

## Skills And MCP Design

Skills should encode design judgment. MCPs should expose live app, signal,
training, replay, and model state.

### Skill: `signal-model-designer`

Use this skill when designing an app that needs realtime signals, MLPs,
classifiers, anomaly detectors, training UX, shadow mode, or agent-triggering
logic.

The skill should guide the agent to:

1. Define the app workflow and user-visible decisions.
2. Identify realtime signals and their owners.
3. Separate deterministic rules from learnable decisions.
4. Define feature windows and label sources.
5. Choose the smallest viable model type.
6. Design one or more topology candidates.
7. Define feedback UI so normal app usage creates labels.
8. Define replay, shadow mode, promotion, rollback, and drift monitoring.
9. Add deterministic gates between learned outputs and risky actions.
10. Produce a coupled product topology and signal/model topology.

Standard skill output:

```md
# Signal Model Design

## App Goal
## User Workflow
## Realtime Signals
## Decisions To Learn
## Deterministic Rules
## Feature Windows
## Model Candidates
## Training UX
## Replay And Shadow Mode
## Gates And Safety
## Agent Loop Integration
## MVP Cut
## Open Questions
```

### Supporting Skills

Additional skills can stay folded into `signal-model-designer` until the work
repeats enough to justify splitting them out.

Possible future skills:

- `patchboard-ux-designer`: design graph, timeline, inspector, and live
  operations UI.
- `mlp-topology-designer`: choose small CPU-friendly network shapes and training
  settings.
- `realtime-training-designer`: design labeling, replay, online learning, and
  shadow-mode workflows.
- `agent-supervisor-designer`: specialize the pattern for coding agents,
  `agent-doc`, `tsift`, tests, diffs, and CI signals.

### MCP: `patchboard-model-mcp`

The MCP should make the design skill evidence-backed. It should expose the live
patchboard state instead of forcing the agent to infer from prose.

Useful tools/resources:

- `list_apps()`
- `describe_app(app_id)`
- `export_patchboard_graph(app_id)`
- `list_signals(app_id)`
- `describe_signal_schema(signal_id)`
- `sample_signal_history(signal_id, time_range, limit)`
- `list_decisions(app_id)`
- `describe_decision(decision_id)`
- `list_label_sets(app_id)`
- `record_label(event_id, label, source)`
- `propose_feature_windows(decision_id)`
- `create_model_candidate(spec)`
- `train_candidate(candidate_id, label_set_id)`
- `replay_candidate(candidate_id, time_range)`
- `compare_candidates(candidate_ids)`
- `start_shadow_mode(candidate_id)`
- `get_shadow_metrics(candidate_id)`
- `promote_candidate(candidate_id)`
- `rollback_model(model_id)`
- `explain_decision(decision_id, event_id)`

The MCP boundary should be conservative. Agents may create candidates and run
replays. Promotion should require explicit user approval unless the scope is a
safe sandbox.

## Local Stack Fit

This idea aligns with existing assets:

- `lazily-rs` / `lazily-dart` / `lazily-js`: the reactive computation substrate,
  now real across three conforming runtimes — Rust (native/server), Dart
  (Flutter: iOS, Android, desktop, web), and JavaScript (browser/editor) — all
  sharing the `lazily-spec` wire protocol and Harel state-chart semantics
- `signal-space`: the shared graph / timeline / inspector / intent surface that
  sits on top of lazily; every product projects its runtime state into this
  shape and validates mutations through its own authority boundary
- `patchboard` (`btakita/patchboard`): the first product adapter over
  signal-space — a Rust spike with one logistic learnable mixer, deterministic
  gates, advisory agent triggers, timeline labels, and gated promote/rollback
- `agent-doc`: git-native session interface, audit, promotion review, human-agent
  co-editing
- `tsift`: bounded evidence packets, code/session graph context, digesting noisy
  logs and diffs into features
- `corky`: correspondence signals, contacts, topics, follow-up loops, scheduled
  outputs
- `rappstack` / Cloudflare Workers: vertical app deployment path
- Presence AI experience: realtime multimodal signals, streaming, latency,
  backpressure, model operations
- demand-model: time-series contracts, interventions, causal guardrails, and
  small-data honesty

## MVP

The first MVP should avoid general-purpose visual programming. Build one wedge
with enough generality to prove the substrate.

Recommended wedge: AI-agent supervisor or ecommerce revenue sentinel.

### MVP Features

- Stream table with live values and history
- Patchboard graph with source, transform, learnable, gate, and output nodes
- Edge inspection with sparkline and raw values
- One learnable mixer node backed by a tiny MLP or logistic model
- Timeline labeling UI
- Historical replay
- Shadow mode
- Promotion and rollback
- Agent trigger output
- Explanation panel with top signals and similar examples

### MVP Non-Goals

- Full neural-network editor
- Automated topology mutation in production
- Fully general connector marketplace
- Medical claims
- Automated trading
- Deep-learning model zoo
- Distributed multi-tenant enterprise controls

## First Commercial Wedges

### 1. AI-Agent Supervisor

Buyer: teams already using coding agents.

Inputs:

- test results
- CI
- git diff
- tsift graph context
- agent-doc cycle state
- prompt queue state
- repeated failure patterns
- user corrections

Outputs:

- continue
- stop
- ask human
- reroute to stronger model
- run targeted tests
- summarize
- create follow-up item

Why it is a good first wedge:

- It uses the existing stack directly.
- Users already feel agent-loop pain.
- The value is immediate: fewer runaway loops, better continuation decisions,
  better evidence.

### 2. Ecommerce Revenue Sentinel

Buyer: small ecommerce store owners and operators.

Inputs:

- orders
- payment failures
- cart events
- fraud indicators
- GA4
- Search Console
- ad spend
- inventory
- support messages

Outputs:

- alert
- likely cause
- action checklist
- agent-generated diagnosis
- revenue impact estimate

Why it is a good wedge:

- Concrete money pain.
- Existing MonsterRodHolders experience gives credibility.
- Works as a productized service before SaaS.

### 3. Founder Attention Router

Buyer: founder, consultant, solo operator.

Inputs:

- inbox
- calendar
- contacts
- docs
- tasks
- billing
- repository activity

Outputs:

- priority queue
- follow-up suggestions
- suppressions
- agent-triggered drafts
- weekly attention report

Why it is a good wedge:

- Corky provides a strong base.
- It is personally dogfoodable.
- It can expand into life signals later.

## Technical Shape

### Runtime

- Local-first event log
- Reactive graph runtime
- Typed stream schema
- Node-level persistence
- Model artifact registry
- Replay engine
- Shadow runner
- Promotion ledger
- Connector adapters

The reactive graph runtime is `lazily`, and the product UI is **Slint**
(https://slint.dev/) rather than Flutter. Slint is Rust-native, so the UI
links directly against `lazily-rs`, `signal-space-rs`, and the patchboard
adapter in one process with no FFI boundary, and it also compiles to
WebAssembly for a browser observer:

| Runtime | Frontends | Role |
|---------|-----------|------|
| `lazily-rs` + Slint | native desktop (Linux, macOS, Windows), WebAssembly | primary product UI; heavy replay and batch training; realtime I/O host |
| `lazily-js` | browser, editor extensions | read-only mirrors and dashboards over the Snapshot/Delta stream |
| `lazily-dart` | Flutter (iOS, Android) | optional mobile companion (labels, alerts, overrides); not the product UI |

All three share `lazily-spec` (the wire protocol and state-chart behavior) and
project through `signal-space`, so a patchboard's graph, timeline, inspectors,
decisions, and intents mean the same thing on every device. The product stays
local-first: raw signals stay on-device when possible, and only derived
features, decisions, labels, and model metadata sync across runtimes when the
user opts in.

### Realtime I/O

The patchboard is not a closed simulation. It has first-class ingress and
egress so other systems can feed it and be notified in realtime.

Ingress connectors:

- webhook intake (HTTP)
- WebSocket subscription to remote feeds
- file tail (logs, JSONL)
- stdin JSONL
- timers and clocks
- service adapters (email, CI, git, Stripe, GA4, agent-doc state, tsift
  packets, corky signals)

Egress connectors:

- webhook POST with a bounded JSON packet
- WebSocket broadcast of the signal-space Snapshot/Delta stream — the spine
  that lets dashboards, browser observers, and other systems watch the rack
  live
- desktop notifications
- allowlisted command execution
- advisory agent triggers (bounded packet to an agent loop such as agent-doc)

I/O rules:

- every egress action passes a deterministic gate and the signal-space
  authority boundary; no connector holds direct authority
- every action taken or suppressed lands on the timeline
- connector failures are themselves signals (freshness and missing-data
  indicators flip)
- credentials are referenced by name and resolved by the host; secrets never
  live in the patchboard document

### Model Options

Start with simple models:

- logistic regression
- small MLP
- online linear model
- autoencoder for anomaly detection
- gradient-boosted tree later if useful

The point is not to maximize architecture novelty. The point is to make the
learnable node reliable, inspectable, fast, and easy to train.

### Training Throughput On Supported Devices

Training throughput depends on feature extraction, memory layout, framework,
batch size, thermal limits, and whether replay is recomputing features. The
numbers below are rough planning ranges for small patchboard models, not
benchmarks.

Assumed model shapes:

- Tiny: 32-128 input features, 1-2 hidden layers, 16-64 units, under 25k
  parameters.
- Small: 128-512 input features, 2 hidden layers, 64-256 units, 25k-250k
  parameters.
- Medium: 512-2,048 input features, 2-3 hidden layers, 128-512 units,
  250k-2M parameters.

Approximate training throughput:

| Device tier | Tiny model | Small model | Medium model | Notes |
|---|---:|---:|---:|---|
| Laptop CPU | 50k-500k examples/sec | 10k-150k examples/sec | 1k-30k examples/sec | Good default for local batch training |
| Desktop / workstation CPU | 100k-1M examples/sec | 25k-300k examples/sec | 5k-75k examples/sec | Better sustained thermals and memory bandwidth |
| Apple Silicon with acceleration | 100k-1M examples/sec | 30k-400k examples/sec | 5k-100k examples/sec | Depends heavily on Metal/Core ML path |
| NVIDIA GPU workstation | 200k-2M+ examples/sec | 100k-1M+ examples/sec | 25k-500k examples/sec | Best for large replay batches; overhead hurts tiny batches |
| Mobile CPU / NPU | 5k-100k examples/sec | 1k-25k examples/sec | 100-5k examples/sec | Best for inference, labels, and tiny personalization |
| Browser WebAssembly | 10k-200k examples/sec | 2k-50k examples/sec | 200-10k examples/sec | Good for demos and local-first web use |
| Browser WebGPU | 50k-1M examples/sec | 20k-300k examples/sec | 2k-100k examples/sec | Availability and driver behavior vary |
| Cloud worker / edge CPU | 10k-200k examples/sec | 2k-50k examples/sec | Usually avoid | Runtime limits favor inference and light updates |

Practical expectations:

- Inference for tiny and small MLPs should be microseconds to low milliseconds
  on laptops and usually acceptable on mobile.
- Single-example online updates should usually be sub-millisecond to a few
  milliseconds for tiny models on laptop CPU.
- Mini-batch updates of 32-256 examples should usually complete in milliseconds
  to tens of milliseconds for tiny or small models.
- Historical replay over feature windows will often bottleneck on IO, feature
  generation, and UI reporting before MLP math.
- Mobile should support tiny local adaptation, but batch retraining should
  normally happen on laptop/desktop or cloud.
- GPU support is useful for heavy replay, embeddings, audio/video feature
  generation, or many candidate models. It is not required for the MVP.

The first product promise should be:

> Local laptop training, no GPU required. Mobile participates through live
> signals, labels, overrides, inference, and light personalization.

### Storage

Store:

- raw signals when allowed
- normalized signals
- feature windows
- labels
- graph versions
- model versions
- decisions
- explanations
- promotions
- rollbacks

For privacy-sensitive scopes, keep raw payloads local and store only derived
features or hashes in shared layers.

## Safety And Trust

Important guardrails:

- Human approval for external actions
- Deterministic gates after learned nodes
- Shadow mode before promotion
- Rollback as a first-class action
- Drift detection
- Label provenance
- Clear separation between observation, recommendation, and action
- No hidden retraining that changes live behavior
- No medical, legal, or trading claims in the first product

## Naming Ideas

- Signal Loops
- Patchboard
- Nerveboard
- Reactive Signal Studio
- CircuitMind
- SignalScope
- Learned Circuits
- Agent Nervous System

The clearest descriptive name is probably **Signal Loops**.

## Open Questions

- Is the first product a standalone app, an agent-doc companion, or a vertical
  productized service?
- Should the first learnable node be an MLP, logistic classifier, or anomaly
  detector?
- What is the minimum connector set for the first wedge?
- Can topology and promotion history live cleanly inside agent-doc documents?
- What is the best visual language for typed realtime streams?
- How much model detail should be hidden by default?
- What is the right local/cloud boundary for connectors and model training?
- Which mobile workflows are core: sensor source, labeling surface, live
  monitor, tiny personalization runtime, or all of them?
- Should the first MCP expose only read/replay tools, or allow candidate
  creation and training from day one?

## Short Summary

Realtime signals patchboard is a reactive complement to agent loops. It lets
users wire live signals through inspectable circuits, train small learnable
mixers from timeline feedback, test candidates in shadow mode, and trigger agent
loops only when a situation deserves reasoning.

Agents become the deliberative layer. The patchboard becomes the perception and
control layer.
