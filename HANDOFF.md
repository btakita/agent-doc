# netadv5 handoff

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

## Status

| R | State | Change | Test |
|---|---|---|---|
| R3 | done | `agent-doc-ipc-io`: `probe_listener_for_pid` → `Live/Absent/Unknown`; typed `ConnectTimedOut`; unlink only on refused/ENOENT + owner pid gone; `prune_stale_editor_sockets` uses it | `slow_listener_connect_timeout_never_unlinks_socket`, `connect_failure_classification_separates_slow_from_refused`, `real_connect_watchdog_error_is_typed_timeout` |
| R1 | done | `rpc.rs` `ensure_serving_controller` → `ensure_serving_controller_with` + `classify_serving_probe` (`Serving/NotBound/Unresponsive`); reap only on `NotBound` (ECONNREFUSED/ENOENT); timeout/reset → `controller_self_heal_deferred` + `CONTROLLER_BUSY_RETRY_LATER` error (FFI returns 0, JB rethrows, focus retried). No JB change needed. | `slow_status_receipt_never_reaps_a_busy_controller`, `refused_status_connect_is_positive_evidence_for_relaunch` |
| R2 | done | New receipt `{"status":"deferred"}` (`SocketReceiptClassification::Deferred`, `DEFERRED_RECEIPT_LINE`, `is_ipc_receipt_deferred_error`); FFI v2 code `3`; JB `APPLY_DEFERRED` for attach-await timeout, coalesced re-register, and persist document-lane timeout. Not counted by `is_ipc_receipt_rejected_error` → never `DropFromDeliveryCut`. JB 0.2.497. | ipc-io `deferred_receipt_from_a_slow_attach_is_not_a_definitive_refusal`; protocol `classify_socket_receipt_separates_deferred_from_rejected`; crdt-relay-io `a_deferred_receipt_from_a_slow_editor_is_never_a_definitive_answer`; JB `PatchWatcherDeferredReceiptTest` (gradle exit 0) |
| R5 | done | `rpc.rs` `reliable_sync_editor_live_for_file`: connect error → `editor_live_on_controller_connect_failure` (false only for refused/ENOENT **and** no same-project controller pid); permanent socket-path rejection → false. | `controller_connect_failure_is_not_proof_of_no_live_editor` |
| R6 | done (claim confirmed: no periodic supervisor heartbeat) | `status::supervisor_lease_holds_against_takeover`; `fresh_foreign_supervisor_lease_holds_document` holds while pid alive and its cmdline still names the document, regardless of heartbeat age. Other `supervisor_lease_is_fresh_and_alive` users (project_controller.rs:7886, :9036, :9272; rpc.rs:19883) left as-is (diagnostic/drain readiness, not destructive). | `idle_supervisor_with_stale_heartbeat_still_holds_against_takeover` |
| R7 | done | `agent_doc_sync::sync_lock_disposition` + `acquire_sync_lock_for_mode`: contended Full sync bails `sync_lock_contended_retry_later` (layout worker retries 250ms→5s); `Unavailable` (no lock possible for anyone) still proceeds; budget env `AGENT_DOC_SYNC_LOCK_WAIT_MS`. | `contended_full_sync_aborts_instead_of_running_unlocked`, `contended_full_sync_aborts_then_progresses_after_release` |
| R8 | in progress (+ netadv4 SIM-F3: ControllerRequest dispatch has no request id) | | |
| R9 | todo | | |
| RTT budgets | todo | | |

## Resume

`cd /home/brian/work/btakita/agent-doc-wt/netadv5`; continue the first `todo`
row. Do not push/install/release/merge.
