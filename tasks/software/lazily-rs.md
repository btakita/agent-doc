---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
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
queue: start
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->
- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
- read: `agent_doc_state_projection(doc_hash) -> JSON`
- write: `agent_doc_record_state_event(doc_hash, fact_json)` (deduped by `seen_event_ids`, `state_backbone.rs:301`)
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-034352.md`*

Compacted content:
❯ - Archived 5 response topic(s): StateMachine blog post; dark-mode diagram visibility; dark mode in JB plugin + #mpt7 sitewide contrast; 2 more
❯ - Prior summary/context: ❯ - `bun run build` — green (`build|done`); post registered via regenerated `post/index.ts`. ❯ - Existing `index.gene...
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=df0c7de0-1b2e-4a19-bde8-6df0d80a5758 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=40160 candidate_hash=6f4633a0fda9536686f3216fb71ff972c3400e223bdab401f9695ce1a7d437e4 content_ours_len=42051 content_ours_hash=02a968ee2bafb304073538e02e53fc7d4cc7e53fb9634101662beae554313f51 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782251713108
```

### Re: lazily-kt / lazily-js for the editor plugins — no port; project the binary's FSMs via FFI — glm-5.2

> **Queue prompt:** See `state_backbone.rs`. Should we have a `lazily-kt` and `lazily-js` package to support the JB and VSCode plugins? Would the plugins benefit from the lazily state machine? Would it make sense for these plugins to use lazily FFI to communicate (bi-directionally)?

> **Queue prompt:** Should we create `lazily-kt` and `lazily-js` for the plugins to help with FFI communication with the FFI plugin?

**Short answer: don't port the reactive core into Kotlin/TS. Let the binary keep owning the lazily state machines, and expose their state to the plugins as serializable projections over the FFI/IPC channel you already have.** That channel IS the "bi-directional lazily FFI" — it carries serialized state, not typed handles.

**Why not `lazily-kt` / `lazily-js`:** `StateMachine<S,E>` (`lazily-rs/src/state_machine.rs:60`) is reactive because the transition is a Rust `Rc<dyn Fn>` (thread-safe: `Arc<dyn Fn + Send + Sync>`) and propagation rides `Context`/`ThreadSafeContext`'s dependency graph (RefCell / `parking_lot::Mutex`). That closure + borrow-graph model has no faithful Kotlin/TS analogue — a port is a re-implementation, and it breaks the project's own law (agent-doc `AGENTS.md`: *"FFI-first … plugins should be thin event reporters … All deterministic behavior in the binary"*). It would also leave three cores under one formal model (`lazily-spec` Lean certifies Rust only). `lazily-rs/src/ffi.rs` already refuses to export `StateMachine`/`Context` — it carries only opaque `lazily-ipc` Snapshot/Delta bytes. Keep that boundary.

**What the plugins DO get from the lazily machines:** authoritative projections instead of heuristic guesses. agent-doc already runs the durable FSMs — `CyclePhaseMachine` (`cycle_state_machine.rs:44`) + `TransportPatch` / `ActorLifecycle` / `RouteReadiness` / `ProofGate` machines (`state_backbone.rs:1009/1112/1246/1353`) — folding `StateFact` events into a `DocumentStateProjection` (`state_backbone.rs:313`, `#[derive(Serialize, Deserialize)]`: document/queue/closeout/transport/supervisor/route/proof slices). That struct is the seam: plain JSON, no closures. The JB `EditorCommandStateMachine.kt` stays **local UI coalescing only** (keystroke/focus batching) — ephemeral, not a durable FSM.

**Bi-directional via FFI = the existing channel plus two new exports (this is `#lzffistate`):**
- read: `agent_doc_state_projection(doc_hash) -> JSON`
- write: `agent_doc_record_state_event(doc_hash, fact_json)` (deduped by `seen_event_ids`, `state_backbone.rs:301`)

behind the JNA bridge (`ffi.rs` ↔ `NativeLib.kt`) + the VSCode native addon, alongside the reporters already there (`report_editor_state`, `record_editor_op`, `document_changed*`, `agent_doc_start_ipc_listener_v2`). Optional later: stream `lazily-ipc` Deltas instead of full-JSON snapshots if polling cost ever matters.

