# Plan: Patchboard MVP Adapter (Phase 9)

## Thesis

`btakita/patchboard` is a **product adapter** over the existing `signal-space`
surface, not a new substrate. It projects a live realtime signal graph
(deterministic nodes + feature window + one learnable mixer + gate + agent
trigger) into the shared `SignalGraph` shape, and it owns the runtime authority
that `signal-space` deliberately does not.

This plan implements **Phase 9 (Patchboard MVP Adapter)** of
[`plan-signal-surface.md`](plan-signal-surface.md) and the MVP Features section
of [`realtime-signals-patchboard.md`](realtime-signals-patchboard.md), scoped to
what can be proven on top of the 0.1.0 surface contract without building a UI.

The design principle from the vision doc governs scope:

> The graph around the MLP matters more to most users than the hidden neural
> topology inside it.

So the spike invests in node families, the feature window, gates, timeline
labels, and the mixer's lifecycle metadata — not in a neural-net topology
editor.

## What This Spike Is — And Is Not

**Is (MVP adapter):** a Rust library that builds and reacts a `SignalGraph`
matching the canonical `patchboard_attention_router.json` fixture, with one
real (logistic) learnable mixer, deterministic gates, advisory agent intents,
timeline labels, and replay/shadow/promotion metadata. Proven by tests against
the shipped fixture.

**Is not (deferred):** visual patchboard UI / oscilloscope, connector
marketplace, full neural-net topology editor, MLP/GNN model zoo, mobile client,
distributed multi-tenant cloud, online drift detection, GBT. These are
post-MVP; they are listed under Non-Goals.

## Decisions That Need Approval

These are the choices I will assume unless you redirect. Each is reversible but
expensive to change late.

1. **Rust-first, single crate.** The sibling adapter `agent-doc-signal-space`
   is Rust, and `signal-space-rs` already ships the `lazily-runtime` feature
   with `SignalSpaceRuntime` (writable graph cell + derived inspector/mirror
   signals). Rust-first lets us reuse the reactive substrate directly and defer
   the browser UI to a later `signal-space-js` mirror. The plan's open question
   #12 ("browser-first or Rust-first") is resolved **Rust-first**.
   - Alternative: browser-first via `signal-space-js`. Higher UI risk, no live
     reactive substrate to bind to yet.

2. **Logistic mixer first, MLP second.** The vision doc's Open Question #2
   asks MLP vs logistic vs anomaly detector. For the spike, a logistic
   regressor (single linear layer + sigmoid) is the honest smallest viable
   mixer: it is a degenerate MLP, trivially inspectable (per-feature weights
   ARE the explanation), CPU-trivial, and needs no new dependencies. The
   fixture's `decision.explanation.top_features` maps directly to logistic
   weights. `Mixer` is a trait so a tiny MLP plugs in as the next branch.
   - Alternative: MLP-first. More expressive but obscures explanation at MVP.

3. **`patchboard` is a product adapter, not a language binding.** It does
   **not** go in `signal-space-spec/conformance/bindings.json` (that registry
   is for `signal-space-{rs,js,py,zig,kt}`). Per `docs/binding-contract.md`
   §Product Boundary, no signal-space binding may depend on patchboard;
   patchboard depends on `signal-space-rs` + `lazily-rs`. This matches the
   repo being created under `btakita/patchboard` rather than
   `signal-space/patchboard`.

4. **Authority posture matches the fixture.** Graph-level `default: gated`,
   `boundary: runtime`, `owner: patchboard`, `requires_approval: true`.
   Model `promote_candidate`/`rollback_model` are `gated`. Gate node holds
   actions (`held: true`). Agent node proposes `advisory` intents only. No
   `direct` authority anywhere; `validate_document` already enforces this.

5. **No UI in the spike.** Prove the reactive graph + mixer + lifecycle
   through tests and the `signal-space-mcp` control plane. A live inspector
   (browser or TUI) is the first post-MVP branch.

## Architecture

### Dependency Direction

```text
lazily-rs
  -> signal-space-rs (with lazily-runtime feature)
    -> patchboard
signal-space-mcp  (control plane; patchboard is exposed through it, not depended on)
```

