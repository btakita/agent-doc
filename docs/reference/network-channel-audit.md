# Network channel audit

Audit `#netadv1`, 2026-10-05, against `v0.35.456`. This is phase 1 of the
network-adversarial-correctness plan. The formal-model half lives in
[`formal/tla/README.md` § Network channel assumptions](../../formal/tla/README.md#network-channel-assumptions).

The operator requirement is that agent-doc is provably correct under varying
network conditions. The design point is a Coder remote workspace (JetBrains
Remote Dev backend) with Zscaler in the path, which brings high and variable
latency, silent half-open stalls, and lost messages. The target property set:

- **Safety** holds under any delay, reorder, loss, duplication, and reconnect.
- **No timeout value** is part of a safety argument.
- **Liveness** holds when a message re-sent infinitely often is eventually delivered.

Cites are `file:line` relative to the repository root. `rpc.rs` means
`agent-doc-controller-io/src/project_controller/rpc.rs`. `JB/` means
`editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/`.

## 1. Topology: where the network actually is

Every agent-doc channel is local to the workspace host:

| Channel | Transport |
|---|---|
| Controller RPC | AF_UNIX `<root>/.agent-doc/controller.sock`, NDJSON, one request per connection (`rpc.rs:450`, `:1429-1473`) |
| Editor intent IPC | AF_UNIX `ipc-<pid>.sock` with an `ipc_hello` handshake (`agent-doc-ipc-io/src/lib.rs:183-202`, `:576-581`) |
| Editor → controller replica RPC | AF_UNIX `controller.sock` (`JB/CrdtReplicaForwarder.kt:1030-1048`) |
| Reliable-sync op/liveness push | JNA into the cdylib (`src/ffi.rs:2906-3091`), then controller RPC `reliable_sync_outbox` (`rpc.rs:16378-16437`), then SQLite outbox |
| Supervisor IPC | AF_UNIX `.agent-doc/supervisor/<uuid>.sock` (`agent-doc-supervisor-io/src/ipc.rs:845-871`) |
| Supervisor ↔ controller recycle | durable `state.db` events with an epoch (`agent-doc-supervisor-io/src/recycle_request.rs:23-131`) |
| tmux | one `tmux` subprocess per command, no timeout (`agent-doc-tmux-io/src/lib.rs:189-219`) |

No audited crate uses TCP. Zscaler sits only on the JetBrains thin-client ↔
backend link, which is JetBrains' own protocol. That has three consequences:

1. **Network faults reach agent-doc as slow or wedged peers**, not as lost
   packets. Examples are an EDT stalled behind remote-dev traffic, delayed or
   coalesced editor events, a controller starved by load, or a hung tmux server.
   In this audit a "half-open stall" means a peer that accepted a connection and
   stopped answering. A "lost message" means a single-attempt send whose failure
   or timeout is not followed by a resend or a pull.
2. **"Editor visible" means the backend `Document`.** agent-doc has no witness for
   what the thin client renders.
3. **Remote Dev has no `currentWindow` or `EditorWindow` on the backend**
   (`JB/EditorTabSyncListener.kt:241-259`, `:489-490`;
   `agent-doc-editor-surface/src/remote_layout.rs:1-38`). When the layout evidence
   is ambiguous, the surface reuses the previous layout.

**Coder defect:** the controller socket path is hard-wired
(`agent-doc-controller/src/paths.rs:15-17`; `JB/CpRouteClient.kt:1168`;
`JB/CrdtReplicaForwarder.kt:1030`). The editor socket honours
`AGENT_DOC_SOCKET_DIR` (`agent-doc-ipc-io/src/lib.rs:68-73`); the controller
socket does not. On a workspace volume that refuses AF_UNIX binds, every
controller lane fails.

The adversarial-channel requirement still applies. Each of these hops behaves
like a channel with unbounded delay, because peer latency is unbounded. A
single-attempt send whose timeout is read as a verdict is the local equivalent
of trusting a lossy network.

## 2. Protocol table

| Protocol | Retry until acked? | Receiver idempotency | Half-open detection | Reconnect resync source | Correctness-path timeouts |
|---|---|---|---|---|---|
| **P1 Editor intent `apply_canonical` (response delivery, socket path)** | No. A single attempt, then the caller falls back to document authority (`agent-doc-write-ipc-io/src/transport.rs:625-629`, `:974-1081`; `agent-doc-write-runtime-io/src/run_entry.rs:1690-1708`). Every retry mints a new `patch_id` (`transport.rs:368-370`; `reuse_patch_id=None` at `run_entry.rs:1647`). | `patch_id` map with a 60s TTL (`JB/PatchWatcher.kt:80-81`, `:121-141`), plus a content `already_applied` check (`agent-doc-ipc-protocol/src/lib.rs:435-437`). The boundary is seeded from `patch_id` (`transport.rs:519-529`). | Connect watchdog 3s (`agent-doc-ipc-io/src/lib.rs:105`, `:349-363`); send/recv 6s per phase (`:89`, `:565-570`); listener read 30s (`:110`) | None at this layer. Proof comes from the controller visible-write projection and `state.db` (`agent-doc-write-converge-io/src/lib.rs:4554-4720`). | `IPC_RECEIPT_TIMEOUT_SECS`=6 (`ipc-io/lib.rs:89`) feeds a 2-strike wedge (`write-converge-io/lib.rs:4495`) and a supervisor recycle request |
| **P2 Other editor intents** (`refresh_content`, `persist_current`, `observe_lazily_current`, `reposition`, `reload_library`) | No, all single attempt (`ipc-io/lib.rs:939-1095`; `write-ipc-io/src/lib.rs:396-476`). `persist_current` is retried by the controller's retained-persistence worker up to 8 times, 1s→30s (`agent-doc-controller-io/src/project_controller.rs:3784-3838`) | hash/len CAS for refresh and persist (`ipc-protocol/src/lib.rs:1044-1112`); in-flight key for observe (`ipc-io/lib.rs:829-862`); none for reposition | as P1 | none | `reload_library` uses 6s on the normal path (`ipc-io/lib.rs:1074`); the 90s budget (`:99`) applies only to the legacy path |
| **P3 Controller → editor `deliver_crdt_remote` wake** | No. One shot per event; a failure is only logged as `crdt_replica_notify_deferred` (`agent-doc-crdt-relay-io/src/lib.rs:5361-5432`, `:3563-3575`) | It is a wake; idempotency comes from the pull | as P1 | The editor pull (P4), which is event-driven only | Editor attach wait `CRDT_AWAIT_ATTACH_TIMEOUT_MS`=750 (`JB/CrdtReplicaManager.kt:151`, `:1245-1250`) |
| **P4 Editor → controller replica RPC** (`register`, `pull`, `update`, `projection` = ACK, `deregister`) | `register` retries 1s→30s (`JB/CrdtReplicaManager.kt:158-159`). `pull` only on events, with backoff 100ms→30s. `update` and `deregister` are fire-and-forget (`JB/CrdtReplicaForwarder.kt:837-842`, `:933-935`). The `projection` ACK failure only requests a drain (`JB/CrdtReplicaManager.kt:2642-2679`). | CRDT op ids; cumulative ACK by content hash (`agent-doc-document-realtime/src/crdt_relay.rs:2650-2700`); legacy `(patch_id, generation)` (`:2547-2580`); state-vector bootstrap | **Client: none.** `sendToSocket` has no deadline (`JB/CrdtReplicaForwarder.kt:1032-1048`). Server idle read 5s (`project_controller.rs:103`; `rpc.rs:15322`). | Register with a state vector; `missing_replica` makes the editor re-register (`agent-doc-crdt-relay-io/src/lib.rs:3620-3636`); peer-missing pull every 5s or more (`src/ffi.rs:2948-2984`) | Barrier release after 12 no-progress charges of about 50ms (`crdt_relay.rs:192`; `crdt-relay-io/lib.rs:4860-4865`); `MAX_REDELIVERIES_WITHOUT_ACK`=50 (`crdt_relay.rs:169`) |
| **P5 Reliable-sync document-op push** | Yes, by re-derivation. A failed RPC does not advance `pushedVersion`, so the next diff re-carries the ops (`JB/CrdtReplicaForwarder.kt:369-379`). `LOCAL_EDITOR_RETRY` 250ms→30s (`JB/CrdtReplicaManager.kt:163-164`). | Monotonic epoch cursor (`agent-doc-sqlite/src/reliable_sync_inbox.rs:98`); the CRDT fold is idempotent within a lineage (`rpc.rs:17585-17612`) | Controller RPC 5s (`project_controller.rs:88`) | SQLite outbox replay (`agent-doc-reliable-sync-io/src/push.rs:68-116`) | 5s. Firing only marks the push non-durable. **This is the model the other protocols should copy.** |
| **P6 Reliable-sync liveness (Open/Close OR-set)** | Open: up to 8 attempts, roughly 16s (`JB/ReliableSyncLivenessListener.kt:77-93`, `:311-322`). **Close: one shot** (`:153-174`) | OR-set tags (`JB/ReliableSyncLivenessGraph.kt:42-72`) plus epoch cursor | Editor death is detected by a pid poll every 500ms (`process_exit_watcher.rs:52`). A wedged IDE is never detected. | Receiver journal (`agent-doc-crdt-relay-io/src/lib.rs:183-246`) | 5s |
| **P7 Controller RPC, generic** (`ControllerRequest`) | Transport drop: one retry (`rpc.rs:697-741`). Handoff refusal: 200ms polls up to 6×30s (`rpc.rs:758-861`). Recv timeout: never retried (`rpc.rs:660-678`). | Async editor commands: `command_id` in an in-memory map with a 5 min TTL, capacity 256 (`project_controller.rs:2797`, `:2921-2997`). `idempotency_key` is built (`rpc.rs:3200-3240`) but never read server-side. Surface observations use `(client_id, generation, sequence)` (`rpc.rs:268-286`). | Client recv timeout only. **No connect watchdog and no send timeout** on either side (`rpc.rs:588-600`, `:650-652`, `:15340-15342`). Server idle read 5s. | State-plane cursor `(controller_generation, plane_version)` (`rpc.rs:1113-1138`); predecessor forwards async commands (`rpc.rs:10040-10133`) | `CONTROLLER_RPC_TIMEOUT` 5s (`project_controller.rs:88`); `CONTROLLER_CRDT_REVISION_READ_TIMEOUT` 750ms (`rpc.rs:45`); `COMMAND_SUPERVISOR_FRESHNESS_TIMEOUT` 250ms (`rpc.rs:40`, fail-open); `EDITOR_ROUTE_PLANE_CATCH_UP_MAX` 500ms (`rpc.rs:11103-11133`) |
| **P8 Visible-write receipt** (writer side) | Yes. A pull loop re-reads authority every iteration in chunks of 250ms or less (`agent-doc-write-converge-io/src/lib.rs:87-92`, `:1875-1940`; `rpc.rs:6107-6193`) | content hash | chunked await | `state.db` projection fallback (`rpc.rs:6107-6147`) | `VISIBLE_WRITE_RECEIPT_TIMEOUT_MS`=6000 (`write-converge-io/lib.rs:84`); fails closed and keeps the intent |
| **P9 Closeout owner lease** | Claim: one shot, 15s (`agent-doc-write-runtime-io/src/lib.rs:608-621`; `rpc.rs:5314-5357`). Heartbeat every 100s, but it **stops permanently after one failed refresh** (`write-runtime-io/lib.rs:655-679`). | Server CAS on `owner_id`. The `owner_id` is minted per call (`rpc.rs:5301-5307`). | none | lease expiry, 300s (`agent-doc-state-backbone/src/lib.rs:4370`) | `CONTROLLER_CLOSEOUT_COORDINATION_TIMEOUT` 15s (`rpc.rs:73`) |
| **P10 Supervisor IPC** (`inject`, `restart`, `stop`, `state`) | No retry. Errors are typed `Connect` vs `ResponseTimeout` (`agent-doc-supervisor-io/src/ipc.rs:63-110`). | `inject`: in-memory key `source:session:generation:pane:content_hash` (`agent-doc-start-runtime-io/src/lib.rs:2469-2503`), lost on re-exec. `restart`/`stop`: none. | `probe_socket` treats only ECONNREFUSED/ENOENT as dead (`ipc.rs:926-937`); server read 5s; **no connect watchdog or send timeout** (`ipc.rs:873-880`, `:969-972`) | none | query 2s, effect 10s, accept read 5s (`ipc.rs:57-59`) |
| **P11 Supervisor lease** | Refreshed only on register, lifecycle transitions and status reconcile. **No periodic heartbeat was found** (`agent-doc-start-runtime-io/src/run.rs:1011`; `lib.rs:2125`; `rpc.rs:20131`). | session/pane/generation match (`rpc.rs:20249-20268`) | Fresh means a heartbeat within 60s **and** the pid is alive (`agent-doc-controller/src/status.rs:774-784`; `project_controller.rs:8944`) | `state.db` row | 60s staleness, which a safety decision depends on (see R6) |
| **P12 Recycle request** | Yes. Durable epoch event, polled at idle boundaries (`agent-doc-start-runtime-io/src/idle_watch.rs:3608-3615`); the layout path re-requests every 600s (`agent-doc-sync-io/src/layout_column_audit.rs:51`) | epoch high-water mark (`agent-doc-supervisor-io/src/recycle_request.rs:55-74`) | — | `state.db` | `RECYCLE_INFLIGHT_SETTLE_TTL_SECS`=120 (`agent-doc-controller/src/dispatch.rs:2222`); `SUPERVISOR_RECYCLE_SETTLE_WAIT` 10s (`project_controller.rs:104`). Modelled in `RecycleSettleDispatch.tla` with `ASSUME Ttl > WaitBudget`. |
| **P13 Editor focus → tmux `select-pane`** | No. `TransportUnavailable`/`Retained` only log (`JB/EditorTabSyncListener.kt:1168-1181`). One self-heal resend on timeout with the same sequence number (`JB/CpRouteClient.kt:225-231`, `:1294-1335`). There is **no read-back** of the active pane (`rpc.rs:24360`). | `(client_id, generation, sequence)`. The graph advances `focused_document` **before** the effect, so a resend becomes `Idle` (`agent-doc-editor-surface/src/lib.rs:383-398`). | 1s plugin watchdog (`JB/CpRouteClient.kt:160`) | Reverse mirror polls `tmux_focus_state` every 500ms (`JB/TmuxPaneFocusSync.kt:172`). The graph is in memory only. | `EDITOR_FOCUS_OBSERVE_TIMEOUT_MS`=1000 (`JB/CpRouteClient.kt:160`), which triggers controller self-heal (R1) |
| **P14 Editor layout → tmux reconcile** | Plugin → controller retries 100ms→2s until exit 0 (`JB/EditorTabSyncListener.kt:398-405`, `:690-721`). The worker retries with read-back, 250ms→5s (`rpc.rs:83-84`, `:23577-23624`, `:23791-23815`). | Signature `Idle`; generation and work-revision supersede (`rpc.rs:23219-23245`) | 60s watchdog (`JB/CpRouteClient.kt:152`) | Desired columns are stored in `state.db` (`rpc.rs:25085`) but **not reloaded into the graph on controller start**. The generation is per process (GH #136). | `SYNC_LOCK_WAIT_BUDGET` 3s, after which the sync **proceeds unlocked** (R7); many 100-1000ms sync budgets (`agent-doc-sync/src/lib.rs:14-34`, logged only) |
| **P15 Prompt injection by tmux `send-keys`** | Text, 80ms, then `Enter` (`agent-doc-tmux-commands/src/lib.rs:258-262`). Acceptance poll every 150ms for 1s. Retries send `Enter` only (`agent-doc-route-io/src/direct_pane_dispatch.rs:208-240`, `:395-445`). Dispatch-start proof within 10-15s (`dispatch_start.rs:251-262`). | none at the receiver. Duplicate protection rests on screen observation. | bare-shell check (`direct_pane_dispatch.rs:452-497`) | cycle-state admission projection (`admission_projection.rs:31-43`) | `PASS_THROUGH_STRANDED_DRAFT_SETTLE` 150ms (`agent-doc-controller/src/dispatch.rs:2937`); stranded-draft admission 3s (`:3227-3233`), see R8 |

### P7 deadline interpretation

The generic 5.0s receive deadline is expected only at a process boundary: an
external caller can observe it while a live controller is scheduled late or is
not servicing its socket. The associated records describe different outcomes:

- `controller_state_event_deadline_retry` is a first-expiry recovery record. A
  stable event id permits exactly one replay; only the second expiry is returned.
- `controller_crdt_current_text_read_unavailable` is a bounded observation miss
  with `idle_disk_fallback`; `retained_write_settlement_local_fallback` derives a
  conservative local settlement rather than losing retained intent.
- `document_model_controller_lookup_error` falls back to the embedded relay when
  present. With no embedded replica it is an unavailable-read error, not proof
  that a write or route failed.

Controller-owned request and effect workers are not expected to reach P7 at
all. They carry controller-local identity and read the reactive projection in
process. In particular, synchronous VS Code Run and the JetBrains async-command
worker share that invariant. A five-second self-RPC appearing in an
`editor_command_async_completed` failure is a context-propagation defect, not an
operator route verdict; the repair is local context propagation, never blind
whole-route replay after potentially ambiguous pane dispatch.

## 3. Serial round trips on hot paths

These are counts for one operation on the happy path. Every one of them multiplies
by peer latency under load.

**Response delivery (CRDT path).** About 11 serial UDS round trips plus 2 EDT hops:

1. CLI → controller CP write: 1 round trip (`agent-doc-crdt-relay-io/src/lib.rs:3490-3592`).
2. Controller → editor `deliver_crdt_remote`: 2 round trips, handshake plus receipt (`:5361-5378`).
3. Editor → controller `flushDocumentOps` then `replica_pull`: 2 round trips (`JB/CrdtReplicaForwarder.kt:871-875`).
4. EDT apply.
5. `replica_projection` ACK: 1 round trip.
6. CLI visible-receipt long poll: at least 1 round trip (`agent-doc-document-realtime-io/src/lib.rs:1229-1297`).
7. CLI → editor `persist_current`: 2 round trips, followed by an EDT `saveDocument`.
8. `replica_projection(disk_persisted)`: 1 round trip.
9. Re-observe authority: 1 round trip (`document-realtime-io/lib.rs:1681-1716`).

**Response delivery (socket patch path).** About 2 editor connects, plus the
hello round trip, plus accepted/applied, plus at least 6 serial controller RPCs:
registration lookups ×1-2, the visible-write status/await loop at ≥3 per
iteration, and the canonical fold ≥1 (`agent-doc-write-ipc-io/src/transport.rs:354-1090`).

**Closeout ACK.** Traced in `#netadv5` (finalize of a CRDT document with an
attached editor, `run_stream`). Every non-embedded `request_controller*` call
pays a `connect_or_launch` `status` round trip before its real request.

| Step | Site | Serial round trips |
|---|---|---|
| Owner claim (`claim_closeout_owner_for_file`) | write-runtime-io `claim_foreground_closeout_owner`; `rpc.rs` claim | 2 (was 3: a redundant `ensure_controller_running` was removed in `#netadv5`; pinned by `CLOSEOUT_OWNER_CLAIM_SERIAL_ROUND_TRIPS`) |
| Lease heartbeat | background thread every 100s | 2 each, not serial |
| Stale-supervisor stage check | `recycle_stale_supervisor_for_turn_stage("finalize_write_start")` | 1-2 |
| Live queue heads + pre-write guards | `observe_live_queue_heads`, `resolve_commit_mode`, lint | ≥5 (cycle-state projection reads over `send_ndjson_request_to_actor`) |
| `response_cell_add` | `response_cell_via_controller_model_for_doc` | 3, plus nested `deliver_crdt_remote` 2 per editor inside the handler |
| Materialize + canonical observe | `materialize_response_cell_projection` | ≥2 |
| Visible-delivery receipt | `visible_editor_projection_receipt_for_target` + wake subscribe | 2, or ~5 per wait iteration |
| Native save | `await_canonical_editor_projection_persisted` → `persist_current` (editor IPC, 2) + observes | ≈7 |
| Commit barrier, `commit_document` (+ nested `refresh_vcs` 2 per editor), baseline record | `complete_required_closeout` | ≈7 |
| Cycle commit, maintenance, clean-closeout, actor closeout, terminal proof | `closeout.rs:226-319` | ≥5 |
| Owner release (`release_closeout_owner_for_file`) | `CloseoutOwnerGuard` drop | 2 (was 3; pinned by `CLOSEOUT_OWNER_RELEASE_SERIAL_ROUND_TRIPS`) |

Floor: about 26 serial round trips after the `#netadv5` trim, realistically
50 or more once the dynamic cycle-state projection reads are counted. The
claim/release budget is asserted by
`closeout_owner_claim_and_release_stay_within_round_trip_budget` through the
per-thread probe `controller_round_trips_on_this_thread`. Each editor intent is
asserted at 2 round trips (`EDITOR_INTENT_SERIAL_ROUND_TRIPS`,
`editor_intent_costs_two_serial_round_trips`). The `send_ndjson_request_to_actor`
funnel in `agent-doc-state-wire` is not yet counted.

Fixed sleeps on this path: native-save fallback 25→250ms backoff
(`agent-doc-document-realtime-io/src/lib.rs:1825`, bounded by
`CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS`) and the CRDT write-frontier backoff
(`:3999`). Both poll an observable state rather than deciding by time, but
should become wake-driven.

**Focus projection.** 1-2 plugin round trips (focus lane plus spanning lane),
then about 12-16 serial subprocesses in the controller. `#netadv5` trace of the
`Focus` intent: 1 `i3-msg -t get_tree` (`rpc.rs:9879`; the second one runs only
on the fenced async-focus path) and 14 tmux subprocesses (15 with
`list-windows`), including three separate `list-panes -a` and two repeated
`display-message #{window_id}`; no pane-snapshot scope is open. Not yet
asserted by a test: the handler takes a concrete `tmux_router::Tmux`, so the
`TmuxCommandRunner` fakes cannot be injected. Original audit list:

- 2 × `i3-msg -t get_tree` (`rpc.rs:9857`, `:24681`)
- `pane_alive` ×2, `pane_pid`, `pane_session`, `active_window`, `active_pane`, `pane_window`, `list-windows`
- `/proc` ownership checks
- `select-pane` (`rpc.rs:24424-24737`)

**Layout reconcile.** The comments record about 109 subprocess spawns, which the
cache reduces to 35 distinct tmux commands (`agent-doc-sync-io/src/sync.rs:2776-2779`).
That is followed by a read-back survey.

**Prompt injection (direct pane).** About 10 or more tmux subprocesses before
dispatch-start polling begins (`agent-doc-route-io/src/dispatch.rs:866-1040`).

## 4. Ranked: protocols whose safety depends on timing or reliable delivery

Ranked worst first. Severity weighs how easily load or latency triggers the
fault against how irreversible the result is.

**R1. A busy controller is killed by a 1s focus timeout.**

- *Chain:*
  1. `EDITOR_FOCUS_OBSERVE_TIMEOUT_MS = 1_000L` (`JB/CpRouteClient.kt:160`).
  2. `selfHealOnTimeout` reads the timeout as a controller that is bound but not serving (`JB/CpRouteClient.kt:1294-1335`).
  3. That calls `agent_doc_ensure_controller_running` (`src/ffi.rs:3383-3393`), which runs `ensure_serving_controller` (`rpc.rs:12906-12933`).
  4. If the `status` RPC does not answer within `CONTROLLER_RPC_TIMEOUT` (5s), `discover_stale_duplicate_pids(root, None)` adds the bootstrap pid, which is the live controller (`rpc.rs:4741-4767`).
  5. `reap_verified_controller_pid` sends SIGTERM, waits 750ms, then SIGKILL (`rpc.rs:4770-4786`).
- *Why it is a safety problem:* two timeouts become a verdict to kill the authority mid-write, mid-closeout or mid-handoff. A wedged tmux makes this more likely, because `Command::output()` has no deadline (`agent-doc-tmux-io/src/lib.rs:192-215`). A hung tmux stalls the focus handler, which trips the 1s timeout.
- *Formal coverage:* no model has this edge.

**R2. A slow editor attach is classified as a definitive refusal, and the replica is dropped from the delivery cut.**

- *Chain:*
  1. `deliver_crdt_remote` forces a re-register, then waits `CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L` (`JB/CrdtReplicaManager.kt:151`, `:1245-1250`). A timeout returns `false`.
  2. The plugin answers `APPLY_FAILED` (rejected) (`JB/PatchWatcher.kt:641-650`).
  3. `recovering_send_error_is_definitive(receipt_rejected, ..)` returns true for any rejected receipt (`agent-doc-crdt-relay-io/src/lib.rs:3721-3726`). That gives `DefinitivelyRefusedByAll` (`:5170-5176`), then `DropFromDeliveryCut` (`:5280-5290`), then `drop_definitively_refused_replica` (`:3788`).
  4. The persist lane does the same: `CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS` 5s counts as `native_save_definitive_refusals` (`JB/CrdtReplicaManager.kt:157`; `lib.rs:5093`). The submitted save is not cancelled and may still land.
- *Why it is a safety problem:* `VisibleDeliveryReceipt.tla` permits the drop edge only on proof that "the endpoint answered and refused", and in this chain a timeout manufactures that proof. A coalesced re-register within 5s also answers rejected (`JB/CrdtReplicaManager.kt:180`, `:4644-4648`).

**R3. One slow connect unlinks a live editor's socket.**

- *Chain:* `is_listener_active_for_pid` runs `std::fs::remove_file(&sock)` on any connect error, including the 3s watchdog (`agent-doc-ipc-io/src/lib.rs:314-327`).
- *Why it is a safety problem:* the listener calls an unlinked name "dead for good" (`:1354-1360`). The function sits on the hot delivery path (`agent-doc-write-ipc-io/src/transport.rs:518`), the reposition path, the commit path, and discovery/prune. The listener-start path uses the round-trip `probe_endpoint` (`:398-440`); this one does not.

**R4. Closeout owner lease: one-shot claim and a heartbeat that stops after one failure.**

- *Lost claim response:* the claim is sent once, so if the response is lost or late after the CAS applied, the live owner pid holds a 300s lease no one uses. Every later closeout then gets `HeldByOther` (`agent-doc-write-runtime-io/src/lib.rs:608-621`; `rpc.rs:68-72`, `:5301-5357`).
- *Failed refresh:* one refresh that does not return `Acquired`, including a single RPC timeout, `break`s the heartbeat loop while the foreground closeout continues (`write-runtime-io/lib.rs:655-679`). The lease then expires under a live owner, which permits concurrent takeover.

**R5. Controller unreachable is read as "no live editor".**

- *Chain:* `reliable_sync_editor_live_for_file` returns `false` on a connect error but `true` on a timeout (`rpc.rs:17111-17114` vs `:17123-17132`). The result feeds `editor_crdt_authority_attached` and the absent-editor write path (`agent-doc-write-runtime-io/src/run_entry.rs:2564-2577`).
- *Mitigation:* the local durable reliable-sync plane is checked first (`rpc.rs:17100-17102`), so the edge is reached only on a durable-plane miss during a controller handoff or recycle.
- *Why it is a safety problem:* a connection refused during handoff is evidence about the controller, not about the editor.

**R6. A supervisor lease that is fresh only with traffic gates claim takeover.**

- *Chain:* freshness requires a heartbeat within 60s (`agent-doc-controller/src/status.rs:774-784`; `project_controller.rs:8944`), but the lease is refreshed only on state transitions. A live supervisor that is idle for more than 60s reads as not fresh, so `fresh_foreign_supervisor_lease_holds_document` returns false (`project_controller.rs:8970-8990`). That permits claim's cross-session auto-force (`agent-doc-claim-io/src/lib.rs:358-388`).
- *Formal coverage:* `PaneExecutionAuthority.tla` models ownership with an atomic check-and-set and a single rebind race.

**R7. A contended sync runs without its lock.**

- *Chain:* after `SYNC_LOCK_WAIT_BUDGET` = 3s (`agent-doc-sync/src/lib.rs:32`), only `SafePassive` aborts. Every other mode continues the stash and join-pane reconcile without exclusion (`agent-doc-sync-io/src/sync.rs:2748-2773`).
- *Why it is a safety problem:* mutual exclusion lasts only until a timer fires.

**R8. Prompt injection can deliver twice.**

- *Stranded-draft path:* a bare `Enter`, then a 3s admission wait (`agent-doc-controller/src/dispatch.rs:3227-3233`). If it times out, `Ok(None)` falls through to the normal path, which sends the full trigger again (`agent-doc-route-io/src/dispatch.rs:829-861`, `:887-915`).
- *Supervisor inject:* a 10s effect timeout (`agent-doc-supervisor-io/src/ipc.rs:58`) reports failure after delivery. The dedupe key is in memory and is lost on supervisor re-exec (`agent-doc-start-runtime-io/src/lib.rs:2469-2503`).
- *Pass-through:* `Cleared` vs re-`Enter` is decided by a 150ms settle (`dispatch.rs:2937`).
- *Why it is a safety problem:* receivers have no idempotency key. Duplicate protection is screen observation within a time window.

**R9. Supervisor restart timeout escalates to a cold start, and the recycle TTL is treated as proof of loss.**

- *Restart:* a response timeout leads to a probe of a live socket and a `Failed` result, which `decide_supervisor_replacement_escalation` maps to `EscalateColdStart` (`rpc.rs:27193-27245`; `agent-doc-controller/src/supervisor_replacement.rs:190-193`). This contradicts "mutating commands must never be replayed merely because that receipt was late" (`agent-doc-supervisor-io/src/ipc.rs:63-69`) and risks a duplicate supervisor.
- *Recycle TTL:* `RECYCLE_INFLIGHT_SETTLE_TTL_SECS`=120 (`agent-doc-controller/src/dispatch.rs:2222`) is treated as proof the settle was lost. `RecycleSettleDispatch.tla:114-119` encodes exactly that.

**R10. Response delivery wake is one-shot, with no periodic pull.**

- *Liveness:* a failed `deliver_crdt_remote` is only logged (`agent-doc-crdt-relay-io/src/lib.rs:3563-3575`, `:5418-5428`). The CLI wait's `signal_immediately` is a no-op (`agent-doc-document-realtime-io/src/lib.rs:661`). JetBrains pulls only on events, and an idle no-op drain does not reschedule (`JB/CrdtReplicaManager.kt:1720-1730`).
- *Recovery is latched once:* the non-convergence recovery is latched on the first send (`agent-doc-document-realtime/src/crdt_relay.rs:2471-2484`). `NonconvergingReplicaDisposition::Retry`, which is what a send timeout yields, is never acted on outside tests (`agent-doc-crdt-relay-io/src/lib.rs:3761-3766`, `:5290`).
- *Result:* the response stays invisible until the operator interacts. Safety holds (it fails closed); liveness does not.

**R11. GH #136: layout admission depends on which lane published last.**

- *Retained columns:* focus escalation republishes whichever lane last published the "retained" columns (`rpc.rs:21312-21328`, `:21376-21436`). The focus and spanning lanes have no fence between them.
- *Fail-open gate:* the column gate admits the stale focused pane by design (`agent-doc-sync-io/src/layout_column_audit.rs:353-354`). It also admits `Unknown` freshness, and freshness reads `Unknown` during a supervisor re-exec window (`:151-156`, `:172-192`, `:554-557`).
- *No restart reload:* the desired layout is not reloaded from `state.db` when the controller restarts.

**R12. Missing deadlines that turn a wedged peer into an unbounded stall.** These are liveness problems, but R1 turns them into safety problems.

- `JB/CrdtReplicaForwarder.kt:1032-1048`: no read deadline. A wedged controller hangs the per-document worker that serialises pull, ACK, register and save (`JB/CrdtReplicaManager.kt:4232`).
- Controller and supervisor connect and write have no deadline (`rpc.rs:588-600`, `:650-652`, `:15340-15342`; `agent-doc-supervisor-io/src/ipc.rs:873-880`, `:969-972`).
- tmux subprocesses have no deadline (`agent-doc-tmux-io/src/lib.rs:192-215`).

**For contrast:** P5 (reliable-sync op push) and P8 (visible-write receipt) already
satisfy the target shape. Both re-derive from durable state, apply idempotently
by cursor or hash, and fail closed on timeout. They are the template for the
fixes below.

## 5. Candidate fixes: one-shot notifications on correctness paths

This table is the original `#netadv1` finding set. Each item names the missing
property at audit time; the `#netadv3` closure below records the fixes proved by
the subsequent NetChannel models and deterministic regressions.

| # | Site | Defect | Missing property |
|---|---|---|---|
| F1 | `JB/CpRouteClient.kt:1294-1335` → `rpc.rs:12919-12930`, `:4741-4786` | A focus timeout leads to a failed status probe, then SIGKILL of the live controller (R1) | Timeouts must never authorize a kill. Require positive evidence such as a generation-stamped wedge proof. |
| F2 | `JB/CrdtReplicaManager.kt:1245-1250` + `JB/PatchWatcher.kt:641-650` + `agent-doc-crdt-relay-io/src/lib.rs:3721-3726` | An attach timeout or coalesced re-register answers `rejected`, which counts as a definitive refusal (R2) | A typed "slow, still trying" receipt that is distinct from "refused" |
| F3 | `agent-doc-ipc-io/src/lib.rs:314-327` | Unlinks a live listener's socket on a failed connect (R3) | Use the round-trip `probe_endpoint` before evicting |
| F4 | `agent-doc-write-runtime-io/src/lib.rs:608-621` | The closeout owner claim is one-shot with a fresh `owner_id`, so a lost response orphans the lease (R4) | A caller-stable `owner_id` and a retry until acked |
| F5 | `agent-doc-write-runtime-io/src/lib.rs:655-679` | The heartbeat `break`s after one failed refresh (R4) | Retry the refresh; abort the closeout if the lease is actually lost |
| F6 | `rpc.rs:17111-17114` | A connect error reads as "no live editor" (R5) | Treat it as unknown and fail closed, like the timeout branch |
| F7 | `JB/ReliableSyncLivenessListener.kt:153-174` | Liveness `Close` is sent once, after the OR-set already removed the tags | Keep the Close in the frame ledger until acked (as path moves already do) |
| F8 | `JB/CrdtReplicaForwarder.kt:933-935`, `:483-489` | `replica_deregister` is fire-and-forget | Retry until acked, or have the controller re-derive membership from liveness |
| F9 | `agent-doc-crdt-relay-io/src/lib.rs:3563-3575`, `:5418-5428`; `agent-doc-document-realtime-io/src/lib.rs:661` | `deliver_crdt_remote` wake is one-shot, and the CLI's `signal_immediately` is a no-op (R10) | Re-send the wake until a projection ACK, or add a periodic editor pull while the hub has pending updates |
| F10 | `agent-doc-crdt-relay-io/src/lib.rs:3761-3766`, `:5290`; `crdt_relay.rs:2471-2484` | The `Retry` disposition is never acted on, and the recovery is latched once (R10) | Re-arm on `Retry` with backoff |
| F11 | `agent-doc-crdt-relay-io/src/lib.rs:3829-3838` | The rebootstrap `replace` flag is cleared on read, before the response is delivered | Clear it on the ACK, not the read |
| F12 | `JB/CrdtReplicaManager.kt:2642-2644`, `:2677-2679`, `:1868-1877` | A failed `disk_persisted` projection receipt only requests a drain, which never re-projects | Re-send the receipt itself |
| F13 | `agent-doc-controller-io/src/project_controller.rs:3784-3816`, `:5983-5992` | Retained native-save retry gives up after 8 refusals and waits for an edge an idle document never produces | Level-triggered retry while the retained intent exists |
| F14 | `rpc.rs:21728-21742`, `:21421-21434`, `:21848`; `agent-doc-editor-surface/src/lib.rs:429-436` | A surface observe returns `Ok` when the layout publish or escalation failed; the graph has already advanced, so a resend reads as `Idle` | Advance the graph on effect, or return a retryable receipt |
| F15 | `JB/EditorTabSyncListener.kt:1168-1181`; `rpc.rs:24360`; `agent-doc-editor-surface/src/lib.rs:383-398` | The focus lane has no retry and no read-back of `select-pane`; `focused_document` advances before the effect | Read back the active pane and keep the intent until it is observed |
| F16 | `agent-doc-ipc-io/src/lib.rs:854-862`, `:1619-1644` | A duplicate `observe_lazily_current` gets a synthetic `applied` before the original finishes | Have the duplicate wait for the original's receipt |
| F17 | `agent-doc-ipc-io/src/lib.rs:1074`, `:1140-1151` | Normal-path `reload_library` uses 6s, not 90s, and a timeout skips the cooldown record, which leads to a refusal storm | Use the 90s budget on the normal path, and record the cooldown on timeout too |
| F18 | `agent-doc-write-ipc-io/src/lib.rs:441-476`; `agent-doc-git-io/src/boundary_reposition.rs:65-69`, `:132-134` | Boundary reposition is one-shot but logged as "retained for retry"; nothing is persisted | Persist the intent, or fix the log |
| F19 | `agent-doc-route-io/src/dispatch.rs:829-861` | A stranded-draft admission timeout resends the full trigger (R8) | An idempotency key, or proof of non-admission before resending |
| F20 | `rpc.rs:27193-27245`; `agent-doc-controller/src/supervisor_replacement.rs:190-193` | A supervisor `restart` response timeout escalates to a cold start (R9) | Treat `ResponseTimeout` as "maybe accepted"; re-observe the generation |
| F21 | `agent-doc-controller-io/src/project_controller.rs:2797`, `:2921-2997`; `rpc.rs:3200-3240` | Async command admission lives only in memory, and `idempotency_key` is never read server-side | Durable admission keyed by `idempotency_key` |
| F22 | `JB/PatchWatcher.kt:122` | The patch dedupe TTL is 60s; a later redelivery relies on content heuristics | Durable or generation-scoped dedupe |
| F23 | `agent-doc-controller/src/paths.rs:15-17` | The controller socket path is not relocatable (Coder volumes) | Honour `AGENT_DOC_SOCKET_DIR` |

### `#netadv3` closure

- F4/F5: closeout claims reuse a caller-stable owner id and retry a lost claim;
  heartbeat transport loss retries, while an answered lease loss aborts before
  commit (`AgentDocCloseoutNet` plus deterministic write-runtime regressions).
- F9/F10/F13: delivery wake, recovery and retained-save paths are level-triggered
  and re-arm after lost/refused receipts (`VisibleDeliveryReceiptNet`).
- F14/F15: a failed editor-surface focus or structural publication retains the
  exact `SurfaceIntent`; the next matching observation reapplies it, and only a
  successful effect, an answered refusal, a document move or client retirement
  clears it (`PassiveTmuxSyncNetAdvanceFirstWedge`).
- SIM-F1/SIM-F2: lifecycle, heartbeat and queue-control level updates carry a
  monotonic send stamp. Reordered older updates in the same generation are
  rejected by the real handlers and by deterministic SimWorld traces
  (`LifecycleSequenceReorderWedge`).

## 6. Formal-model gap summary

The full table is in `formal/tla/README.md`. The original `#netadv1` gap was
closed for the selected hot paths by `#netadv3`: requests and receipts are split
over `NetChannel`, safety runs under the full adversary, and liveness runs under
fair loss with protocol resend fairness. Remaining gaps stay explicit in each
model rather than being hidden behind an atomic receipt.

- **Lossy channels:** the `*Net` hot-path models now cover loss, delay, duplication, reordering, stalls and reconnect cuts.
- **Believed refusals:** VisibleDeliveryReceipt, EditorReplicaStrand and TransientRefusalLatch believe every refusal. Neither R2 (timeout read as refusal) nor R3 (connect failure read as stale socket) can be expressed.
- **Timer-derived facts:** RecycleSettleDispatch and WaitMachine encode a timer firing as a fact (`Ttl`, `globalHangCeiling`). That conflicts with the target property.
- **First remodel targets for netadv3:**
  - VisibleDeliveryReceipt: R2, R10.
  - EditorReplicaStrand: R2, R3.
  - A new controller-liveness/self-heal model: R1.
  - Closeout lease: R4.
  - RecycleSettleDispatch and the supervisor restart: R9.
  - GH #136 layout admission: R11.

## Appendix: correctness-path timing constants

Values below 1s are localhost-tuned.

| Constant | Value | Site | Fires → |
|---|---|---|---|
| `EDITOR_FOCUS_OBSERVE_TIMEOUT_MS` | 1000 | `JB/CpRouteClient.kt:160` | controller self-heal / reap (R1) |
| `CRDT_AWAIT_ATTACH_TIMEOUT_MS` | 750 | `JB/CrdtReplicaManager.kt:151` | definitive refusal (R2) |
| `CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS` | 5000 | `JB/CrdtReplicaManager.kt:157` | definitive save refusal (R2) |
| `IPC_CONNECT_TIMEOUT_SECS` | 3 | `agent-doc-ipc-io/src/lib.rs:105` | socket unlink (R3) |
| `IPC_RECEIPT_TIMEOUT_SECS` | 6 | `agent-doc-ipc-io/src/lib.rs:89` | write wedge strike → recycle |
| `IPC_DEWEDGE_TIMEOUT_THRESHOLD` | 2 | `agent-doc-write-converge-io/src/lib.rs:4495` | supervisor recycle request |
| `EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD` | 3 | `agent-doc-ipc-protocol/src/lib.rs:567` | endpoint marked unregistered |
| `CONTROLLER_RPC_TIMEOUT` | 5s | `agent-doc-controller-io/src/project_controller.rs:88` | status fail → reap (R1) |
| `CONTROLLER_CRDT_REVISION_READ_TIMEOUT` | 750ms | `rpc.rs:45` | fail-closed "live" |
| `COMMAND_SUPERVISOR_FRESHNESS_TIMEOUT` | 250ms | `rpc.rs:40` | fail-open (liveness) |
| `CONTROLLER_CLOSEOUT_COORDINATION_TIMEOUT` | 15s | `rpc.rs:73` | orphaned claim (R4) |
| `CLOSEOUT_OWNER_LEASE_SECS` | 300 | `agent-doc-state-backbone/src/lib.rs:4370` | takeover |
| `SUPERVISOR_LEASE_GUARD_STALE_AFTER` | 60s | `agent-doc-controller-io/src/project_controller.rs:8944` | claim auto-force (R6) |
| `SYNC_LOCK_WAIT_BUDGET` | 3s | `agent-doc-sync/src/lib.rs:32` | unlocked sync (R7) |
| stranded-draft admission | 3s | `agent-doc-controller/src/dispatch.rs:3227-3233` | trigger resent (R8) |
| `PASS_THROUGH_STRANDED_DRAFT_SETTLE` | 150ms | `agent-doc-controller/src/dispatch.rs:2937` | Cleared vs re-Enter (R8) |
| `SUPERVISOR_IPC_EFFECT_RESPONSE_TIMEOUT` | 10s | `agent-doc-supervisor-io/src/ipc.rs:58` | inject/restart reported failed (R8, R9) |
| `RECYCLE_INFLIGHT_SETTLE_TTL_SECS` | 120 | `agent-doc-controller/src/dispatch.rs:2222` | settle presumed lost (R9) |
| `SUPERVISOR_RECYCLE_SETTLE_WAIT` | 10s | `agent-doc-controller-io/src/project_controller.rs:104` | single-shot settle verdict |
| `MAX_BARRIER_WAITS_WITHOUT_PROGRESS` | 12 × ~50ms slices | `agent-doc-document-realtime/src/crdt_relay.rs:192` | availability release |
| `MAX_REDELIVERIES_WITHOUT_ACK` | 50 | `agent-doc-document-realtime/src/crdt_relay.rs:169` | availability release |
| `DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS` | 400 | `agent-doc-crdt-relay-io/src/lib.rs:300` | fail closed (liveness) |
| `EDITOR_ROUTE_PLANE_CATCH_UP_MAX` | 500ms | `rpc.rs:11103-11133` | late layout frame loses to route |
| `VISIBLE_WRITE_RECEIPT_TIMEOUT_MS` | 6000 | `agent-doc-write-converge-io/src/lib.rs:84` | fail closed, retain |
| `CRDT_PROJECTION_OBSERVATION_TIMEOUT_MS` | 8000 | `agent-doc-document-realtime-io/src/lib.rs:155` | receipt refused → refusal count |
| `globalHangCeiling` | 10000ms | `formal/wait_machine/WaitMachine.lean:41` | IPC-ack wait fails closed |