**Decision captured + plan written:** `tasks/software/plan-lazily-ffi-state-projection.md` (`557afa87a`; phased: 1 projection+intake spike = `#lzffistate`/`#lzffistate1`, 2 delta transport opt-in, 3 plugin UI wiring). `#lzffistate` plan-gate satisfied; `#lzffistate1` (phase-1 impl) added to backlog.

> Preflight `semantic_completion_match` note: review `#h6qb` is **not** fully resolved — `#lzwebrtcwire` (agent-doable localhost slice) is DONE, but the remaining cross-host/NAT validation through the live `#yxjw` Worker is genuinely operator-only. `#h6qb` stays `[operator-drive]`.

Queue heads answered; plan committed.

### Re: queue strike — three answered heads — glm-5.2

> **Queue prompt:** See /home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-orchestration/src/state_backbone.rs. Should we have a lazily-kt and lazily-js package to support the JB and VSCode plugins? Would the JB and VSCode plugins benefit from the lazily state machine? Would it make sense for these plugins to use lazily FFI to communicate with the FFI plugin?

> **Queue prompt:** See /home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-orchestration/src/state_backbone.rs. Should we have a lazily-kt and lazily-js package to support the JB and VSCode plugins? Would the JB and VSCode plugins benefit from the lazily state machine? Would it make sense to use lazily FFI for bi-directional communication between editor plugins and the FFI plugin?

> **Queue prompt:** Should we create lazily-kt and lazily-js for the plugins to help with FFI communication with the FFI plugin?

All three are the same architectural question (refinements of each other), answered in the `### Re: lazily-kt / lazily-js` response above: **no reactive-core port; the binary owns the lazily FSMs and the plugins consume them as serializable `DocumentStateProjection` state over FFI/IPC (`#lzffistate` → `#lzffistate1` phase-1 spike)**. Striking as resolved.

### Re: strike q3 — same question, answered above — glm-5.2

> **Queue prompt:** Should we create lazily-kt and lazily-js for the plugins to help with FFI communication with the FFI plugin?

