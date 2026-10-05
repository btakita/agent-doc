# netadv3 handoff

Task `#netadv3` (plan: `tasks/agent-doc/plan-network-adversarial-correctness.md`
in agent-loop). Branch `netadv3`, based on `batch-0505` (gh136 + netadv1 + netadv2
merged). Do NOT push, install, release or merge.

Scope: re-check the hot-path TLA+ models over `formal/tla/NetChannel.tla` and fix
LOST-MESSAGE / one-shot defects (audit `docs/reference/network-channel-audit.md`,
F1-F23). Out of scope (sibling `netadv5`): timeout-as-verdict R1, R2, R3, R5, R7,
R8, R9 (F1, F2, F3, F6, F19, F20) and round-trip budget tests.

## Status

- [ ] 1. Port StaleColumnRecycle's plain-set channel onto NetChannel
- [ ] 2. Net models: VisibleDeliveryReceipt, EditorReplicaStrand, RecycleSettleDispatch, AgentDocCloseout, PassiveTmuxSync
- [ ] 3. Code fixes for counterexamples (+ regression + Wedge cfg each)
- [ ] 4. `make tla` + `make check` green, exit status captured

## Plan / ids

Code-defect ids used here: audit F1-F23/R1-R12; netadv4 SimWorld findings
SIM-F1 (lifecycle/heartbeat last-arrival-wins), SIM-F2 (queue control
last-arrival-wins); new ones found by these models: ERS-1, RSD-1, VDRN-post-ack.

Models (formal/tla, run by scripts/run_tla.sh):
- StaleColumnRecycle: ported onto NetChannel (in place).
- VisibleDeliveryReceiptNet: wedges F9 (WakeOneShot), F10 (RecoveryLatch),
  F13/F12 (SaveOneShot), StaleAck (receipt keying), TimeoutRefusal (R2, netadv5).
- EditorReplicaStrandNet: wedges ERS-1 (Latch), GiveUpRefusal (R2/R3, netadv5).
- TODO: RecycleSettleDispatchNet (RSD-1 unreachable-controller read as refusal;
  R9 TTL-as-proof wedge, netadv5), AgentDocCloseoutNet (F4/F5 lease),
  PassiveTmuxSyncNet (F14/F15 focus graph advances before effect),
  LifecycleSequence (SIM-F1/F2 seq fence).

Code fixes planned (in order): F10, F9, F13, ERS-1, RSD-1, F4/F5, SIM-F1/F2
(merge netadv4 first), then F11, F16, F17, F18, F14/F15, F21 if time allows.

## Log

- (start) HANDOFF created.
- StaleColumnRecycle ported (positive 481k states, 40s); all 7 wedge/reach cfgs violate as required.
- VisibleDeliveryReceiptNet + 8 cfgs; TLC: positive + safety pass, all wedges/reach violate.
- EditorReplicaStrandNet + 4 cfgs; TLC: positive passes; Latch wedge trace = budget whose
  receipts were all dropped -> Exhaust latches -> stuck (ERS-1).

## Remaining items

(filled in as work proceeds)

---

## Merged sibling handoff: netadv5 (verbatim at dfbfcb68c)

## netadv5 handoff

Branch `netadv5` (based on `batch-0505`). Scope: a timeout is never a verdict;
hot-path round trips are budgeted. Source of the R-ids:
`docs/reference/network-channel-audit.md` §4. Plan:
`agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`.

Principle: a timeout yields "unknown / retry later", never "dead / refused /
absent". Destructive action needs positive evidence (pid gone, ECONNREFUSED on
a socket whose owner pid is dead, an explicit refusal message).

Sibling `netadv3` owns TLA ports + one-shot/lost-message F-items. Overlap
note: F1/F2/F3/F6/F19/F20 are the code halves of R1/R2/R3/R5/R8/R9, which the
coordinator assigned here.

### Status

