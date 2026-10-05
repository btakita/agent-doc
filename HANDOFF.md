# HANDOFF — netadv1 (channel-assumption audit)

Item: `#netadv1` (agent-doc). Plan: `/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`.
Worktree: `/home/brian/work/btakita/agent-doc-wt/netadv1`, branch `netadv1`. Do NOT push/install/release/merge.

## Goal
Channel-assumption audit for Coder remote workspace + Zscaler (high/variable latency,
silent half-open stalls, lost messages):
1. Per formal model (formal/tla/*.tla, formal/*/ Lean): assumption per cross-process
   interaction (atomic / reliable FIFO / reliable unordered / lossy); Drop/Duplicate/Reorder/Reconnect modelled?
2. Per IPC protocol (agent-doc-ipc-protocol, -ipc-io, -reliable-sync-io, -write-ipc-io,
   controller rpc, JetBrains plugin IPC, tmux): transport, retry-until-ack vs one-shot,
   receiver idempotency, heartbeat/half-open detection, reconnect resync source,
   correctness-path timeouts (value + file:line), serial round trips on hot paths.
3. Rank protocols whose SAFETY depends on timing/reliable delivery, worst first.

Outputs: "Network channel assumptions" table section in `formal/tla/README.md`;
ranked findings in `docs/reference/network-channel-audit.md` (+ link in `docs/SUMMARY.md`);
list of one-shot-notification defects as candidate fixes (report only, don't fix).

## Done
- Read plan and formal/tla/README.md. 24 TLA models, Lean in formal/authority_ladder,
  formal/capture_closeout, formal/wait_machine.
- Collected 122 Rust timing constants: `formal/tla/.netadv1-timing-constants.txt`
  (scratch; delete before final commit, or fold into the doc appendix).
- Launched 4 read-only research agents (TLA/Lean models; core IPC crates + controller rpc;
  reliable-sync/crdt-relay/JetBrains; tmux/focus/layout/supervisor). Results pending.

## Remaining
1. Collect agent results (or redo research if this session ends first).
2. Write README table section.
3. Write docs/reference/network-channel-audit.md: per-protocol table, ranked risks, one-shot defect list.
4. Add SUMMARY.md link; run `make audit-docs` if cheap.
5. Commit; update this file; report branch, sha, top-10 risks, defect list.

## Findings so far (raw, unverified for safety impact)
Localhost-tuned timeouts on likely correctness paths:
- agent-doc-write-converge-io/src/lib.rs:84 VISIBLE_WRITE_RECEIPT_TIMEOUT_MS = 6_000 (100 in test cfg :82); :4495 IPC_DEWEDGE_TIMEOUT_THRESHOLD = 2
- agent-doc-ipc-io/src/lib.rs:89 IPC_RECEIPT_TIMEOUT_SECS = 6; :105 IPC_CONNECT_TIMEOUT_SECS = 3; :110 IPC_LISTENER_READ_TIMEOUT_SECS = 30
- agent-doc-crdt-relay-io/src/lib.rs:300 DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS = 400; :289 DOCUMENT_MODEL_ENSURE_TIMEOUT_MS = 5_000
- agent-doc-controller-io/src/project_controller.rs:102 CONTROLLER_RPC_TIMEOUT = 2s (5s variant :88); :104 SUPERVISOR_RECYCLE_SETTLE_WAIT = 10s
- agent-doc-controller-io/src/project_controller/rpc.rs:40 COMMAND_SUPERVISOR_FRESHNESS_TIMEOUT = 250ms; :45 CONTROLLER_CRDT_REVISION_READ_TIMEOUT = 750ms; :758-761 handoff settle 6x200ms
- agent-doc-sync/src/lib.rs:14-34 many 100-1000ms sync budgets (SYNC_OWNERSHIP_PROOF_BUDGET 750ms, SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET 250ms)
- agent-doc-sync-io/src/resync.rs:116-117 PROCESS_GRACE 4 x 75ms
- agent-doc-route-io/src/document_write.rs:8 RETAINED_ROUTE_WRITE_TIMEOUT 30s
- agent-doc-controller/src/dispatch.rs:2937 PASS_THROUGH_STRANDED_DRAFT_SETTLE 150ms
- src/session_actor_cmd.rs:35 CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT 900ms
- agent-doc-supervisor-io/src/ipc.rs:57-59 query 2s / effect 10s / accept read 5s
- JetBrains: CrdtReplicaManager.kt:151 CRDT_AWAIT_ATTACH_TIMEOUT_MS 750; CpRouteClient.kt:160 EDITOR_FOCUS_OBSERVE_TIMEOUT_MS 1000; SyncLayoutAction.kt:745 OBSERVED_LAYOUT_HEARTBEAT 50
Whether each timeout changes a safety decision is NOT yet established.
- docs/reference/ipc.md claims "Retries resume the same intent ... receipt replay is idempotent" — verify against code.

## Commands / status
- Navigation: `rtk proxy rg -n ...` (plain grep is aliased; rtk rg filter drops flags).
- No builds/tests run. Nothing pushed.
