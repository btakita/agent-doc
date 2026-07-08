# Plan: Lazily command/RPC message passing for agent-doc editor plugin commands

`#lzmsgpcp`

## Implementation status (2026-07-07)

The command/RPC message plane (`command-plane-v1`) is implemented and verified
across the lazily stack. Phases 6–7 (agent-doc controller integration + rollout)
are deliberately deferred — the plan sequences them "after the lazily packages
land," and both preconditions are currently unmet (see below).

| Phase | Status | Evidence |
|---|---|---|
| 1. lazily-spec | ✅ done | `schemas/message-passing.json`, `docs/message-passing.md`, 8 fixtures in `conformance/message-passing/`, test hooks in `tests/test_schema_conformance.py`, coverage row; `make check` green |
| 2. lazily-formal + embedded | ✅ done | `LazilyFormal/Command.lean` in both `lazily-formal` and `lazily-spec/formal/lean`; `lake build` green; theorems: accepted_nonterminal, stale_generation_noop, duplicate_submit_idempotent, cancel_cannot_override_applied, terminal_conflict_fails_closed, projection_reconnect_equiv, rpc_call_terminal_only |
| 3. lazily-rs | ✅ done | `src/command.rs` (types, serde, `CommandProjection` reducer, `CommandRpcClient` facade, receipt helpers); `tests/command_conformance.rs` (9), `tests/schema_compliance.rs` command case; 10 unit tests; clippy `-D warnings` clean |
| 4. lazily-kt | ✅ done | `Command.kt` + `CommandConformanceTest.kt`; `./gradlew test` green |
| 5. lazily-js | ✅ done | command plane in `src/index.js` + `src/index.d.ts` (tsc `--strict` clean); `test/message-passing.test.js` (15) + schema-conformance command case; `npm test` green (237) |
| 6. agent-doc controller integration | ✅ endpoint done (live smoke pending) | **Payload contract:** `editors/command-payloads.md` defines every `agent-doc.*.v1` payload. **Shadow endpoint:** `agent-doc-controller-io/Cargo.toml` enables lazily `ipc`; `rpc.rs` adds `handle_editor_command_submit_rpc` (decodes `agent-doc.editor_route.v1` from a `CommandSubmit`, dispatches the unchanged `handle_editor_route_rpc`, folds a `CommandProjection` with progress events + a terminal causal receipt — applied on exit 0, rejected otherwise) plus an `"editor_command_submit"` dispatch arm. Wire-contract test `tests/command_plane_shadow.rs` **green (4/4)**; crate compiles with the endpoint. All edits additive (near lines ~4140 / ~5920); the concurrent editor-sync WIP in the same crate was not touched. Remaining: live editor smoke (operator-gated) + plugin switch (Phase 7) |
| 7. rollout and removal | ◑ opt-in shipped | **Gate 1 (shadow mode):** controller serves both `editor_route` and `editor_command_submit`. **Gate 2 (plugin opt-in):** both plugins switch to the command plane when `AGENT_DOC_COMMAND_PLANE=1`, else keep the classic path. VS Code: `editors/vscode/src/commandPlane.ts` (`buildEditorRouteCommandMessage` + `resolveEditorRouteTerminal`, terminal-only) wired into `extension.ts` `runEditorRouteViaPcp`; `commandPlane.test.ts` **green (5/5)**, `tsc` clean. JetBrains: `CpcRouteClient.kt` command-plane path (`editorCommandSubmitRequest` + `resolveCommandSubmitData`); `CpcRouteClientCommandPlaneTest.kt` **green (4/4)**. Both build with the concurrent WIP untouched. **Remaining gates 3–6:** live editor smoke, then default-on for Run (flip the `commandPlaneEnabled()` default), then sync/focus/session, then remove the old `editor_route` request shape after two plugin versions. Plugin version bump + publish deferred (concurrent gradle.properties WIP; no release cut per project convention). |

