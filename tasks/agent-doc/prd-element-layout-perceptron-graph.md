# PRD: Element Layout And Perceptron Graph

## Problem

agent-doc needs to support two related use cases without splitting into
incompatible document formats:

- a single large model working with a human in one interactive session
- a network of small harness loops coordinated through shared task, worker, and
  runtime state

The current `agent:exchange`, `agent:queue`, and `agent:backlog` elements are a
good base, but they do not yet provide a typed control-plane layout for workers,
jobs, runtime signals, graph decisions, or advisory routing. Without a shared
element model, agent-doc risks growing separate "session document" and
"orchestrator document" formats that duplicate lifecycle and closeout rules.

## Goals

- Keep one markdown document substrate for both single-agent and multi-harness
  workflows.
- Model each use case through typed `agent:*` elements with explicit ownership,
  parser, merge, scheduling, and audit semantics.
- Add a graph-oriented element layout that can describe work nodes, worker
  nodes, dependencies, signals, and schedule recommendations.
- Add an optional perceptron graph element for inspectable, deterministic
  advisory routing decisions over document/runtime signals.
- Keep actual mutations to queue, backlog, workers, and jobs behind the normal
  agent-doc closeout and session-check boundary.

## Non-Goals

- Do not create a separate orchestrator-only document format.
- Do not let a perceptron or model-generated graph silently mutate runnable
  work.
- Do not make neural or statistical routing load-bearing for correctness.
  Deterministic guards, dependencies, leases, and closeout proofs remain
  authoritative.
- Do not require external graph databases for the first implementation slice.
- Do not inline child transcripts or large worker logs into the orchestrator
  element layout.

## Element Model

The document remains a container of independently typed elements. Each element
descriptor should declare:

- name and accepted marker attributes
- parser and validation rules
- local authority model
- merge policy
- whether the element can produce runnable work
- whether agents may mutate it directly
- audit/proof requirements

Candidate elements:

| Element | Role |
|---|---|
| `agent:exchange` | Human/main-agent conversation and response history |
| `agent:backlog` | Durable task pool |
| `agent:queue` | Runnable prompt heads and batch controls |
| `agent:workers` | Harness loop registry, leases, current jobs, readiness |
| `agent:jobs` | Job packet index and worker-result summaries |
| `agent:graph` | Work graph, dependencies, ownership, and schedule state |
| `agent:signals` | Deterministic runtime observations and derived facts |
| `agent:perceptron` | Optional advisory scoring graph over signals |
| `agent:schedule` | Generated or accepted schedule recommendations |
| `agent:done` | Completion archive |

## Perceptron Graph

`agent:perceptron` is an inspectable scoring layer over existing document and
runtime signals. It should not own execution. It reads declared inputs from
`agent:signals`, `agent:queue`, `agent:workers`, `agent:jobs`, or graph
projection facts; computes deterministic scores; then writes an advisory
projection into `agent:schedule` or emits a planning record.

Example shape:

```yaml
<!-- agent:perceptron format=yaml version=1 -->
inputs:
  queue_depth:
    source: agent:queue.open_count
  stale_workers:
    source: agent:workers.stale_count
  ci_failed:
    source: agent:signals.ci_failed
  high_context:
    source: agent:signals.context_accretion

nodes:
  dispatch_small_fix:
    activation: threshold
    threshold: 0.5
    bias: -0.4
    weights:
      queue_depth: 0.3
      stale_workers: -0.7
      ci_failed: 0.6
      high_context: -0.5
    action:
      type: recommend_route
      target: worker.small-fix

  compact_before_drain:
    activation: threshold
    threshold: 0.6
    bias: -0.2
    weights:
      high_context: 0.9
      queue_depth: 0.2
    action:
      type: recommend_queue
      prompt: "compact exchange before continuing"
<!-- /agent:perceptron -->
```

The output is advisory:

```yaml
<!-- agent:schedule format=yaml generated-by=agent:perceptron -->
recommended:
  - action: recommend_route
    target: worker.small-fix
    score: 0.71
  - action: recommend_queue
    prompt: "compact exchange before continuing"
    score: 0.68
<!-- /agent:schedule -->
```

Applying a recommendation still requires a normal binary-owned mutation such as
adding a queue item, creating a job packet, claiming a worker, or finalizing a
response.

## Scheduling Contract

- `agent:backlog` and `agent:queue` define local work.
- `agent:workers` and `agent:jobs` define execution capacity and worker result
  evidence.
- `agent:signals` defines deterministic observed state.
- `agent:graph` defines dependencies and ownership constraints.
- `agent:perceptron` scores possible actions from visible inputs.
- `agent:schedule` records the recommended or accepted plan.
- `finalize` / `write --commit` / `session-check` remain the only completion
  boundary for document-visible work.

## Acceptance Criteria

- A document can declare the element layout without changing the existing
  session document format.
- Element descriptors exist for graph, workers, jobs, schedule, and perceptron
  candidates, even if early implementations preserve them as typed inert
  elements.
- `agent:perceptron` parsing validates input sources, numeric weights, activation
  type, threshold, and action shape.
- Perceptron evaluation produces bounded advisory output and never mutates
  `agent:queue`, `agent:backlog`, `agent:workers`, or `agent:jobs` directly.
- `agent:graph` can reference backlog item ids, queue heads, job packet ids, and
  worker ids without copying child transcripts.
- The same `finalize` and `session-check` rules apply whether a task came from a
  human prompt, queue head, schedule recommendation, or worker result.

## First Implementation Slice

1. Add a spec for the element descriptor contract and the proposed control-plane
   elements.
2. Add inert parser/round-trip support for `agent:graph`, `agent:workers`,
   `agent:jobs`, `agent:schedule`, and `agent:perceptron`.
3. Add deterministic validation and evaluation for `agent:perceptron` behind a
   non-mutating command, for example `agent-doc graph evaluate <FILE> --json`.
4. Render advisory schedule output without applying it.
5. Add tests proving evaluation is bounded, deterministic, and non-mutating.

## Open Questions

- Should `agent:perceptron` be its own element or a typed section inside
  `agent:graph`?
- Should accepted schedules live in `agent:schedule`, `agent:queue`, or both
  with explicit provenance?
- How much of worker state belongs in the document versus sidecars under
  `.agent-doc/`?
- Should perceptron inputs be limited to built-in signal paths initially, or can
  custom plugin signal paths be declared in the first slice?