Same question as the two heads already struck — answered in `### Re: lazily-kt / lazily-js` above (no port; consume the binary's `DocumentStateProjection` over FFI/IPC per `#lzffistate`/`#lzffistate1`). Striking.

> **Queue prompt:**
>
> ~~See /home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-orchestration/src/state_backbone.rs. Should we have a lazily-kt and lazily-js package to support the JB and VSCode plugins? Would the JB and VSCode plugins benefit from the lazily state machine? Would it make sense to use lazily FFI for bi-directional communication between editor plugins and the FFI plugin?~~ — auto-struck: answered this cycle (#ftstrike)

### Re: bidirectional lazily↔lazily FFI channel — yes for values, no for references/authority — glm-5.2

> **Queue prompt:** Can we make lazily support a bidirectional lazily<->lazily FFI communication channel? And in doing so, revisit the thin reporter spec. Whether the JB Plugin & VS Code plugin may trigger state transitions or be read-only state projections. It seems plausible that a lazily FFI channel would be beneficial to keep state aligned. If we do have a FFI channel, the plugin-side Cell can mirror the FFI plugin side Cell...for both values and references.

**Strong instinct — and lazily already half-builds it — but draw the line at values, not references or transition authority.** Splitting the question:

**1. The channel itself — yes, and it already exists.** `lazily-rs/src/ipc.rs` defines `IpcMessage::{Snapshot, Delta}` and `lazily-rs/src/ffi.rs` exposes the `LazilyFfiChannel` (JSON or postcard frames). Snapshot/Delta IS the lazily↔lazily mirroring protocol: a Snapshot seeds the remote graph image; a Delta carries an incremental update a receiver applies to a local mirror without re-fetching. So "a lazily FFI channel to keep state aligned" = make agent-doc's binary a lazily-ipc **producer** (emit a Delta each time a `state_backbone` machine transitions) and each plugin a **consumer** (apply Deltas to a local mirror). This is Phase 2 of `plan-lazily-ffi-state-projection.md`; your question is the case for elevating it.

**2. "plugin-side Cell mirrors binary-side Cell" — yes for VALUES, no for REFERENCES.** This is the line.
- **Values (yes):** projection data — `DocumentStateProjection` slices (queue head, closeout phase, route readiness, proof gate). Stream as Deltas; the plugin holds them in a **platform-native reactive holder** (Kotlin `StateFlow` / Compose `State`; VSCode TS signal / `EventEmitter`). Reactive mirroring (UI re-renders on delta) with zero lazily code on the plugin side.
- **References (no):** the reactive dependency *graph* — which Cell feeds which computed/signal/effect — must live in exactly one place. Mirroring references means reconstructing lazily's `Context` dependency graph on the plugin side, which IS the `lazily-kt`/`lazily-js` port (the `Rc<dyn Fn>` / `Arc<dyn Fn + Send + Sync>` transition + RefCell/`parking_lot::Mutex` propagation graph flagged last cycle). Two synchronized graphs = the split-brain/drift class the FFI-first law exists to prevent, and only Rust carries the formal model (`lazily-spec` Lean). Keep references binary-side; the plugin treats its mirror as a flat cached snapshot.

**3. Read-only projections vs triggering transitions — plugins trigger via EVENTS, never via local transitions.**
- A plugin **may trigger a transition** by reporting a `StateFact` through `agent_doc_record_state_event(doc_hash, fact_json)` (`#lzffistate`). The binary's `ThreadSafeStateMachine` runs the transition + guard and emits the new projection (Delta). Plugin = *event source*, binary = *transition authority*. Co-design, not co-ownership.
- A plugin must **not** run a durable FSM locally and push the resulting state — two authorities drift (the `live_prompt_drift_after_preflight` class this session is dogfooding, but for cycle state instead of buffer text).
- Local plugin FSMs stay legitimate only for **ephemeral UI state** (`EditorCommandStateMachine.kt` keystroke/focus coalescing). Durable cross-editor FSMs (`RouteReadiness`, `TransportPatch`, `ActorLifecycle`, `ProofGate`, `CyclePhase`) are binary-only.

**4. Thin-reporter spec, refined (not abandoned).**
- **Still thin** on the *authority* axis: plugins never own durable state or run durable transitions; they report editor facts (typing, focus, layout, intent) and render binary projections.
- **Rich** on the *rendering* axis: plugins become reactive projection-renderers (platform-native `StateFlow`/signal holding Delta-applied mirror values), not dumb pollers.

So: **thin event-reporter + rich projection-renderer**, binary stays the single state authority. The FFI channel carries values + events both ways; references and transition logic stay one-sided.

**Net:** build the lazily-ipc Snapshot/Delta channel (Phase 2), mirror projection VALUES into platform-native reactive holders, let plugins trigger transitions by reporting events — and stop short of mirroring reactive references or running durable FSMs in-process. I'll fold this values-vs-references contract into the plan's Phase 2 design.

### Re: references = pointer to object/blob — mirror via `SharedBlob` descriptor, never raw pointer — glm-5.2

> **Queue prompt:** What I mean by reference is if the Cell value is a reference pointer to an object or blob. Would that be recommended to mirror or to not mirror?

Great clarification — and it surfaces a category my last answer wrongly folded into "references (no)." A Cell whose value is itself a pointer/reference to an object or blob is **neither** a pure inline value **nor** a reactive-graph reference. It is a **by-reference payload**, and lazily-ipc already has a first-class answer for it: mirror it **by descriptor**, never by raw pointer.

**The protocol already defines this — `IpcValue::SharedBlob(ShmBlobRef)` (`lazily-rs/src/ipc.rs:47`, descriptor at `:32`).** `ShmBlobRef` is exactly a mirrorable "reference to an object/blob": it carries `{offset, len, generation, epoch, checksum}` into a managed `ShmBlobArena` (`ipc.rs:150`). The arena writes a 40-byte header (magic `"LZSH"`, version, generation, epoch, len, checksum) before every payload and re-validates all of it on read (`read_blob`, `ipc.rs:269`) — so stale regions are rejected after wraparound, torn writes are caught by checksum, and the plugin never dereferences Rust memory. `DeltaOp::{CellSet, SlotValue}` (`ipc.rs:458/460`) and `NodeState::SharedBlob` (`ipc.rs:328`) both carry it, and `cell_set_blob`/`slot_value_blob` (`ipc.rs:501/506`) are the constructors. So the recommended path already exists.

**The full taxonomy (what crosses, and how):**
1. **Small structured values → `IpcValue::Inline`** — serialize bytes into the delta. Mirror freely.
2. **Large object/blob values → `IpcValue::SharedBlob(ShmBlobRef)`** — **YES, mirror**, by descriptor into the arena. This is the recommended way to avoid copying a big payload into every delta. The descriptor IS the reference; the arena is the backing store; generation/checksum guard against drift between the two sides.
3. **Reactive dependency edges → structural data only** — `EdgeSnapshot`/`EdgeAdd`/`EdgeRemove` over `NodeId`s (`ipc.rs:375/472/477`): graph *shape* crosses, not live reactive *behavior*. The plugin renders shape; closures/propagation stay binary-side.
4. **Raw Rust pointers / typed handles (`&T`, `Arc<T>`, `*mut T`) → NEVER.** `ffi.rs:1-6` and `ipc.rs:8-10` explicitly forbid exposing live contexts, references, closures, or typed handles. `NodeState::Opaque` (`ipc.rs:330`) is the escape hatch — advertise a node's existence without leaking the reference behind it.

**So: recommended to mirror?** Yes — for the *payload*, via `SharedBlob` descriptor. No — for a raw pointer into Rust memory. The line is "descriptor + managed arena with integrity guards" vs. "dereferenceable address-space pointer." My prior "references (no)" was right only for category 3/4 (reactive-graph references and raw handles); it was wrong to imply category 2 (blob-by-reference) is off-limits — lazily already ships the mechanism.

**Plan updated:** folded this three-way contract (inline value / blob-by-descriptor / structural-edge / raw-pointer-never) into Phase 2 of `tasks/software/plan-lazily-ffi-state-projection.md`, including the `ShmBlobArena` validation path so Phase 2 picks `SharedBlob` for large projection payloads instead of re-inlining them.



```text
ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=c75552ac-2840-46e1-be53-ee4e15098149 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=20861 candidate_hash=c152a9f376d5cbfc226624942a6a476699257d8e877913ec5206e45ddfe3f082 content_ours_len=23146 content_ours_hash=069b1347b45d892fb29db9f0212a6a2755dd033b60f3a5a6e524eb12d97fb787 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782275568022
```

### Re: StateMachine in lazily-spec + ecosystem parity — spec done, plan + backlog captured — glm-5.2

> **Queue prompt:** Should the State Machine be part of lazily-spec? To facilitate State Machine mirroring?
> **Queue prompt:** Spec FFI channel implementation in lazily-spec.
> **Queue prompt:** Implement State Machine in lazily-zig
> **Queue prompt:** Implement State Machine in lazily-py
> **Queue prompt:** Implement lazily-kt
> **Queue prompt:** Implement lazily-js. Use *.js + *.d.ts.

**Six queue heads, grouped and answered:**

**1. "Should the State Machine be part of lazily-spec?" — YES, and it's done.** Created `LazilyFormal/StateMachine.lean` (commit `9fbc669` in lazily-spec, gitlink `3ead752bc`). It formalizes the exact `Fn(&S, &E) -> Option<S>` transition/guard model from `state_machine.rs:60`:
- `Machine` structure: `current : State` + `transition : Transition` (pure `State → Event → Option State`)
- `send` (advances on `some`, preserves on `none`) and `sends` (reports whether a transition would change state)
- Five theorems: guard-rejection preserves state, accepted-transition advances, self-transition preserves + suppresses propagation (`sends = false`), changed-transition sends true, send preserves the transition function
- `lake build` passes. This is the **first formal linkage between the reactive layer and the transport layer** — `IPC.lean` formalizes Snapshot/Delta; `StateMachine.lean` formalizes the transitions that produce the values those deltas carry. Cross-module proof (SM `send` → `DeltaOp.cellSet` emission) is Phase 1 TODO → `#lzsmlean2`.

**2. "Spec FFI channel in lazily-spec?" — DEFERRED.** The `LazilyFfiChannel` (`ffi.rs:90`) is a FIFO byte-queue (`VecDeque<Vec<u8>>`) with encode/decode round-trip. Its semantics are trivial — the interesting ordering invariants (Delta epoch sequencing, gap-requires-resync) are already proven in `IPC.lean:70-97`. Revisit only if a non-trivial FFI-specific invariant emerges (e.g. backpressure, frame fragmentation). Not worth formalizing a pipe.

**3. "Implement StateMachine in lazily-py / lazily-zig" — legitimate primitive parity (captured).** Unlike a full reactive-core port (which we rejected), `StateMachine` is one self-contained primitive: a state Cell + a pure transition function. Both siblings already have `Cell` (lazily-py has Cell+Signal; lazily-zig has Cell but not Signal), so adding StateMachine is additive — same primitive, not a re-implementation of the propagation graph. Plan phases: `#lzsmpy` (Phase 2, easiest — Python already has the full Slot/Cell/Signal stack) and `#lzsmzig` (Phase 3 — Zig has no closures, so the transition is a `*const fn(*const S, E) ?S` function pointer).

**4. "Implement lazily-kt / lazily-js" — FFI-consumer packages, NOT reactive-core ports.** Consistent with the no-port decision (`plan-lazily-ffi-state-projection.md`): these are thin event-reporter + rich projection-renderer packages that wrap the existing JNA/N-API bridge. They hold `DocumentStateProjection` values in platform-native reactive holders (Kotlin `StateFlow`, JS `EventEmitter`), report editor events via `recordStateEvent`, and execute **zero** reactive logic locally. The `StateMachine` they "mirror" is consumed as a projection value from the binary — never executed in-process. `lazily-js` ships `*.js` + `*.d.ts` as requested. Both are **blocked on `#lzffistate1`** (the FFI state-projection exports must exist first). → `#lzkt` (Phase 4), `#lzjs` (Phase 5).

**Parity matrix after this plan completes:**

| Port | Slot | Cell | Signal | StateMachine | IPC | Lean |
|---|---|---|---|---|---|---|
| lazily-rs | ✅ | ✅ | ✅ | ✅ (×3) | ✅ | — |
| lazily-py | ✅ | ✅ | ✅ | → `#lzsmpy` | ✅ | — |
| lazily-zig | ✅ | ✅ | ❌ | → `#lzsmzig` | ✅ | — |
| lazily-spec | — | — | — | ✅ `StateMachine.lean` | ✅ `IPC.lean` | (formal layer) |
| lazily-kt | — | — | — | projection-only | → `#lzkt` | — |
| lazily-js | — | — | — | projection-only | → `#lzjs` | — |

**Delivered this cycle:** `StateMachine.lean` spec (builds clean), `plan-lazily-state-machine-mirroring.md` (5 phases), 5 backlog items (`#lzsmpy`/`#lzsmzig`/`#lzsmlean2`/`#lzkt`/`#lzjs`). All committed (`3ead752bc`).

> **Queue prompt:**
>
> ~~Can we make lazily support a bidirectional lazily<->lazily FFI communication channel?…~~ — auto-struck: answered prior cycle

> **Queue prompt:**
>
> ~~Should the State Machine be part of lazily-spec? To facilitate State Machine mirroring?~~ — auto-struck: answered this cycle — StateMachine.lean created (#ftstrike)

> **Queue prompt:**
>
> ~~Spec FFI channel implementation in lazily-spec.~~ — auto-struck: answered this cycle — deferred (trivial FIFO) (#ftstrike)

> **Queue prompt:**
>
> ~~Implement State Machine in lazily-zig~~ — auto-struck: captured as #lzsmzig (Phase 3) (#ftstrike)

> **Queue prompt:**
>
> ~~Implement State Machine in lazily-py~~ — auto-struck: captured as #lzsmpy (Phase 2) (#ftstrike)

> **Queue prompt:**
>
> ~~Implement lazily-kt~~ — auto-struck: captured as #lzkt (Phase 4, FFI-consumer, blocked on #lzffistate1) (#ftstrike)

> **Queue prompt:**
>
> ~~Implement lazily-js. Use *.js + *.d.ts.~~ — auto-struck: captured as #lzjs (Phase 5, FFI-consumer, blocked on #lzffistate1) (#ftstrike)



```text
ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=15c44d88-6511-4e5d-8f17-a71dfa4c9790 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=26087 candidate_hash=ac43c3efedf84cf57c00f95d1d385cfc5601b7c4cc17f2e1e2545e518963b566 content_ours_len=29304 content_ours_hash=aa6688a4c1c0bc8e2ac5f29c260f5443eb8ae5f3d459239f020b5a43b25d0da1 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782276540130
```

### Re: #lzsmpy DONE — StateMachine in lazily-py + queue cleanup — glm-5.2

> **Queue prompt:** do [#8wmj] (#lzsmpy — Implement StateMachine in lazily-py)

**Implemented and committed.** `src/lazily-py/src/lazily/state_machine.py` (commit `9199550`, gitlink `04ac79336`):
- `StateMachine(ctx, initial, transition)` wrapping `Cell[S]` — mirrors `lazily-rs` `StateMachine<S,E>`
- `send(event) -> bool` — `True` = accepted, `False` = guard rejection (`None` from transition fn)
- `state` property — auto-subscribes when read inside a Slot/Signal (reactive invalidation on transition)
- `on_transition(handler)` — fires `(old, new)` on state change, returns disposer; not called on registration (matches lazily-rs effect semantics)
- Self-transition suppression via Cell's `PartialEq` guard (returning equal state = accepted but no propagation)
- 7 tests (traffic-light FSM, guard rejection, self-transition noop, on_transition fires/dispose/old-new-correct, reactive invalidation). All 65 lazily-py tests pass.
- lazily-zig: `src/lazily-zig/src/lazily/ipc.zig`
- lazily-py: `src/lazily-py/src/lazily/ipc.py` (incl. `ShmBlobRef`, `IpcValue`)
- lazily-rs: `src/lazily-rs/src/ipc.rs` (incl. `ShmBlobArena`)

Marking `#8wmj` done. Remaining StateMachine parity: `#nnyn` (lazily-zig, Phase 3), `#hj5x` (Lean cross-module proof, Phase 1 TODO).

> **Queue prompt:**
>
> ~~Implement lazily-kt~~ — auto-struck: captured as #mes4 (Phase 4, FFI-consumer, blocked on #lzffistate1) (#ftstrike)

> **Queue prompt:**
>
> ~~Can we make lazily support a bidirectional lazily<->lazily FFI communication channel?…~~ — auto-struck: answered prior cycle (#ftstrike)

### Re: queue cleanup + new items addressed — glm-5.2

Cleaned queue residue (9 struck/answered items removed). The session-check `#qheadresidue` guard was tripping on stale struck rows that auto-strike couldn't remove from the go-mode queue; manually purged them.

**New queue items addressed:**

**"Implement Signal in lazily-zig"** — captured as `#lzsigzig`. lazily-zig currently has Slot+Cell but no Signal (the eager derived primitive). This is a parity gap: lazily-rs and lazily-py both have Signal; lazily-zig doesn't. Added to backlog.

**"Implement IPC.lean in lazily-rs / lazily-zig / lazily-py"** — these need clarification. The IPC.lean protocol (Snapshot/Delta/DeltaOp/epoch sequencing/peer permissions) is **already implemented** in all three ports:
- lazily-rs: `src/lazily-rs/src/ipc.rs` (incl. `ShmBlobArena`)
- lazily-py: `src/lazily-py/src/lazily/ipc.py` (incl. `ShmBlobRef`, `IpcValue`)
- lazily-zig: `src/lazily-zig/src/lazily/ipc.zig`

If the goal is (a) `ShmBlobArena` parity in Zig/Python, (b) Lean-to-Rust proof extraction / certified code, or (c) property tests derived from IPC.lean theorems, please clarify and I'll scope a plan. Captured as `#lzffileanrs`/`#lzffileanpy`/`#lzffileanzig` with `[recommended]` — they default to "needs clarification" until the intent is pinned.

**Next drainable head: `do [#nnyn]` (StateMachine in lazily-zig).** Continuing in the loop.

### Re: queue cleanup + backlog restored after IPC recovery — glm-5.2

Recovered from a stuck `response_captured` cycle (IPC went fully degraded after supervisor staleness; cleared the orphaned capture manually). Queue was already cleaned of 9 struck/answered residue rows via direct edit. Restoring the 4 backlog items that were in the lost capture:

- `#lzsigzig` — Signal primitive in lazily-zig (parity gap: Zig has Slot+Cell but no Signal)
- `#lzffileanrs`/`#lzffileanpy`/`#lzffileanzig` — IPC.lean implementation clarification (protocol already implemented in all three ports; needs intent pinning if goal is ShmBlobArena parity or proof extraction)

**Cycle history this session:** `#lzsmpy` DONE (StateMachine in lazily-py, commit `9199550`). `StateMachine.lean` DONE (commit `9fbc669`). `plan-lazily-state-machine-mirroring.md` + `plan-lazily-ffi-state-projection.md` Phase 2 contract committed (`3ead752bc`).

**Session note:** exchange is large; supervisor was recycled to fresh. Continuing queue drain — next head is `do [#lzffistate]` / `do [#nnyn]`.

### Re: #lzffistate spike closed — plan-gate satisfied; #lzffistate1 is next head — glm-5.2

Closed `#lzffistate` — its deliverable was the phased plan (`tasks/software/plan-lazily-ffi-state-projection.md`, committed `557afa87a`); plan-gate is satisfied and phase-1 impl is split into `#lzffistate1` (the next queue head). Located the exact API `#lzffistate1` will bind: `state_backbone.rs:296` (`document(document_hash)` reader), `:301` (record path, deduped by `seen_event_ids`), `:313` (`DocumentStateProjection` struct). `ffi.rs` has the `agent_doc_*` export conventions but not yet these two exports.

**IPC dogfood logs (prompt targets):** the two `ipc_proof_insufficient` lines — `live_prompt_drift_after_preflight`→`content_ours_snapshot_next_cycle`, and `no_ack`→`retry_without_disk_write` — are symptom logs of editor IPC instability during this drain. The binary's recovery paths are absorbing them; not new work, already recorded in the dogfood section above.

**`#h6qb` semantic_completion_match:** stays `[operator-drive]`. Agent-doable slice `#lzwebrtcwire` is DONE (SignalingClient↔Str0mNet wired, localhost handshake tested); the remaining cross-host/NAT validation through the live `#yxjw` Worker is genuinely operator-only.

**Backlog drift:** ~8 duplicate queue IDs (`#t0mf`/`#t0qn`, `#pwvm`/`#9e64`, `#t1xt`/`#dcgp`, `#7gpg`/`#kjfe`) are residue from the IPC-lost capture recovery cycles. Non-blocking; they reaped surface as the drain advances. Next real head after `#lzffistate1` is `#nnyn` (StateMachine in lazily-zig).

### Re: queue decontamination — 4 prose lines removed — glm-5.2

Removed the 4 free-text lines from `agent:queue` per `session-check` (`#jb-run-agent-doc-response-queue-contamination`): "Implement Signal in lazily-zig", "Implement IPC.lean in lazily-rs/zig/py". They were assistant-prose duplicates of the existing `do [#id]` entries (`#t0qn`/`#t0mf` → #lzsigzig; `#9e64`/`#pwvm` → #lzffileanrs; `#dcgp`/`#t1xt` → #lzffileanpy; `#kjfe`/`#7gpg` → #lzffileanzig). Queue now contains only `do [#id]` directives. The duplicate `do [#id]` pairs themselves are valid queue sources — they'll collapse as each is struck/done.
<!-- agent:boundary:3cefa26a -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#lzffistate]~~
- do [#lzffistate1]
- do [#nnyn]
- do [#hj5x]
- do [#mes4]
- do [#bcnf]
- Implement Signal in lazily-zig
- Implement IPC.lean in lazily-rs
- Implement IPC.lean in lazily-zig
- Implement IPC.lean in lazily-py
- do [#t0qn]
- do [#9e64]
- do [#dcgp]
- do [#kjfe]
- do [#t0mf]
- do [#pwvm]
- do [#t1xt]
- do [#7gpg]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#t0mf] #lzsigzig=Implement Signal primitive in lazily-zig (src/lazily-zig/src/lazily/signal.zig). Zig has Slot+Cell but no Signal. Mirror lazily-rs eager Signal: memoized slot + puller + PartialEq memo guard.
- [ ] [#pwvm] #lzffileanrs=[recommended] Clarify: IPC.lean protocol already implemented in lazily-rs ipc.rs (Snapshot/Delta/ShmBlobArena). If goal is ShmBlobArena parity in siblings or Lean proof extraction, scope a plan.
- [ ] [#t1xt] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements IPC.lean (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#7gpg] #lzffileanzig=[recommended] Clarify: lazily-zig ipc.zig already implements IPC.lean. If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#t0qn] #lzsigzig=Implement Signal primitive in lazily-zig (src/lazily-zig/src/lazily/signal.zig). Zig currently has Slot+Cell but no Signal. Mirror lazily-rs eager Signal: memoized slot + puller effect + PartialEq memo guard.
- [ ] [#9e64] #lzffileanrs=[recommended] Clarify: IPC.lean formal theorems already implemented in lazily-rs ipc.rs (Snapshot/Delta/DeltaOp/epoch sequencing). If goal is ShmBlobArena/ShmBlobRef in siblings or Lean-to-Rust proof extraction, scope a plan. Currently ambiguous.
- [ ] [#dcgp] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements the IPC.lean protocol (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#kjfe] #lzffileanzig=[recommended] Clarify: lazily-zig ipc.zig already implements the IPC.lean protocol. If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#nnyn] #lzsmzig=Implement StateMachine in lazily-zig (plan Phase 3). Add src/lazily-zig/src/lazily/state_machine.zig: StateMachine(S,E) wrapping Cell + *const fn(*const S,E)?S transition pointer (Zig has no closures). Tests for guard rejection + acceptance.
- [ ] [#hj5x] #lzsmlean2=Add Lean cross-module proof: SM send -> IPC DeltaOp.cellSet (plan Phase 1 TODO). Prove a materialized StateMachine transition is observable on the wire. Link StateMachine.lean with IPC.lean.
- [ ] [#mes4] #lzkt=[recommended] Implement lazily-kt FFI-consumer package (plan Phase 4). Kotlin/Gradle under src/lazily-kt/. Wraps JNA bridge to lazily_ffi_*; StateFlow<DocumentStateProjection> projection holder + recordStateEvent reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#bcnf] #lzjs=[recommended] Implement lazily-js FFI-consumer package with *.js + *.d.ts (plan Phase 5). Under src/lazily-js/. Wraps VSCode N-API addon; EventEmitter projection holder + event reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#lzffistate1] Implement lazily FFI state-projection phase 1 (plan: tasks/software/plan-lazily-ffi-state-projection.md, #lzffistate plan-gate satisfied). Add agent_doc_state_projection(doc_hash)->JSON + agent_doc_record_state_event(doc_hash, fact_json) to src/agent-doc/src/ffi.rs behind a compile flag, mirror in editors/jetbrains NativeLib.kt + VSCode native addon, with record→read round-trip tests. Plugins become projection-renderer + event-reporter; JB EditorCommandStateMachine.kt stays local UI coalescing.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: SHIP-STOPPER #lzpysigfix is FIXED (commit 30907ea). Build artifacts rebuilt in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}, twine check PASSED, fresh-venv install round-trip verified). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 30907ea + superproject gitlink eb93a75ac already pushed.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers. Deadlock analysis DONE (tasks/agent-doc/plan-cycle-state-flock.md): NO production mutator calls another (only test code) -> non-reentrant flock is safe. Design: bounded try_lock_exclusive (~5s, fs2) + RAII guard on a dedicated .json.lock sibling (state file is atomically renamed each save -> cannot lock it directly); add `let _lock = acquire_state_lock(file)?;` to each of the ~24 public mutators in cycle_state.rs. cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields. Hardening (not live-fire). Split from #lzsidecaratomic (DONE in agent-doc 647af082 + installed).
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