| R | State | Change | Test |
|---|---|---|---|
| R3 | done | `agent-doc-ipc-io`: `probe_listener_for_pid` → `Live/Absent/Unknown`; typed `ConnectTimedOut`; unlink only on refused/ENOENT + owner pid gone; `prune_stale_editor_sockets` uses it | `slow_listener_connect_timeout_never_unlinks_socket`, `connect_failure_classification_separates_slow_from_refused`, `real_connect_watchdog_error_is_typed_timeout` |
| R1 | done | `rpc.rs` `ensure_serving_controller` → `ensure_serving_controller_with` + `classify_serving_probe` (`Serving/NotBound/Unresponsive`); reap only on `NotBound` (ECONNREFUSED/ENOENT); timeout/reset → `controller_self_heal_deferred` + `CONTROLLER_BUSY_RETRY_LATER` error (FFI returns 0, JB rethrows, focus retried). No JB change needed. | `slow_status_receipt_never_reaps_a_busy_controller`, `refused_status_connect_is_positive_evidence_for_relaunch` |
| R2 | done | New receipt `{"status":"deferred"}` (`SocketReceiptClassification::Deferred`, `DEFERRED_RECEIPT_LINE`, `is_ipc_receipt_deferred_error`); FFI v2 code `3`; JB `APPLY_DEFERRED` for attach-await timeout, coalesced re-register, and persist document-lane timeout. Not counted by `is_ipc_receipt_rejected_error` → never `DropFromDeliveryCut`. JB 0.2.497. | ipc-io `deferred_receipt_from_a_slow_attach_is_not_a_definitive_refusal`; protocol `classify_socket_receipt_separates_deferred_from_rejected`; crdt-relay-io `a_deferred_receipt_from_a_slow_editor_is_never_a_definitive_answer`; JB `PatchWatcherDeferredReceiptTest` (gradle exit 0) |
| R5 | done | `rpc.rs` `reliable_sync_editor_live_for_file`: connect error → `editor_live_on_controller_connect_failure` (false only for refused/ENOENT **and** no same-project controller pid); permanent socket-path rejection → false. | `controller_connect_failure_is_not_proof_of_no_live_editor` |
| R6 | done (claim confirmed: no periodic supervisor heartbeat) | `status::supervisor_lease_holds_against_takeover`; `fresh_foreign_supervisor_lease_holds_document` holds while pid alive and its cmdline still names the document, regardless of heartbeat age. Other `supervisor_lease_is_fresh_and_alive` users (project_controller.rs:7886, :9036, :9272; rpc.rs:19883) left as-is (diagnostic/drain readiness, not destructive). | `idle_supervisor_with_stale_heartbeat_still_holds_against_takeover` |
| R7 | done | `agent_doc_sync::sync_lock_disposition` + `acquire_sync_lock_for_mode`: contended Full sync bails `sync_lock_contended_retry_later` (layout worker retries 250ms→5s); `Unavailable` (no lock possible for anyone) still proceeds; budget env `AGENT_DOC_SYNC_LOCK_WAIT_MS`. | `contended_full_sync_aborts_instead_of_running_unlocked`, `contended_full_sync_aborts_then_progresses_after_release` |
| R8 | done (+ netadv4 SIM-F3) | (a) `authorize_dispatch` mints `dispatch_request_key` (diagnostic payload field) once; all retries reuse it; `handle_dispatch` answers an applied key from new state.db table `dispatch_request_keys` (first writer wins, 7d retention) with the original `DispatchAuthorization`. SimWorld models the key (net send id) via production `dispatch_request_admission`; netadv4 F3 test flipped (`netadv4_f3_retransmitted_dispatch_after_proof_injects_once`), F3 removed from `KNOWN_OPEN_NET_FINDINGS`. (b) stranded-draft admission timeout re-observes the composer: draft gone → `TransportSubmittedOnly`, still there → retryable error; never re-sends the trigger (`stranded_draft_unobserved_admission_followup`). (c) supervisor inject admission key persisted at `.agent-doc/supervisor/inject-admission-<session>.key` (survives re-exec; cleared on Ready/failure/retry-proof). (d) pass-through settle configurable `AGENT_DOC_PASS_THROUGH_SETTLE_MS`. | `retransmitted_dispatch_request_is_answered_not_reinjected`, `dispatch_request_key_records_first_outcome_durably`, `dispatch_request_key_round_trips_and_never_rekeys`, `stranded_draft_admission_timeout_never_authorizes_a_second_trigger`, `inject_dedupe_survives_supervisor_reexec`, `pass_through_settle_is_configurable_for_slow_hosts`, sim-net corpus green with F3 un-whitelisted |
| R9 | done | Restart: `SupervisorReplacementIpcOutcome::ResponseTimedOut` (typed `ResponseTimeout` on a live socket) → `AwaitAcceptedInPlace` / force `WaitThenEscalate`; never `EscalateColdStart`. Recycle TTL: `recycle_inflight_unsettled_verdict_with_owner` — past TTL with a live doc supervisor → `RefuseOwnerStillRecycling` (retryable), abandonment only when no supervisor process. `RecycleSettleDispatch.tla` still encodes TTL-as-fact (netadv3 owns TLA). | `late_restart_receipt_never_escalates_to_cold_start`, `late_supervisor_restart_receipt_is_maybe_accepted`, `recycle_ttl_elapsed_with_live_supervisor_is_not_abandonment` |
| RTT budgets | done (partial, see Remaining) | Per-thread probe `controller_round_trips_on_this_thread` at both controller request funnels (`request_path_json`, `request_controller_on_stream_with_timeout`). Closeout claim/release trimmed 3→2 RTT (removed redundant `ensure_controller_running`; same `connect_or_launch` already runs in the request). `EDITOR_INTENT_SERIAL_ROUND_TRIPS = 2`. Audit §3 closeout ACK chain traced (floor ≈26 serial RTT, realistically 50+) and focus projection (1 i3-msg + 14-15 tmux subprocesses). | `closeout_owner_claim_and_release_stay_within_round_trip_budget` (feature test-support), `editor_intent_costs_two_serial_round_trips` |