**Coverage marks** (`lazily-spec/coverage.json`): Rust ✅, Kotlin ✅, JS ✅;
Python/Dart/Zig/Go `—` (not in this slice).

**Compilation note:** the earlier "must republish lazily" concern was wrong —
`agent-doc/Cargo.toml` has `[patch.crates-io] lazily = { path = "../lazily-rs" }`
and lazily-rs 0.21.5 satisfies the controller's `lazily = "0.21"` pin, so the
controller already builds against the local command plane. Phase 6 wiring is
gated **only** on the concurrent editor-command WIP landing (patching those
churning files now would collide with an active session).

**Next step (live smoke + default-on):** the controller shadow endpoint and both
plugin opt-in paths are landed and unit-green. To finish the rollout: run a real
editor with `AGENT_DOC_COMMAND_PLANE=1` and confirm `Run Agent Doc` reaches
terminal `applied`/`rejected` (JB + VS Code); then flip `commandPlaneEnabled()`
to default-on for Run, extend to sync/focus/session, and remove the classic
`editor_route` request shape after two plugin versions ship with command-plane
support. Bump plugin versions + publish once the concurrent editor-sync WIP
lands.

## Operator ask

Create a plan for lazily message passing across `lazily-spec`,
`lazily-formal`, `lazily-rs`, `lazily-kt`, and `lazily-js` to support
agent-doc plugin command issues.

This plan intentionally creates the missing cross-repo design artifact. It does
not implement the protocol yet.

Update: lazily should support RPC ergonomics for plugin callers, but RPC should
be a facade over the evented command/message-passing substrate rather than a
separate authority or transport model.

## Problem

Agent-doc editor commands currently use several incompatible command paths:

- JetBrains `Run Agent Doc` sends a custom `editor_route` JSON request through
  `CpcRouteClient`.
- VS Code `Run Agent Doc` now sends a custom `editor_route` JSON request over
  `.agent-doc/controller.sock`.
- Manual sync and focus use PCP-backed native/FFI calls.
- Session operator commands still use CLI subprocess calls from plugins.
- CRDT replica traffic, file IPC patches, save-document signals, and visible
  write receipts each have their own message shape.

The recent `Run Agent Doc` PCP migration fixed one symptom, but it still leaves
the editor command surface with ad hoc request/response contracts. That makes
these issues recurring:

- command ids, route keys, controller generations, and payload hashes are not
  represented by one reusable message type;
- in-flight, dedupe, supersede, cancellation, retry, and timeout semantics are
  reimplemented separately in JB, VS Code, and Rust;
- `accepted` / `queued` style feedback can be mistaken for terminal success
  unless every caller remembers the local rule;
- reconnect after controller recycle has no standard command projection or
  resync protocol;
- plugin command tests cannot be shared across language bindings.

## Target model

Add a lazily command/RPC plane that is generic enough for editor and runtime
command traffic, while keeping domain payloads owned by agent-doc. The core
protocol is evented message passing; the public binding APIs expose RPC helpers
for callers that want a simple request/terminal-result shape.

The core contract:

1. Lazily owns the typed command envelope, correlation ids, idempotency,
   generation guards, causal receipts, projection rules, and conformance
   fixtures.
2. Agent-doc owns command namespaces and domain payload schemas such as
   `agent-doc.editor_route`, `agent-doc.sync_tmux_layout`,
   `agent-doc.focus_document_pane`, `agent-doc.save_document`,
   `agent-doc.session.clear`, and `agent-doc.crdt_replica.*`.
3. Causal receipts remain authoritative for terminal command outcomes:
   `observed` and `accepted` are progress only; `applied` and `rejected` are
   terminal.
4. Command progress/detail events are UX and diagnostics data, not proof that an
   effect happened. They may carry output summaries, queue position, retry
   advice, or copied CLI output, but terminal proof still folds through causal
   receipts and domain facts.
5. Every command can be replayed or deduped by `command_id` after reconnect.
   Stale generation receipts/events are ignored or rejected, never silently
   applied.
