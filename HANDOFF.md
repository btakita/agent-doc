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
1. Collect code-protocol agent results (3 pending: core IPC+controller rpc; reliable-sync/crdt-relay/JetBrains; tmux/focus/layout/supervisor). Redo if the session ended.
2. DONE (f57d681b7): README table section `## Network channel assumptions` in formal/tla/README.md.
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

## Milestone: core IPC + controller RPC agent results (verified D1-D3 by reading code)
All transports are AF_UNIX NDJSON, one connection per request; no TCP, no heartbeat frames.
Over Coder, latency appears as slow peers (editor UI thread, controller), not socket RTT.
Candidate one-shot / loss-sensitive defects:
- D1 agent-doc-ipc-io/src/lib.rs:314-327 is_listener_active_for_pid unlinks a LIVE editor socket on one failed/slow connect (3s watchdog :105, :349-363). Called on the hot delivery path (agent-doc-write-ipc-io/src/transport.rs:518). The listener itself calls an unlinked name "dead for good" (:1354-1360). Listener-start uses the round-trip probe_endpoint (:398-440); this path does not.
- D2 agent-doc-controller-io/src/project_controller/rpc.rs:17111-17114 controller connect error → "no live editor" (false), but a timeout → true (:17123-17132). Can select detached_disk_authority (agent-doc-write-runtime-io/src/run_entry.rs:2564-2577).
- D3 agent-doc-write-runtime-io/src/lib.rs:608-621 closeout owner claim is one-shot, and a lost response orphans the 300s lease (rpc.rs:5301-5357, CLOSEOUT_OWNER_LEASE_SECS state-backbone/src/lib.rs:4370). Heartbeat :661-679: one failed refresh breaks the loop permanently while the closeout continues.
- D4 agent-doc-ipc-io/src/lib.rs:854-862,1619-1644 a duplicate observe_lazily_current gets a synthetic `applied` before the original finishes, which defeats the barrier (:986-989).
- D5 rpc.rs:27193-27245 + agent-doc-controller/src/supervisor_replacement.rs:190-193 a supervisor restart response timeout can escalate to a cold start (duplicate supervisor). Contradicts supervisor-io/src/ipc.rs:63-69.
- D6 agent-doc-ipc-io/src/lib.rs:1074 normal reload_library uses the 6s budget, not 90s (:99 is legacy-only, :1080-1085). A timeout skips the cooldown record (:1140-1151), giving a refusal storm.
- D7 agent-doc-write-ipc-io/src/lib.rs:441-476 boundary reposition is one-shot but logged "retained for retry" (agent-doc-git-io/src/boundary_reposition.rs:132-134); nothing is persisted.
- D8 project_controller.rs:2797,2921-2997 async editor command admission is in-memory (5 min TTL), and the idempotency_key (rpc.rs:3200-3240) is never read server-side.
- D9 rpc.rs:588-600,650-652,15340-15342; supervisor-io/src/ipc.rs:873-880,969-972 controller/supervisor connect + write are unbounded (no watchdog, no send timeout).
- D10 rpc.rs:996,1026-1042,916-924 Rust authority-stream client has no read deadline and stops permanently after 2 disconnects (no callers; latent).
Response delivery happy path ≈ 2 editor connects + hello RT + accepted/applied + ≥6 serial controller RPCs (transport.rs:354-1090).
Closeout claim 1 RPC (15s), heartbeat every 100s, release 1 RPC. Focus observe 1 RT (1s, CpRouteClient.kt:160) + up to 2 on timeout. editor_route submit+await 2 RT.