### Verification

- `make check` EXIT=0 on a8c15748e (nextest 11064 passed / 267 skipped; clippy, sim-medium, sim-net, editor-parity, audit-docs, tla, lean all green).
- JetBrains: `./gradlew --no-daemon --console=plain test` exit 0 (0.2.497), `PatchWatcherDeferredReceiptTest` 2/2.

### Remaining (concrete)

1. Focus-projection RTT test: `handle_focus_document_pane_with_policy` (rpc.rs ~:24713) takes a concrete `tmux_router::Tmux`; make it accept a `TmuxCommandRunner` so `RecordingRunner` (tmux-io :831-885) can assert the 14-15 subprocess budget; then collapse the 3× `list-panes -a` and 2× `display-message #{window_id}` with a pane-snapshot scope.
2. Count the second controller funnel `agent_doc_state_wire::send_ndjson_request_to_actor` (state-wire lib.rs:249) in the probe; then assert an end-to-end closeout budget.
3. Ack-driven replacements still open: native-save fallback 25→250ms sleep loop (document-realtime-io lib.rs:1825) and CRDT write-frontier backoff (:3999) → wake on the projection subscription.
4. R1 JetBrains side: `EDITOR_FOCUS_OBSERVE_TIMEOUT_MS = 1000` still triggers `agent_doc_ensure_controller_running`; it is now harmless (Rust defers on a late status) but could be RTT-derived.
5. R6: other `supervisor_lease_is_fresh_and_alive` callers (project_controller.rs:7886, :9036, :9272; rpc.rs:19883) still read heartbeat age; non-destructive today. A periodic supervisor heartbeat would make freshness meaningful.
6. R9: `formal/tla/RecycleSettleDispatch.tla` still encodes TTL-as-fact (netadv3).
7. R4 (closeout claim one-shot / heartbeat stops after one failure) not in this scope (netadv3 F4/F5).

### Resume

`cd /home/brian/work/btakita/agent-doc-wt/netadv5`; continue the first `todo`
row. Do not push/install/release/merge.