6. Existing `Snapshot` / `Delta` graph IPC is not replaced. Command messages are
   an additive sibling plane that can be carried over the same transports and
   reflected into the normal state graph projection.
7. RPC is a convenience facade over the command plane. `call` submits a command,
   observes events/receipts, and resolves only when the command projection has a
   terminal `applied` or `rejected` outcome. A transport ACK or `accepted` event
   never resolves the RPC call as successful completion.

## Proposed wire shape

Add a feature-gated command plane, advertised as `command-plane-v1` during
capability negotiation. Old peers that lack the feature fail closed before
accepting command traffic.

Do not add command requests as new `Snapshot` / `Delta` variants. Add a sibling
externally-tagged message family:

```json
{
  "CommandSubmit": {
    "command_id": "cmd-...",
    "causation_id": "cmd-...",
    "source": "vscode-plugin",
    "target": "project-controller",
    "namespace": "agent-doc",
    "name": "editor_route",
    "authority_generation": 42,
    "idempotency_key": "project-root:doc:run",
    "deadline_ms": 120000,
    "policy": {
      "dedupe": "same_idempotency_key",
      "supersede": false,
      "cancel_on_preempt": true
    },
    "payload_type": "agent-doc.editor_route.v1",
    "payload_hash": "sha256:...",
    "payload": { "Inline": [123, 34, 102, 105, 108, 101, 34, 58, 34, 46, 46, 46, 34, 125] },
    "required_features": ["causal-receipts", "command-events"]
  }
}
```

Other minimum variants:

- `CommandCancel`: cancel/preempt a still-nonterminal command by `command_id`
  with a generation and reason.
- `CommandEvents`: batch of progress/detail events keyed by `command_id`.
- `CommandProjection`: query/resync image of known command state after a
  reconnect or controller handoff.

## RPC facade

Expose RPC-style APIs in `lazily-rs`, `lazily-kt`, and `lazily-js`, implemented
entirely in terms of the command message family:

```ts
await client.call("agent-doc.editor_route", payload, {
  commandId,
  idempotencyKey: "project-root:doc:run",
  authorityGeneration: 42,
  deadlineMs: 120000,
  policy: {
    dedupe: "same_idempotency_key",
    supersede: false,
    cancelOnPreempt: true
  }
});
```

Facade semantics:

- `call` builds and sends `CommandSubmit`, then waits on the folded
  `CommandProjection` for a terminal causal receipt.
- `submit` returns the `command_id` immediately for callers that want explicit
  event/projection management.
- `cancel` sends `CommandCancel` and returns the resulting terminal or
  nonterminal projection depending on deadline.
- `observe` or `stream` exposes `CommandEvents` and projection updates for UI
  progress.
- Reconnect uses `CommandProjection`; callers do not restart an RPC blindly
  unless the idempotency policy says replay is safe.
- Unary RPC completion means command effect completion, not network delivery.
  A network ACK, controller admission, or queued state is progress only.

This gives plugins the API shape they want while preserving the properties that
motivated message passing: replay, dedupe, cancellation, generation guards,
progress streaming, and reconnect projection.

The exact names can change during implementation, but the required concepts
should not:

- stable `command_id` and `causation_id`;
- source/target identity;
- namespace/name;
- authority generation;
- idempotency key;
- deadline;
- dedupe/supersede/cancel policy;
- payload type/hash/body using existing `IpcValue` inline/shared-blob payloads;
- required feature list;
- terminal outcome via `CausalReceipt`.

## Agent-doc command mapping

The first agent-doc payload set should cover existing plugin command issues:

| Command | Current path | Lazily payload |
|---|---|---|
| `Run Agent Doc` | JB `CpcRouteClient`, VS Code raw socket `editor_route` | `agent-doc.editor_route.v1`: file path, relative path, dispatch-only/plain-trigger flags, wait budget, layout args, route key, editor attempt id |
| `Sync Tmux Layout` / `Load Tmux Window` | PCP native `sync_tmux_layout` | `agent-doc.sync_tmux_layout.v1`: project root, columns, focus, exact-visible, no-autostart, caller kind |
| focus handoff | PCP native `focus_document_pane` | `agent-doc.focus_document_pane.v1`: document path, project root, no promotion, active-window guard |
| `save_document` | file signal / socket IPC | `agent-doc.save_document.v1`: document path, patch id, expected generation/hash |
| session status/clear/restart/doctor | editor-spawned CLI | `agent-doc.session_command.v1`: subcommand, force/interrupt flags, document path, display policy |
| CRDT replica register/update/pull/ack | controller `crdt_replica` custom envelope | `agent-doc.crdt_replica.v1`: existing replica op body wrapped in command ids and receipts |

This lets JB and VS Code share command admission and receipt projection code
while agent-doc keeps its existing domain behavior behind the controller.

## Phase 0: inventory and compatibility decision

Deliverables:

- Enumerate every first-party plugin command and classify it as command-plane,
  state-plane, or local-only UI.
- Decide whether `CommandMessage` is a new top-level transport frame or a
  `LazilyFrame` wrapper around existing `IpcMessage`, `ReceiptMessage`, and
  command messages.
- Decide the stable RPC facade surface across bindings: `call`, `submit`,
  `cancel`, `observe`, `projection`, and the terminal-result error shape.
- Reserve FFI message kind `Command = 4` if the FFI boundary carries the new
  frame family. `CrdtSync = 3` stays unchanged.
- Define a downgrade rule: without `command-plane-v1`, first-party plugins keep
  the current PCP/CLI paths, but the binary emits a stale-plugin/update warning
  when a command requires the new plane.

Done when:

- The migration can be staged without breaking currently installed plugins.
- Every command has a named interim path and target final path.

## Phase 1: lazily-spec

Files likely touched:

- `src/lazily-spec/protocol.md`
- `src/lazily-spec/docs/message-passing.md`
- `src/lazily-spec/docs/SUMMARY.md`
- `src/lazily-spec/schemas/message-passing.json`
- `src/lazily-spec/schemas/defs.json` if shared ids need factoring
- `src/lazily-spec/conformance/message-passing/*.json`
- `src/lazily-spec/tests/test_schema_conformance.py`
- `src/lazily-spec/coverage.json`

Spec requirements:

- Define `CommandSubmit`, `CommandCancel`, `CommandEvents`, and
  `CommandProjection`.
- Define idempotency, generation, terminal conflict, stale-generation, and
  reconnect projection rules.
- Define RPC facade semantics: submit-vs-call, unary completion, streaming
  progress, deadline behavior, cancellation, reconnect, and terminal-result
  error mapping.
- State clearly that transport ACKs and nonterminal command events are not
  authority.
- Add command-plane capability negotiation.
- Add conformance fixtures:
  - `editor_route_submit.json`: full agent-doc Run payload over `CommandSubmit`;
  - `sync_tmux_layout_submit.json`: shared-blob capable payload fixture;
  - `accepted_then_applied_receipt.json`: nonterminal progress followed by
    terminal applied receipt;
  - `stale_generation_ignored.json`: stale receipt/event does not update
    projection;
  - `terminal_conflict_fail_closed.json`: conflicting terminal receipts fail;
  - `cancel_preempts_nonterminal.json`: cancel can terminally reject only before
    an applied terminal outcome;
  - `reconnect_command_projection.json`: snapshot of in-flight/terminal command
    state after reconnect;
  - `rpc_call_waits_for_terminal.json`: facade scenario proving accepted/queued
    progress does not resolve a unary call.

Verification:

- `uv run pytest` or the repo-local schema test command.
- `make check` from `src/lazily-spec` if available.

## Phase 2: lazily-formal and embedded lazily-spec formal

Files likely touched:

