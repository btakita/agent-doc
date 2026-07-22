---
name: project_live_editor_reactive_family_initiative
description: live_editor liveness → lazily reactive backbone; S1–S5 DONE (reactive editor-attached gate + cross-platform crash watcher + both plugins on a real lazily StateGraphMirror); S6 re-scoped as sidecar-retirement-onto-lazily-sync (own plan)
metadata: 
  node_type: memory
  type: project
  originSessionId: c5e474d8-123b-4ee3-b563-491b0275c542
---

Started 2026-07-11 from a tmux-pane-no-longer-syncs-with-editor-selection report.

**Root cause of the symptom:** agent-doc `live_editor` liveness is imperative +
polled — two unreconciled representations. Path A: `RelayHub.Member.live: bool` +
`live_count()` pull-scan (`agent-doc-document-realtime/src/crdt_relay.rs`), consumed by
the runtime resolver in `agent-doc-crdt-relay-io/lib.rs` (`crdt_relay_no_live_editors`
→ `authority=disk`), commit `live_buffer_guard`, `plugin-owner::live_editor_endpoint_attached`.
Path B (already reactive, NOT read by resolver): `crdt_authority.rs::authority_for_projection`
over the lazily state-backbone `Transport` projection. A stale editor lease flips
`live=false`, `live_count()` returns phantom 0, nothing propagates → pane/selection
sync dies while the plugin is still alive (CrdtReplicaForwarder still polling).

**Operator directive:** make the entire live_editor pipeline reactive on the lazily
backbone (full pipeline + push to consumers). Then: can RelayHub BE a lazily
`ReactiveFamily`? **No as-is** — `ReactiveFamily`/`CellFamily` are `Rc`-based `!Send`
(lazily-rs `reactive_family.rs`: `inner: Rc<FamilyInner>`); `RelayHub` lives in
`static Mutex<HashMap<String,RelayHub>>` so must be `Send`. Operator chose to add the
missing primitive to lazily.

**Scope (operator-set):** add `ThreadSafeReactiveFamily` AND `AsyncReactiveFamily` to
lazily-rs, + lazily-spec conformance, + lazily-formal (Lean) proofs, + features/coverage
table. Then migrate agent-doc liveness onto `ThreadSafeReactiveFamily<u64, Liveness>`.

**Status — lazily side COMPLETE (all pushed):**
- ✅ `ThreadSafeReactiveFamily` (lazily-rs 8978f49) — `Arc<Mutex>`, `ThreadSafeFamilyHandle`
  over ThreadSafeContext; ts cell/slot aliases; 9 tests (feature `thread-safe`).
- ✅ `AsyncReactiveFamily` (lazily-rs 409ad51) — `AsyncFamilyHandle` over AsyncContext;
  uniform sync `Fn(&K)->V` factory (slot wraps ready future); `observe -> Option<V>`
  (eventual); drive via `get_async`; 6 tokio tests (feature `async`).
- ✅ lazily-formal (097520a) — Materialization.lean confluence (`materialize_present_comm`/
  `_observe_comm`); AsyncMaterialization.lean eventual transparency (`eventual_transparency`/
  `async_resolved_matches_sync`). Full lake build green.
- ✅ lazily-spec (bfde220) — 2 coverage rows (Rust ✅, others — one-commit-per-binding);
  cell-model.md § "Execution-context flavors" (context = 3rd axis ⊥ kind/mode).