`patchboard` re-exports nothing from signal-space as its own public surface; it
constructs `signal_space::SignalGraph` values and validates them through
`signal_space::validate_document`, mirroring how `agent-doc-signal-space`
operates.

### Reused Surface (do not reinvent)

From `signal-space-rs/src/lib.rs`:
- Types: `SignalGraph`, `SignalNode`, `NodeFamily`, `SignalEdge`, `NodeState`,
  `StateField`, `StateClass`, `TimelineEvent`, `DecisionEnvelope`,
  `SurfaceIntent`, `Target`, `Authority`, `AuthorityLevel`, `IntentModule`,
  `IntentSchema`, `SPEC_VERSION`.
- Validation: `validate_document` (catches duplicate ids, unknown edge
  endpoints, derived+writable fields, `trainable_model.lifecycle` without
  capability, advisory→direct escalation).
- Runtime (`lazily-runtime` feature): `SignalSpaceRuntime` (graph cell +
  derived `inspectors()` signal + read-only `mirror_export()`).

From `signal-space-spec/fixtures/patchboard_attention_router.json`: the **target
shape** the spike must produce from a live runtime, not merely parse. Five nodes
(`source.editor`, `window.route_features`, `model.route_priority`,
`gate.approval`, `agent.review_patch`) over four edges, with
`trainable_model.lifecycle`, `agent.lifecycle`, and `approval.gates` intent
modules.

### Patchboard-Owned Layer

The crate adds what signal-space deliberately does not own:
- **Node builders** that emit fixture-shaped `SignalNode`s with correct
  authority per family.
- **`PatchboardRuntime`**: wraps `SignalSpaceRuntime`; the graph is the one
  writable cell; patchboard semantics (windowing, gating, mixer scoring) are
  derived signals over that cell.
- **`Mixer` trait + `LogisticMixer`**: the one learnable node. Exposes
  replay/shadow/promotion through `DecisionEnvelope.capabilities` and
  `StateField`s (`model.active_candidate`, `model.confidence`).
- **Gate logic**: confidence threshold + cooldown + `requires_approval`.
- **Agent trigger**: a `small_agent` decision that emits an advisory
  `SurfaceIntent` (`approve_action` / `wake_agent`) only when the gate
  releases.
- **Label/override pipeline**: labels and overrides land as `action`-class
  timeline events with provenance and feed the next training pass.

## Branch Breakdown

Branch prefix follows `AGENTS.local.md`: `yyyymmdd/short-description`. Today is
2026-06-30.

### Branch 1 — `20260630/bootstrap`

Repo skeleton for the empty `btakita/patchboard`.

Deliverables:
- `Cargo.toml`: single crate `patchboard`, `edition = "2024"`,
  `rust-version = "1.88"`, `license = "MIT"`, `spec-version = "0.1.0"` metadata
  field (mirrors sibling pattern), `repository = "https://github.com/btakita/patchboard"`.
  Deps: `signal-space = { path = "../signal-space-rs" }`, `anyhow`, `serde`,
  `serde_json`. Dev-deps only for tests.
- `.gitignore`, `LICENSE` (MIT), `README.md` stating the authority boundary and
  that this is a product adapter depending on signal-space-rs + lazily-rs.
- `.github/workflows/ci.yml` mirroring `agent-doc-signal-space` CI: checkout
  self + `signal-space/signal-space-rs` + `lazily-hub/lazily-rs`, then
  `cargo fmt --check` + `cargo test`. **Flag:** the repo is private, so CI
  needs either a PAT secret or `permissions: { contents: read }` on a private
  runner — confirm with owner.
- `src/lib.rs` with crate-level docs re-stating the dependency direction.
- agent-loop: register `src/patchboard` in `.gitmodules` as
  `git@github.com:btakita/patchboard.git` (note: the `signal-space-*` repos
  are not yet in `.gitmodules` either; this branch does not fix that).