- `src/lazily-formal/LazilyFormal/Command.lean`
- `src/lazily-formal/LazilyFormal.lean`
- `src/lazily-spec/formal/lean/LazilyFormal/Command.lean`
- `src/lazily-spec/formal/lean/LazilyFormal.lean`

Formal model:

- Model `CommandState` with at least `submitted`, `accepted`, `running`,
  `cancelled`, `applied`, and `rejected`.
- Model generation equality as the guard for accepting events/receipts.
- Model idempotent command submit by command id.
- Model cancel/supersede as legal only while no terminal outcome exists.
- Model an RPC call as derived behavior over submit plus projection fold, not as
  a separate terminal-state authority.
- Reuse or import the existing receipt model so terminal semantics are not
  duplicated.

Theorems:

- `accepted_nonterminal`: accepted/admitted progress cannot complete a command.
- `stale_generation_noop`: stale events/receipts do not change the current
  projection.
- `duplicate_submit_idempotent`: replaying the same command id is a no-op or
  returns the same projection.
- `cancel_cannot_override_applied`: a cancel after applied is ignored or
  rejected, never terminally changes applied into rejected.
- `terminal_conflict_fails_closed`: applied vs rejected conflict is not resolved
  by winner selection.
- `projection_reconnect_equiv`: folding events then snapshotting is equivalent
  to applying the command projection after reconnect.
- `rpc_call_terminal_only`: a unary call cannot resolve success or failure until
  the projection contains a terminal causal receipt.

Verification:

- `lake build` in `src/lazily-formal`.
- `lake build` in `src/lazily-spec/formal/lean`.
- `make test-lazily-formal` and `make test-lean-formal` from bindings that
  already gate on both formal repos.

## Phase 3: lazily-rs

Files likely touched:

- `src/lazily-rs/src/command.rs`
- `src/lazily-rs/src/lib.rs`
- `src/lazily-rs/src/ffi.rs`
- `src/lazily-rs/tests/command_conformance.rs`
- `src/lazily-rs/tests/schema_compliance.rs`
- `src/lazily-rs/tests/ipc.rs` if the frame multiplexer lives there

Implementation:

- Add Rust types for command submit/cancel/events/projection.
- Add serde wire encoding matching `message-passing.json`.
- Add `CommandProjection` reducer that folds command events plus
  `CausalReceipt`s.
- Add a transport-agnostic `CommandRpcClient` facade or trait with `call`,
  `submit`, `cancel`, `observe`, and `projection` operations.
- Add helper constructors for observed/accepted/applied/rejected receipts keyed
  by command id.
- Add FFI frame support for the command plane if Phase 0 chooses the FFI route.
- Add transport-agnostic framing helpers, but keep Unix socket/WebSocket I/O
  outside the pure type module.

Agent-doc bridge preparation:

- Expose a small adapter API that agent-doc can use without knowing the full
  lazily internals: encode submit, decode submit, fold event/receipt, and build
  terminal receipts.

Verification:

- Rust unit tests for idempotency, stale generation, cancel, terminal conflict,
  projection replay, and RPC facade terminal-wait behavior.
- Conformance tests against every `message-passing` fixture.
- `cargo test --all-features` or `make check` in `src/lazily-rs`.

## Phase 4: lazily-kt

Files likely touched:

- `src/lazily-kt/src/main/kotlin/io/github/lazily/Command.kt`
- `src/lazily-kt/src/main/kotlin/io/github/lazily/LazilyFfiBoundary.kt`
- `src/lazily-kt/src/test/kotlin/io/github/lazily/CommandConformanceTest.kt`
- `src/lazily-kt/src/test/resources/conformance/message-passing/*.json`

Implementation:

- Add Kotlin command wire types with hand-rolled JSON parity matching the
  existing `Ipc.kt` / `Receipt.kt` style.
- Add `CommandProjection` reducer with the same status values as Rust.
- Add command-plane FFI frame support if selected.
- Add a thin `CommandClient` abstraction that JetBrains can use later:
  `call`, `submit`, `cancel`, `observeProjection`, `terminalFor`.