- ✅ rs conformance (723b7a0) — ts + async replay of the same materialization fixtures.
- Note: non-Rust sibling READMEs reverted (render on their binding's impl); full `make check`
  not run (validated: default build, ts 102 + async 124 suites, both conformance, clippy, full
  lazily-formal build).

**Released 2026-07-11:** lazily-rs **v0.29.0** (crates.io + GitHub Release), lazily-formal
**v0.26.0** (GitHub Release), lazily-spec **v0.25.0** (GitHub Release + Pages auto-deploy).
crates.io token: `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`; publish
manual `cargo publish` (no tag-triggered OIDC). `make benchmark-check` is a PRE-EXISTING
baseline failure (missing p50/p95 latency rows on `thread_safe_graph_propagation`, fails on
clean HEAD too) — NOT a CI gate (ci.yml runs fmt+clippy --all-features+test matrix, not
benchmark-check).

**agent-doc migration — COMPLETE (S1–S4 + S4b), all pushed 2026-07-11:**
- ✅ PREREQUISITE: lazily 0.21.6→0.29.0 across 12 crates (0a66491b), NON-BREAKING.
- ✅ S1 (reactive RelayHub liveness core, 824a2632), S2 (reactive editor open-docs registry),
  S2b+S3 (resolver/visible-write route through reactive open-docs authority, 55d91244),
  S4 (lazily `editor_open_docs` is the authority; durable lease is cold-miss backup only,
  d5d346e6).
- ✅ **S4b (45fbbb3c, superproject d014b09f0):** the editor-attached gate `authority_for_file`
  (~30 CRDT-op sites) now reads pure reactive lazily state; the plugin-owner lease is consulted
  only on a cold miss. New `agent-doc-document-realtime/src/editor_attach.rs` (reactive `alive:
  CellFamily<pid,bool>` + `registered: CellFamily<(doc,pid),bool>` + derived `is_attached`,
  injectable `ProcessExitWatcher` seam, SimWorld tests). Crash signal = a **cross-platform**
  (Linux/macOS/Windows, per operator) OS process-exit watcher
  `agent-doc-controller-io/src/process_exit_watcher.rs` — a portable liveness poller off the hot
  path (`kill(pid,0)` Unix; `OpenProcess`/`WaitForSingleObject` Windows via windows-sys cfg dep),
  installed in `serve_with_options`. CLI-safety: no watcher → never trusts cache → lease-fresh
  (pre-S4b behavior; that's why `make check` sees no change — no watcher installed in tests).
  pidfd/kqueue *event* primitives remain a latency optimization behind the same seam. make check
  7427 passed. **NOT released** (STOP-releases directive holds; local install-full only).
- ✅ **S5 (agent-doc f46e4efa, lazily-js aaf6da1, superproject 6afec1e40):** both editor plugins now
  delegate the snapshot/delta state graph to a REAL lazily reactive `StateGraphMirror` (not a hand
  fold). `#lzpkgwire` was TWO version gaps (JVM21 bytecode + Kotlin2 metadata), not a shaded-artifact
  problem. **JB:** bumped plugin toolchain to Kotlin 2.0 / JVM 21 / IntelliJ **2024.2+** (sinceBuild
  242, drops 2024.1); StateGraphMirror.kt = thin adapter → `io.github.lazily.StateGraphMirror` (Gson
  parses wire → constructs lazily-kt public WireSnapshot/WireDelta → applySnapshot/applyDelta;
  decodeSubscribe stays internal). Wired via conditional Gradle **composite build** (includeBuild
  sibling lazily-kt if present, else mavenLocal/GH-Packages `io.github.lazily:lazily:0.19.0`).
  **Exclude ALL platform-provided transitives** — kotlinx-coroutines MUST NOT bundle (breaks IDE
  dispatcher), + kotlin-stdlib/serialization/annotations/jna — so only lazily-kt.jar ships (zip 1.4M).
  Built `agent-doc-jetbrains-0.2.234.zip` (install-from-disk = operator step, no CLI for running IDE).
  **VSCode:** lazily-js had the reactive PRIMITIVES but NO mirror class → ported `StateGraphMirror`
  INTO lazily-js (`src/state-graph-mirror.js`, real reactive graph on Context + memoized summary slot;
  export `@lazily-hub/lazily-js/state-graph-mirror`); extension delegates to it, **esbuild-bundled** so
  the vsix inlines the ESM lazily-js (self-contained). Installed `agent-doc-0.2.42.vsix` via `codium
  --install-extension`. MirrorTurnProjection stays plugin-side (no lazily equivalent). JB 32/32 +
  extension 149/149 + lazily-js mirror 7/7 green. Conformance pins kept as cross-language drift catches.
- **[operator-verify] live editor eyeball** (both S4b + S5): S4b crash→disk demote; S5 dispatch-ready/
  busy indicators still derive correctly from the delegated reactive mirror in a live IDE / VSCodium.
- lazily-js NOT npm-published (E404 OIDC) — fine, extension bundles source. lazily-kt 0.19.0 in
  mavenLocal + composite; publish to GH Packages if agent-doc CI ever builds the plugin standalone.
- **S6 RE-SCOPED (2026-07-11, 21d7fdec)** — was "reject bespoke wire"; S5 killed its premises
  (lazily `Delta` is ALREADY `{base_epoch,epoch,ops:Vec<DeltaOp>}` = epoch-batched set, NOT
  single-step; toolchain/ESM blockers gone; plugins now host a real reactive graph). New plan
  **`tasks/agent-doc/plan-sidecar-retirement-lazily-sync.md`** = retire the filesystem sidecars
  (plugin-owner lease + `.agent-doc/live-buffer/`) onto lazily reliable sync, killing the
  stale/phantom/divergent disk-read race class ([[plan-sidecar-authority-hot-path]]). Key facts:
  lazily Bridge is fire-and-forget (NO retry/outbox/ack; `ResyncRequired` unwired/zero-callers; no
  per-doc channel; webrtc-gated); CrdtSync plane IS idempotent+frontier-resumable but caller-driven
  (no periodic loop); agent-doc epoch = cumulative accepted-event count (a delta may span epochs);
  agent-doc's current `state_subscribe(last_epoch)` pull+SQLite is ALREADY retry-safe (re-poll from
  cursor). Design = **mechanism→lazily, policy→injected**: put ResyncCoordinator + DurableOutbox
  trait + SyncDriver skeleton + OR-set/LWW liveness INTO lazily (conformance-pinned rs/js/kt);
  agent-doc injects SQLite store + UDS transport + retry cadence. Operator-set phasing:
  **lazily-spec + lazily-formal FIRST → lazily-rs/kt/js → S6 agent-doc integration** (UDS carrier;
  keep pull+epoch+SQLite for controller→consumer; move plugin→controller open-set/lease push onto the
  CrdtSync plane; dual-run→parity→delete sidecar writers/reapers). S7 decided (keep content-CRDT vs
  state-projection split). **plan-live-editor-reactive-backbone.md COMPLETE (S1–S5); S6 now its own
  active plan.**
- [operator-verify] live editor-crash eyeball: prove a real editor crash (no deregister) demotes
  `authority_for_file` to disk within the ~500ms poll window while the controller stays healthy.
- Immediate pane-sync recovery: DIAGNOSED — do NOT blunt-force-recover a live session; lease
  self-heals on next editor edit; durable fix = this now-shipped migration.

Plan doc: `src/agent-doc/tasks/agent-doc/plan-live-editor-reactive-backbone.md`.
Immediate live-session recovery (heal stale lease so live_count→1, restore pane sync)
is a separate, still-open thread. [[reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery]]

**Sidecar-retirement Phase 3 progress (2026-07-12):** Phase 0/1/2 were already done (spec+formal+
rs/kt/js reliable-sync). This session landed:
- **lazily-rs v0.32.0 RELEASED to crates.io** (tag v0.32.0) — ships `SyncDriver` (the reliable-sync
  loop; was unreleased HEAD past v0.31.0). Additive over 0.31. Publish = manual `cargo publish`,
  `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`.
- **lazily-spec** (pushed) — first-class `IpcSink`/`IpcSource` **transport-seam contract** in
  protocol.md § SyncDriver (send-one / recv-poll `Ok(None)` / recv-`Err`=reconnect / sink-fail=
  retain-and-stall) + **backpressure contract** (host policy: unbounded enqueue + at-least-once outbox
  by design; bound via stall/retained signals + coalescing + spill-to-disk outbox) + coverage row.
  **CORRECTION:** lazily-**kt AND js already ship a full `SyncDriver` + transport seam** from their
  Phase 2 ports (kt `ReliableSync.kt` `class SyncDriver` since 67c980f; js `index.js`
  `export class SyncDriver`) — my initial "rs-only SyncDriver" coverage mark + spec commit text were
  WRONG; correct row is **rs/kt/js ✅** (operator corrected coverage.json; only py/dart/zig/go/cpp lack
  it). Decided **NOT** to formalize seam in lazily-formal (I/O trait has no algebraic content;
  invariants proven over frame sequences). Constraint semantics: **caret** `0.32.0` not `>=0.32.0`
  (0.x minors can break).
- **agent-doc** (pushed, main 212ab5ee) — all 14 lazily crates 0.31→0.32 (uniform: `^0.31.0` excludes
  0.32). **Phase 3A** `agent-doc-reliable-sync-io` crate = UDS carrier (`IpcSink`/`IpcSource` over the
  existing socket; msgpack frame in a `type=reliable_sync` NDJSON envelope). **Phase 3C receiver core**
  `liveness::LivenessProjection` = folds `LivenessOp` into lazily `OrSet`+`WireLwwRegister`, derives
  `open_docs`(#lbreap-scan replacement)/`live_docs`(whole-editor-death cascade)/`pid_alive`; CrdtSync
  carriage (encode/decode_liveness_frame); full push-loop SimWorld proving at-least-once across
  disconnect/reconnect through the REAL SyncDriver; `dual_run_enabled()` env
  `AGENT_DOC_RELIABLE_SYNC_DUAL_RUN` (default OFF). 19 crate tests. 3C store `SqliteOutbox` was already
  done.
- **tsift** (committed) — its 5 lazily crates bumped 0.21.6→0.32.0 (caret), separate repo.
- **Gotcha:** bumping the **VSCode plugin version** (`editors/vscode/package.json`) requires updating the
  hardcoded `"version": "0.2.x"` assertion in `tests/full_content_static_guard.rs`
  (`vscode_manifest_exposes_jetbrains_parity_commands`) or `make check`/CI fails. (JB version is NOT
  pinned in a test.) Bit me on the 0.2.43 bump; fixed 1daed05d.
- **agent-doc controller receive wiring DONE (38af5456)** — `plane::ControllerLivenessPlane` +
  `rpc.rs` `"reliable_sync"` dispatch arm → `handle_reliable_sync` (global `CONTROLLER_LIVENESS_PLANE`
  `LazyLock<Mutex>`, gated `dual_run_enabled()` OFF→ack 0) + dual-run parity SimWorld (`SidecarOpenSetModel`
  oracle; recycle=outbox-replay rebuild). Flaky-test note: `agent-doc-controller-io` lib test
  `detached_controller_exits_when_temp_project_root_is_removed` (rpc.rs) is **flaky under parallel load**
  (spawns a real detached process; passes in isolation, run-to-run failure count varies) — pre-existing,
  NOT a reliable-sync regression; don't chase it.
- **3C controller-receive + plugin-push Rust/FFI ALL DONE (this session, agent-doc main 793330d5):**
  controller-receive plane + `reliable_sync` RPC arm (38af5456, **CI green**); plugin-push core
  `push::LivenessPushEndpoint` (9ff636bf); FFI (793330d5) — controller-io `RpcLivenessPushTransport` +
  `push_reliable_sync_liveness()` over `request_controller`; `src/ffi.rs` C-ABI
  **`agent_doc_reliable_sync_liveness_enqueue`/`_flush`** (exported `T` in libagent_doc.so; per-`(root,doc)`
  `LivenessPushEndpoint` backed by durable `SqliteOutbox` at `.agent-doc/reliable_sync_outbox.db`,
  `resuming()` recycle-safe epoch resume). Full plugin→controller push loop proven in SimWorld.
- **Plugin emission DONE (design B, operator-chosen; agent-doc main 4bbd3b3a):** both editors host a
  real lazily liveness graph (`OrSet`) → FFI push. **JB** v0.2.235 (`buildPlugin` green):
  `ReliableSyncLivenessGraph.kt` (lazily-kt OrSet) + `ReliableSyncLivenessListener.kt`
  (`FileEditorManagerListener`) + `NativeLib.kt` JNA decls + `plugin.xml`. **VSCode** v0.2.43 (typecheck+
  esbuild+vsix green): `reliableSyncLiveness.ts` (lazily-js OrSet) + `native.ts` koffi wrappers +
  `extension.ts` register. Controller S4b watcher injects `Alive{false}` (`record_reliable_sync_editor_exit`
  in `process_exit_watcher.rs`). FFI enqueue gated on `dual_run_enabled()` → safe no-op by default.
- **REMAINING (all `[operator-verify]` + STOP-releases blocks the agent-doc release):** **switch
  `editor_open_docs`/`editor_attach`/`#6b5h` lease to READ `LivenessProjection`** (hot-path authority
  flip); ~~**3B**~~ **DONE** (see below); **cutover** (turn dual-run ON → confirm parity live in a real
  IDE/VSCodium → switch hot path → delete sidecar writers/reapers). Live eyeball = both editors emit +
  derived open-set matches sidecar + a real crash cascades to not-live.
- **3B clean-split wire flip — DONE 2026-07-12 (atomic no-fallback cutover).** Both `state_subscribe`
  emitters now speak the canonical native lazily wire (externally-tagged `IpcMessage` Snapshot/Delta):
  controller `rpc.rs::handle_state_subscribe` AND — the handoff omitted this, but both producers had to
  flip together — `src/ffi.rs::agent_doc_state_subscribe` (the plugins' direct-FFI subscribe path). Both
  route through `agent_doc_state_wire::lazily_convert::wire_subscribe_to_ipc_message` (`WireSubscribe`→
  native; `build_delta` still emits the internal `WireDelta`, the bridge converts it). Plugins fold it
  through a **generic** lazily `GraphView` and layer their own domain `AgentDocProjection`
  (route/transport/proof) + `AgentDocTurnProjection` (idle/awaiting_response/persisting): JB
  `StateProjectionBridge.kt`→GraphView (plugin-local base64 `StateGraphMirror.kt` DELETED; vocab+turn moved
  into `AgentDocProjection.kt`); VSCode `stateMirror.ts`→GraphView + `applyIpcMessageToView`
  (`IpcMessage.fromWire`), `native.ts`/`extension.ts` repointed (dropped `initStateMirror`). Verified: rust
  `make check` 7468 green; JB `./gradlew test` green (v0.2.237 zips); VSCode tsc + 13 tests green (v0.2.45
  vsix). **Follow-ups:** (1) `[operator-verify]` live editor eyeball (route/transport/proof + turn render;
  edit round-trips); (2) ~~retire lazily-library StateGraphMirror~~ **DONE** (see below); (3) release gated by
  STOP-releases (plan step 7). Plan: `plan-sidecar-retirement-lazily-sync.md` § 4-5.
- **lazily-library mirror retirement — DONE 2026-07-12 (operator-authorized).** lazily-kt **f669eb7** deleted
  `StateGraphMirror.kt` (StateGraphMirror + `WireDelta`/`WireSnapshot`/`WireDeltaOp`/`WireNodeSnapshot`/
  `WireEdgeSnapshot`/`WireSubscribe` + `MirrorProjectionSummary` + `MirrorNode`) and its dead consumer
  `StateProjectionClient.kt` (+ `decodeSubscribe`) + 2 tests; lazily-js **c2698ca** deleted
  `state-graph-mirror.{js,d.ts}` + test + the `./state-graph-mirror` export/build entry. Kept the native
  `AgentDocStateConformanceTest.kt` (IpcMessage/GraphView replay — only a stale comment mentioned WireDelta)
  and js `state-projection.js` (FFI cold-projection consumer, mirror-free). lazily-kt `make check` green; js
  build + **343 tests** green. **Superproject lazily gitlinks intentionally NOT bumped** (coordinated with the
  pending ReactiveMap-migration bump; the agent-doc plugin sees the retirement via its source composite build,
  not the gitlink). **Residual (out of scope):** lazily-kt `LazilyFFI.kt` is now orphaned (dead
  agent-doc-welded `agent_doc_*` FFI interface — later removal candidate); kt/js cold-consumer asymmetry (js
  keeps `state-projection.js`, kt dropped its dead client). Also from this session: close-gate liveness fix
  (agent-doc 0df51f4c) + a cycle_ack CI-flake fix (43568953).
- **Phase-3C dual-run parity findings (2026-07-12, from a live `reliable-sync-status` read).** Two
  distinct things surfaced; only one was a bug:
  1. **`#lzsync-close-no-disk-regate` — FIXED (agent-doc 0df51f4c; JB v0.2.238, VSCode v0.2.46).** Both
     editor liveness listeners gated the **Close** op via `agent_doc_is_session_document`, which reads the
     file from **disk**. A session doc momentarily unreadable at close (deleted/renamed/mid-git-checkout/
     teardown) failed that gate → the compensating `Close` op was dropped → the plane's `OrSet` kept the doc
     "present" forever with **no reaper** (unlike the sidecar `#lbreap`). Fix: Close is gated only by
     `graph.close()` returning non-null (this editor actually opened it); Open stays disk-gated. Shadow
     dual-run path only; not installed (would recycle Brian's live session — see
     [[feedback_install_full_recycle_disrupts_live_ide_session]]).
  2. **Parity oracle is NOT submodule-aware (open question for cutover, not a bug).** The plane keys liveness
     by the **IDE project root** (JB `project.basePath` = the agent-loop superproject), but a session doc
     nested in a **submodule that has its own `.agent-doc/`** (e.g. `src/boost-client/tasks/
     monsterrodholders.md`) writes its live-buffer sidecar under the **submodule** root. So the agent-loop-root
     `reliable-sync-status` oracle shows such a doc as plane-only (`<no sidecar to resolve path>`) even though
     it is genuinely open — the plane is correct; the two open-sets are just scoped to different roots.
     **Cutover readiness must reconcile root scoping** (either the plane keys by nearest-`.agent-doc` ancestor
     like the sidecar, or the oracle unions submodule live-buffers) before flipping the plane to authority,
     or submodule docs will parity-fail forever. Verified via hashing the plane paths (sha256 of canonical
     path = `document_id_for_path`) against the sidecar/registry sets.
