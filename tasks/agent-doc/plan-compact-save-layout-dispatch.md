# Compact settlement and editor layout dispatch

Invariant: editor-visible retained writes continue to an exact native-save receipt before settlement and commit; a fresh editor route runs only after its desired layout generation physically converges.

Policy owner: controller retained-transition Computed and controller pane-layout graph.

Transition table: missing delivery activates observation; unsettled delivery requests native save; exact or semantically rebased visible target also requests native save until the pending write settles; divergent content remains a conflict; settled writes enable the existing compact continuation. Layout absent, needs-effect, applying, retry, converged, superseded, and operator-owned states retain distinct outcomes.

Evidence inputs: generation-tagged CRDT visible content, pending intent and compact continuation, native save/disk receipts, desired layout, effect receipts, physical pane observations.

Reactive topology: retained intent + delivery Sources → retained-transition Computed → latest-durable save Effect → exact save and authority Sources → settlement Effect → compact continuation. Layout desired + observation + effect receipts → projection Computed → layout worker Effect → receipts.

Imperative extraction audit: no manual retry or disk write is introduced for compact settlement; the existing latest-durable worker owns save ordering. Layout sink installation previously republished an identical desired value and missed effect activation. Worker readiness now has its own Source in the existing ProcessScope; the retained Effect observes it. Await-result formatting is a one-shot pure adapter over the computed projection, and fences every phase by requested generation.

Allowed edit surfaces: controller graph/state transitions, controller RPC layout await and regression tests; relevant command specs.

Verification: retained-transition table and native-save regression, production layout graph regression, full make check, install, CI status.

Out of scope: replaying or force-writing live session documents; changes to unrelated editor recovery work already in progress.

Evidence: the native-save regression covers delivery version 36 pending → 37 visible → write settlement → compact continuation. The layout activation regression failed before the readiness Source change and passed afterward, including two fresh identical route intents. A phase table covers every await outcome and supersession. The original backend compact recovered independently through continuation settlement and commit `fb5d356`, retaining the 20260909-043501 archive reference.

Timeout extension: the IDE's five-minute failure is Compact Exchange on `dev.md`; the continuation was durable before the handler reloaded the complete project ledger. Invariant: a controller-local fact reaches its existing document graph before publication returns, without self-IPC or project rehydration. Policy owner: shared typed state-event ingress. Transition rows: new event appends and applies once; duplicate is quiet; missing local runtime fails closed. Extend the edit surface to local state publication and compact RPC return. Regression: publish a compact continuation in a production-style request-thread context and assert immediate live visibility with no refresh.

Full verification after all three fixes and the 0.35.349 version projection: `make check` passed, artifact `src/agent-doc/.tsift/artifacts/tart-c528a8a1f7.test.log`. This includes the local continuation ingress regression and all native-peer/editor parity, simulator, and formal gates.

Installed 0.35.349 with `JAVA_HOME=/home/brian/.jdks/ms-21.0.10 make install-full` (artifact `tart-cfa527e197`), then recycled the affected project controller. Live retained startup converged at attempt 1. Fresh SDK layout generation 2 converged with `%48`; project document generations 5 and 6 converged with `%42`, including default window resolution. A passive no-autostart cross-project probe refused unresolved bindings; normal provisioning-only controller ingress proved convergence without dispatching prompts. The infra compact continuation settled and committed as `b0fee52`. The dev compact is still retained: the controller reports `editor_attached_model_missing`, including after its supported recovery request. This requires an editor replica reconnect; do not rewrite or repeat compact.