JetBrains adoption target:

- Replace `CpcRouteClient` raw JSON with `CommandClient.call` for
  `agent-doc.editor_route`, with `CommandSubmit` remaining the underlying wire
  message.
- Replace native sync/focus command receipts with command projections where the
  controller supports them.
- Keep IntelliJ UI logic local, but drive in-flight/dedupe/supersede state from
  the shared command projection.

Verification:

- Kotlin conformance replay for every message-passing fixture.
- Existing receipt tests still pass.
- `./gradlew test` or `make check` in `src/lazily-kt`.

## Phase 5: lazily-js

Files likely touched:

- `src/lazily-js/src/index.js`
- `src/lazily-js/src/index.d.ts`
- `src/lazily-js/test/message-passing.test.js`
- `src/lazily-js/test/schema-conformance.test.js`
- `src/lazily-js/test/conformance/message-passing/*.json`

Implementation:

- Add JS command classes and `.d.ts` types.
- Add `CommandProjection` reducer with the same behavior as Rust/Kotlin.
- Add `callCommand`, `submitCommand`, `cancelCommand`, and projection observer
  helpers for Node-based plugin callers.
- Add JSON encode/decode parity with the spec fixtures.
- Add a small socket transport helper for Node environments if needed by the VS
  Code plugin; browser/WebWorker environments can keep only the wire types.

VS Code adoption target:

- Replace the raw `net.createConnection(... controller.sock ...)` JSON request
  in the extension with lazily-js `callCommand` for
  `agent-doc.editor_route`.
- Fold command events and receipts to drive the existing
  `EditorCommandRegistry` decisions.
- Keep VS Code UI state local where appropriate, but require terminal proof from
  `CommandProjection`.

Verification:

- `npm test` in `src/lazily-js`.
- Fixture round-trip parity with Rust and Kotlin.
- TypeScript declaration tests for command payload construction and RPC helper
  calls.

## Phase 6: agent-doc controller integration

Files likely touched after the lazily packages land:

- `src/agent-doc/agent-doc-controller-io/src/project_controller/rpc.rs`
- `src/agent-doc/editors/jetbrains/.../CpcRouteClient.kt`
- `src/agent-doc/editors/vscode/src/extension.ts`
- `src/agent-doc/editors/vscode/src/native.ts`
- `src/agent-doc/src/ffi.rs`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/editors/vscode/SPEC.md`

Controller work:

- Add a lazily command endpoint beside existing `editor_route`,
  `sync_tmux_layout`, `focus_document_pane`, and `crdt_replica` endpoints.
- Serve the RPC facade by mapping each `call` to `CommandSubmit`, event
  observation, and terminal projection; the RPC response is the terminal command
  result, not the controller's admission ACK.
- Decode `agent-doc.*` command payloads and dispatch to existing controller
  code paths.
- Emit `CommandEvents` plus `CausalReceipt`s for observed, accepted/queued,
  started, applied, rejected, cancelled, superseded, and timed-out states.
- Persist command projection in the controller/state backbone so plugin reconnect
  can resync in-flight commands after handoff/recycle.
- Keep old endpoints during shadow mode and compare old receipts with new
  command projections.

Plugin migration:

- JB and VS Code call `Run Agent Doc` through the lazily RPC facade, with
  `CommandSubmit` remaining the underlying wire shape.
- Sync/focus move next because they already have PCP receipts.
- Session commands move last because they currently use CLI subprocesses and
  have broader operator UX surfaces.

Done when:

- `Run Agent Doc`, sync, focus, and session operator commands all have one
  command id, one generation rule, one receipt projection, and one reconnect
  story across JB and VS Code.

## Phase 7: rollout and removal

Rollout gates:

1. Shadow mode in controller: accept both old custom commands and new
   `CommandSubmit`; log projection mismatches.
2. Plugin opt-in flag: JB/VS Code can switch one command at a time to the new
   plane.
3. Default-on for `Run Agent Doc` through the RPC facade after both plugin tests
   and live editor smoke pass.
4. Default-on for sync/focus.
5. Default-on for session commands.
6. Remove old custom `editor_route` socket request shape only after two plugin
   versions have shipped with command-plane support.

Rollback:

- Keep existing PCP endpoints callable while the command plane is experimental.
- A plugin that lacks `command-plane-v1` should fail closed only for commands
  that require it; other existing commands keep their previous paths.

## Verification matrix

Cross-package gates:

| Package | Required checks |
|---|---|
| `lazily-spec` | schema tests, command fixtures, coverage update |
| `lazily-formal` | `lake build`, command theorems |
| `lazily-spec/formal/lean` | `lake build`, synced minimal command/receipt model |
| `lazily-rs` | command reducer/RPC facade tests, fixture round trips, schema compliance, `make check` |
| `lazily-kt` | fixture round trips, reducer/RPC facade tests, `./gradlew test` / `make check` |
| `lazily-js` | fixture round trips, reducer/RPC facade tests, `.d.ts` coverage, `npm test` |
| `agent-doc` | controller command endpoint tests, JB tests, VS Code tests, focused Rust controller tests, live editor smoke |

Agent-doc live smoke:

- JB `Run Agent Doc` command id appears in controller command projection and
  reaches terminal applied/rejected.
- VS Code `Run Agent Doc` uses the same wire shape and terminal rules.
- JB/VS Code RPC calls resolve only after terminal applied/rejected projection,
  not after accepted/queued/admission ACK.
- Repeating Run dedupes or supersedes according to declared policy, not local
  ad hoc string matching.
- Clear preempts a still-nonterminal Run through `CommandCancel`.
- Controller recycle/handoff preserves enough command projection for plugins to
  show the correct in-flight or terminal state after reconnect.

## Risks and decisions

- **Top-level frame shape.** Need an explicit decision on whether commands are
  a new `CommandMessage` sibling or a broader `LazilyFrame` wrapper. A wrapper
  is cleaner long-term but touches more bindings.
- **Domain payload schemas.** Lazily should not absorb agent-doc command
  payload details. Agent-doc should publish its own payload schemas and only
  reference lazily's envelope.
- **Progress vs proof.** Command events must not become a second proof system.
  The plan depends on keeping causal receipts as the terminal outcome source.
- **RPC hiding command semantics.** The facade must not collapse accepted,
  queued, applied, rejected, and timed-out into one generic request/response
  boolean. RPC errors should expose the terminal command projection.
- **Same-version plugin installs.** During rollout, plugin versions must bump
  whenever command-plane behavior changes, or stale-plugin detection cannot
  guide operators reliably.
- **Formal duplication.** The embedded `lazily-spec/formal/lean` model and the
  standalone `lazily-formal` model need a clear sync rule. Until one source is
  retired, both must build.

## First implementation slice

The smallest useful slice is:

1. Add `message-passing.md`, schema, and fixtures in `lazily-spec`.
2. Add `Command.lean` to `lazily-formal` and the embedded spec formal model,
   proving nonterminal/terminal/generation/idempotency rules.
3. Implement `CommandSubmit`, `CommandEvents`, `CommandProjection`, and the
   minimal RPC facade in `lazily-rs`, `lazily-kt`, and `lazily-js`.
4. Add an agent-doc controller shadow endpoint that decodes
   `agent-doc.editor_route.v1` from the lazily command envelope, dispatches the
   existing `editor_route` path, and emits command events plus causal receipts.
5. Switch VS Code `Run Agent Doc` to the lazily-js RPC facade first because its
   current raw socket client is the smallest isolated caller.
6. Switch JetBrains `Run Agent Doc` after the Kotlin RPC facade and projection
   tests pass.

This slice directly addresses the plugin command issue that prompted the plan
without forcing sync/focus/session-command migration into the first landing.