Acceptance:
- `cargo build` / `cargo fmt --check` / `cargo test` (empty test) pass.
- `cargo tree` shows no dependency from signal-space-rs back into patchboard.

### Branch 2 — `20260630/fixture-projection`

Pure builders that construct the fixture-shaped graph. The analog of
`agent-doc-signal-space::project_graph`.

Deliverables:
- `builders.rs`: `source_node`, `feature_window_node`, `model_node`,
  `gate_node`, `agent_node`, plus `attention_router_graph()` assembling them
  per the fixture, and `attention_router_document()` wrapping with
  `SPEC_VERSION`.
- Reuse the fixture's exact intent modules (`trainable_model.lifecycle`,
  `agent.lifecycle`, `approval.gates`) and `allowed_intents`.
- All mutation-bearing nodes stay `advisory`/`gated`; no `direct`.

Acceptance:
- `validate_document(&attention_router_document())` is `Ok`.
- Round-trip: `serde_json::to_value` then back equals the original
  (structural) shape.
- A test loads the canonical
  `../signal-space-spec/fixtures/patchboard_attention_router.json` and asserts
  the builder produces the same node ids, edges, intent module ids, and
  authority levels (value-for-value where deterministic; field-by-field for
  scalar `state` values).

### Branch 3 — `20260630/reactive-runtime`

`PatchboardRuntime` over `SignalSpaceRuntime`. Graph = the writable cell;
patchboard semantics = derived signals.

Deliverables:
- `runtime.rs`: `PatchboardRuntime::new(graph)` owning a `SignalSpaceRuntime`.
  Public read accessors: `graph()`, `inspectors()`, `mirror_export()` (all
  delegated, read-only).
- `update_graph(impl FnOnce(&mut SignalGraph))` to mutate the cell, matching
  `SignalSpaceRuntime::update_graph`. Mutations that cross authority (promote,
  rollback, release gate) go through dedicated methods that revalidate.
- Feature window: a `memory` family node backed by a lazily signal over the
  source cell, maintaining a bounded rolling vector (capacity from fixture's
  `window: 5m` explanation).
- Timeline append helper that writes `TimelineEvent`s with correct
  `state_class` and `node_id`.

Acceptance:
- Pushing events into the source node updates the window signal and appends a
  `features_updated` observation timeline event.
- `mirror_export()` stays read-only; mutating it does not compile (type-driven
  check in tests via attempt-and-assert).
- Revalidation after every mutation: a change that would break an invariant
  (e.g. orphaned edge) is rejected before publish.

### Branch 4 — `20260630/learnable-mixer`

The one learnable node. Logistic first; trait leaves room for MLP.

Deliverables:
- `mixer.rs`: `trait Mixer { fn score(&self, features:&[f64]) -> f64; fn
  train_step(&mut self, features:&[f64], label:f64, lr:f64); fn
  feature_weights(&self) -> Vec<(String,f64)>; }`
- `LogisticMixer`: pure `Vec<f64>` weights + sigmoid; no ndarray dep at the
  spike (add ndarray in the MLP branch if needed). Deterministic init for
  reproducible tests.
- Wire into `model.route_priority`: `DecisionEnvelope.capabilities` includes
  `trainable_model.lifecycle`; `StateField`s expose `model.confidence`
  (derived) and `model.active_candidate` (writable, `mlp_v4`/`logistic_v1`).
  Explanation = top-N |weights| as `decision.explanation.top_features`.
- `train_step` consumes labeled windows; `score` produces the
  `priority_score` written to the timeline as a `model_scored` recommendation.

Acceptance:
- A tiny synthetic dataset (two separable clusters) trains to a separating
  score within N steps; test asserts confidence ordering on held-out examples.
- `validate_document` still passes after training updates the model node.
- Top contributing features returned match the largest-magnitude weights.

### Branch 5 — `20260630/gate-and-agent-trigger`

Deterministic boundary after the mixer + the slow reasoning node.

Deliverables:
- `gate.rs`: gate releases only when `confidence >= threshold` AND cooldown
  elapsed AND `requires_approval` honored. `held` is a writable `action`
  field. Gate transitions append `gate_held`/`gate_released` timeline events.
