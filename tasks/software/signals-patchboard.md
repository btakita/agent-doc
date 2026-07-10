---
agent_doc_session: e0ce0be3-c996-483f-880a-e5fc38cd998e
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: Signals Patchboard full implementation plan (Slint) — fable-5

> **Queue prompt:** Read tasks/software/realtime-signals-patchboard.md. We will use Slint https://slint.dev/ for UI instead of Flutter. Please plan a full implementation of Signals Patchboard. Plan an engaging UI using Slint...where signals can be observed. We should also support state charts (State) in the UI. We should support deterministic logic + multi-layer perceptrons. Use signal-space as the domain interface. Build out signal-space-spec + signal-space-rs as needed. The goal is to integrate with services and other sources of information to bring life to the Signals Patchboard.

> **Queue prompt:** It should feel like a music Patchboard but for signals information + logic for people to use. There should be I/O as well, so other systems can be notified in realtime. Update realtime-signals-patchboard.md.

Wrote the full implementation plan at **tasks/software/plan-signals-patchboard-full.md** and updated **realtime-signals-patchboard.md** (both committed, ff244d0a8).

**Plan shape** — 9 phases on the existing substrate (`lazily-rs -> signal-space-rs -> patchboard -> patchboard-app`):

1. **#spb1 Spec 0.3.0** — typed ports/jacks (`PortSpec`), live-cable `StreamTelemetry`, and `IoBinding` (ingress/egress transport descriptors) in signal-space-spec + signal-space-rs. Backward compatible with 0.2.0; new `patchboard_io_rack` fixture.
2. **#spb2 MLP mixer** — `MlpMixer` (ndarray, 1–2 hidden layers, CPU-only) beside `LogisticMixer` under the existing `Mixer` trait, plus a `Trainer` feeding the unchanged replay/shadow/promote lifecycle. Deterministic nodes and MLPs share the `DecisionEnvelope`.
3. **#spb3 Realtime I/O host** — tokio `patchboard-io`: webhook/WebSocket/file-tail/timer/stdin ingress; gate-guarded webhook POST, WebSocket Snapshot/Delta broadcast (the "notify other systems in realtime" spine), desktop notify, allowlisted exec, and advisory agent triggers as egress. Connector failures become freshness/missing-data signals.
4. **#spb4 Slint app skeleton** — `patchboard-app` (patchboard repo becomes a workspace). Slint links the Rust core in-process (no FFI). Read-only rack render, live cable telemetry via lazily deltas + `invoke_from_event_loop`, scope panel, 3-pane layout (rack | timeline | inspector).
5. **#spb5 Music-patchboard feel** — jack-to-jack cable dragging with live dtype checking, module palette, cable slack/pulse/color, multi-trace scope, VU meters. Every edit is a `SurfaceIntent` through revalidate-and-revert; rejected patches visibly snap back.
6. **#spb6 State chart UI** — spec 0.2.0 `state_chart` (mirrors lazily `StateMachine`) rendered as an inspector tab: current state highlighted, transitions animated, gated fire-event buttons (events double as intent types), timeline scrub replays chart state.
7. **#spb7 Training/labeling UI** — timeline labeling, semantic-knobs-first train dialog, replay compare, shadow mode side-by-side, gated promote/rollback writing immutable `CandidateRecord`s.
8. **#spb8 Agent-supervisor wedge** — dogfood rack over git/CI/test/agent-doc/tsift/corky signals producing continue/stop/ask-human decisions in shadow mode; exposed through patchboard-mcp.
9. **#spb9 Packaging + demo** — release builds, wasm browser observer consuming the delta stream, recipe gallery, docs.

Critical path is 1 → 4 → 5 → 7; the main risk is the custom Slint node canvas (no built-in node editor — front-loaded spike in Phase 4/5). Open questions in the plan: Slint license fit before packaging, patchboard-io as module vs workspace sibling, wasm observer via Slint-wasm vs signal-space-js.

**Vision doc updates**: modular-synth patchbay is now the leading metaphor ("music patchboard for signals, information, and logic"), the runtime table swaps Flutter for Slint (lazily-dart demoted to optional mobile companion), and a new **Realtime I/O** section specifies ingress/egress connectors with the gate + authority + timeline + no-secrets-in-document rules.

Backlog now holds #spb1–#spb9 in priority order; each references the plan file. Top of backlog: **#spb1**.
<!-- agent:boundary:981284fc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [x] [#spb1] Spec 0.3.0: typed ports/jacks, stream telemetry, IoBinding in signal-space-spec + signal-space-rs — tasks/software/plan-signals-patchboard-full.md Phase 1
- [x] [#spb2] MlpMixer + Trainer (ndarray MLP beside LogisticMixer, replay/shadow/promote compatible) in patchboard — plan Phase 2
- [x] [#spb3] patchboard-io realtime connector host: webhook/websocket/file-tail/timer ingress, gated webhook/WS-delta/notify/exec/agent egress — plan Phase 3
- [x] [#spb4] patchboard-app Slint skeleton: workspace split, load+validate document, live rack render, scope panel, 3-pane layout — plan Phase 4 *(eval 2026-07-10: spike only — no patchboard-io/tokio wiring, 60ms full-reprojection polling instead of delta-driven invoke_from_event_loop; reopened as #spb10)*
- [x] [#spb5] Patch canvas interactions: jack-to-jack cabling, palette, intent-validated edits, undo, music-patchboard aesthetics — plan Phase 5 *(eval: click-click patching not drag, no unpatch/remove_edge, no pulse/snap-back animation, no VU meters/zoom/keyboard, feature palette button missing; reopened as #spb11 + #spb12)*
- [x] [#spb6] State chart UI: statechart inspector tab, live current-state, gated fire-event intents, timeline scrub replay — plan Phase 6 *(eval: state list + fire-event buttons only — no transition graph/animation, no timeline scrub; reopened as #spb13)*
- [x] [#spb7] Training/labeling/lifecycle UI: timeline labels, train candidate, replay compare, shadow, gated promote/rollback — plan Phase 7 *(eval: 2 of 6 LabelKinds, hardcoded MlpConfig::tiny + rollback target "mlp_v4", no train dialog/knobs, no FP/FN plot, no shadow side-by-side, approval toggle not dialog; reopened as #spb14)*
- [x] [#spb8] Agent-supervisor wedge: git/CI/test/agent-doc/tsift/corky ingress adapters + supervisor rack recipe in shadow mode — plan Phase 8 *(eval: rack + MCP exposure solid; only git/CI adapter real — digest-runner/tsift/corky missing, agent-doc parser missing, scorer defaults to logistic proxy; follow-up #spb16)*
- [x] [#spb9] Packaging + docs + demo: release builds, wasm browser observer over delta stream, recipe gallery — plan Phase 9 *(eval: docs/recipes/authority good; no release automation, Linux .desktop only, fmt+test-only CI; follow-up #spb17)*
- [ ] [#spb10] Wire patchboard-io into patchboard-app: tokio host thread beside the UI, ingress/egress from the loaded document's IoBindings, runtime updates via invoke_from_event_loop + Weak<AppWindow> (replace the 60ms full-reprojection Timer with delta-driven refresh), scope traces fed by real StreamTelemetry instead of synthetic sine waves, load a document from CLI arg/file dialog — plan Phase 4 acceptance ("live-updating cables, 10+ streams"); everything below depends on this
- [ ] [#spb11] Cable observability (music-patchboard core): render CableVm label + last-value preview on/near cables, per-cable sparkline, freshness dot, rate readout; make cables clickable (TouchArea on Path or hit-test) → click opens/pins the scope; line traces instead of dot raster — plan Phase 4/5, vision "every cable is observable in the moment"
- [ ] [#spb12] Patching feel: drag-based out-jack→in-jack cabling with live preview cable + dtype-aware highlight, rejected patch snap-back animation (keep toast reason), remove_edge/unpatch gesture, animated pulse on signal fire, distance-aware cable slack, VU meters on output nodes, wheel/pinch zoom, keyboard (ctrl+Z undo, arrow-nudge), add missing `feature` family palette button — plan Phase 5 acceptance
- [ ] [#spb13] Inspector + state chart completion: inspector renders ports, state fields, decision envelope, allowed intents; state-chart transitions drawn as a graph (nodes + event-labeled arrows, current highlighted, transition animation) instead of a flat list; timeline scrub that replays chart/graph state — plan Phases 4/6
- [ ] [#spb14] Training/lifecycle UX: train-candidate dialog (logistic|MLP choice, semantic knobs sensitive↔quiet / adapt↔stable / faster↔accurate, advanced drawer for layers/width/lr/batch), all 6 LabelKinds on selectable timeline events, real feature vectors (drop hardcoded ["f0","f1"] w/ zero second feature), FP/FN plotted on timeline, shadow side-by-side "would have done" column, gated approval dialog (not toggle), rollback target selection (drop hardcoded "mlp_v4"), uncertainty-sampling "label these next" queue — plan Phase 7 acceptance
- [ ] [#spb15] Core runtime gaps: true Delta frames in the patchboard-io WS stream (currently snapshot-only; #spbwasm assumes deltas), MLP explanation packet per plan (input×first-layer-weight attribution — current is input-independent weight magnitude — plus nearest-labeled-examples) — plan Phases 2/3
- [ ] [#spb16] Phase 8 adapter completion: digest-runner JSONL test-results adapter, tsift evidence-packet adapter, corky follow-up adapter, agent-doc cycle-state parser feeding queue_depth (currently caller-supplied), wire MlpMixer as the shipped continuation scorer, add dedicated patchboard.supervisor JSON fixture + parity test — plan Phase 8
- [ ] [#spb17] Packaging/CI hardening: release automation (cargo-dist or release.yml), macOS/Windows bundle metadata + rasterized icons, clippy in CI, connector-authoring how-to doc, fix stale signal-space-spec conformance/bindings.json pin (still expects rs spec-version 0.1.0) — plan Phase 9
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
- [ ] [#spbwasm] Wasm browser observer (deferred from #spb9): compile the read-only rack view to WebAssembly consuming the patchboard-io WebSocket Snapshot/Delta stream. Open decision: Slint-wasm vs signal-space-js; both reuse the same delta stream.
<!-- /agent:icebox -->