- `agent.rs`: `agent.review_patch` is a `small_agent` `DecisionEnvelope`. When
  the gate releases, it emits an **advisory** `SurfaceIntent`
  (`approve_action` targeting `gate.approval`, or `wake_agent`) with evidence
  and `idempotency_key`. It never mutates the gate directly.
- Intent modules `approval.gates` and `agent.lifecycle` registered per fixture.

Acceptance:
- Below threshold: no intent is proposed, gate stays held.
- Above threshold + cooldown: exactly one advisory intent is proposed; its
  `authority` is `advisory` or `gated`, never `direct` (assert).
- `validate_document` rejects a hand-crafted `direct` escalation.

### Branch 6 — `20260630/labels-overrides-replay`

Closes the loop: labels and overrides feed the mixer; replay/shadow metadata
is exposed.

Deliverables:
- `labels.rs`: `record_label(event_id, label, source)` appends an
  `action`-class timeline event with provenance; `override_gate(decision)`
  flips `gate.held`. Labels persist into the candidate's training set.
- `lifecycle.rs`: candidate artifacts carry replay metrics, shadow metrics,
  promotion operator, timestamp, rollback pointer — projected into
  `DecisionEnvelope.explanation` (JSON). Promotion and rollback are `gated`
  intents; the runtime refuses to apply them without an approved
  `SurfaceIntent`.
- Replay: re-score a historical window range with the current mixer and emit
  comparison timeline events.

Acceptance:
- A labeled false-alarm reduces future confidence on a similar window after a
  `train_step`.
- Promotion without an approved intent returns an error; with one, it swaps
  `model.active_candidate` and records an immutable promotion record on the
  timeline.
- Shadow run records "what it would have done" without mutating the gate.

### Branch 7 — `20260630/conformance-and-docs`

Acceptance:
- Fixture round-trip test against the canonical
  `patchboard_attention_router.json` passes.
- README documents: authority boundary, what is deferred, how to run tests,
  and the relationship to `signal-space`, `lazily`, `agent-doc`, and
  `signal-space-mcp`.
- A `docs/authority.md` note explaining why promotion/rollback are gated and
  why the agent never holds direct authority — matching
  `signal-space-spec/docs/authority-boundaries.md`.

## Non-Goals (Deferred)

- Visual design/topology/live modes UI (browser or TUI).
- Connector marketplace (email/Stripe/GA4/etc.). Sources are stubbed.
- MLP, autoencoder, GBT, and model zoo beyond logistic.
- Mobile client and on-device personalization.
- Distributed multi-tenant cloud runtime.
- Automated topology mutation in production.
- Drift detection, online learning policy, and scheduled retrain cadence.
- The `signal-model-designer` skill and `patchboard-model-mcp` design layer
  (separate workstreams; the vision doc lists them but they are not Phase 9).

## Open Questions For You

1. **CI for a private repo.** Sibling CIs check out public repos. For
   `btakita/patchboard` (private), do you want (a) a PAT/bot secret in CI,
   (b) make the repo public now, or (c) keep CI green only locally + on
   push-with-token for now?
2. **Mixer dependency.** Logistic with raw `Vec<f64>` (zero deps) vs. pulling
   in `ndarray` now so the MLP branch is a drop-in. Recommend zero-dep for the
   spike.
3. **`signal-space-*` submodules.** They exist on disk but are not in
   `.gitmodules`. Should Branch 1 also register them, or is that separate
   housekeeping?
4. **Scope confirmation.** Is the 7-branch spike the right unit, or do you
   want it cut tighter (e.g. stop after Branch 4 — mixer proven, defer
   gate/agent/labels)?

## Near-Term Backlog (post-approval)

1. Create Branch 1 and push the skeleton to `btakita/patchboard`.
2. Register `src/patchboard` in agent-loop `.gitmodules`.
3. Land Branches 2-7 sequentially behind `cargo test` at each step.
4. Once stable, wire the runtime into `signal-space-mcp` as a product adapter
   example (control-plane only; realtime stays on lazily mirror channels).
