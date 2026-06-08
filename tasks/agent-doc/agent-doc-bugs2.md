---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260608-002609.md`*

Compacted content:
- Archived 1 response topic(s): Why is "stale sidecar content" a persistent issues with monsterrodholders.md? What is the "sidecar content"? Is there...
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: sidecar rearchitecture backlog — gpt-5

> **Queue prompt:**
>
> Add backlog items for the sidecar rearchitecture. I want to use a sqlitedb for the durable state. The canonical state should live in memory, ideally with lazily-rs, while the background durable state is written to the Sqlite db. Are using the unified Control Pane process?


Assuming "Control Pane" means the existing `Single Process Control Plane` contract: yes, that should be the target, but it is not fully landed yet. Current code already uses the project controller and `.agent-doc/state.db` for actor records; the rearchitecture should finish consolidating controller, session, store, and supervisor actors into one project-scoped control-plane process, with external CLIs/editors still talking to it over IPC.

I added backlog slices for the unified process, the SQLite write-through store, lazily-rs-backed in-memory authority evaluation, dispatch/session actors, projection cleanup, crash recovery, admin/backpressure, and deterministic SimWorld coverage. I kept them ordered so authority moves to the controller before obsolete sidecar writers are removed.
Implemented `#ctrlplane-process`.

> **Queue prompt:**
>
> do [#ctrlplane-process]


`agent-doc controller status` now reports an explicit `control_plane` snapshot for the existing project-scoped single-process controller. The JSON names the controller IPC boundary, `.agent-doc/state.db` as the durable state authority, compatibility projection output as the projection authority, and role snapshots for dispatch/store/session/supervisor/projection ownership. The active controller RPC path and inactive/offline status fallback both use the same status builder, and the new unit coverage exercises start/register/dispatch/status/projection facts through the controller path.

Updated specs:
- `specs/07-session-tmux-commands.md` documents the `control_plane` status field.
- `specs/08b-single-process-control-plane.md` documents the admin/status surface for this contract.

Verification:
- `cargo test -p agent-doc-orchestration controller_status_reports_single_process_control_plane_runtime`
- `cargo test -p agent-doc-orchestration project_controller::tests::`
- `make check`
- `cargo install --path .`

Commits/push:
- `src/agent-doc`: `a1abe8c7 controller: expose control-plane status`, pushed to `origin/main`.
- `agent-loop`: `ee4e9b30f agent-doc(agent-doc): update control-plane status pointer`, pushed to `origin/main`.

CI:
- `src/agent-doc` latest CI for `a1abe8c7` was `in_progress` at closeout (`27109791646`, 2026-06-08 00:41:17Z), so I did not wait on it.
- `agent-loop` has no workflow named `CI`.
Implemented `#ctrlplane-storeactor`.

The controller SQLite state layer now explicitly covers the remaining store-actor durable fact families. `agent-doc-sqlite::state_store` initializes WAL mode and owns schema/API helpers for `queue_heads`, `document_cycles`, `admin_operations`, and `crash_recovery_markers`, alongside the existing actor documents, lifecycle transitions, supervisor leases, dispatch attempts, projection diagnostics, and layout state. The status path now reads aggregate counts through `load_control_plane_store_counts`, and `agent-doc controller status` reports per-role `categories` maps so the store actor snapshot exposes the durable families it owns.

Updated specs:
- `specs/07-session-tmux-commands.md` documents category counts in `control_plane` role snapshots.
- `specs/08b-single-process-control-plane.md` documents the store actor's per-category SQLite status counts.

Verification:
- `cargo test -p agent-doc-sqlite control_plane_store_counts_extended_fact_categories`
- `cargo test -p agent-doc-orchestration controller_status_reports_single_process_control_plane_runtime`
- `make check`
- `cargo install --path .`

Commits/push:
- `src/agent-doc`: `91da5c42 controller: extend sqlite store actor state`, pushed to `origin/main`.
- `agent-loop`: `015cc4d84 agent-doc(agent-doc): update store actor pointer`, pushed to `origin/main`.

CI:
- `src/agent-doc` latest CI for `91da5c42` was `in_progress` at closeout (`27110075916`, 2026-06-08 00:52:49Z), so I did not wait on it.
Implemented `#ctrlplane-memoryauth`.

> **Queue prompt:**
>
> :round_pushpin: do [#ctrlplane-memoryauth]


The active project controller now owns a `ControllerRuntime` with a live in-memory actor map loaded from `.agent-doc/state.db` at startup. Controller IPC actor reads use that memory snapshot for actor binding, dispatch authorization, supervisor registration/heartbeat, lifecycle checks, operator commands, and attach handling. Accepted actor mutations remain SQLite write-through: the handler commits through the existing `store_actor_record` transaction first, then refreshes the runtime memory map before returning the accepted result. Failed commits therefore do not advance memory.

`agent-doc controller status` now exposes the active session-actor memory authority through `control_plane.session_actors.categories`, including `actor_records`, `map_backend_std_btree_map`, and `write_through_sqlite`. The specs document the current `BTreeMap` baseline and the acceptance criteria for evaluating `lazily-rs` later: it can replace the map/index layer only if it preserves the same post-commit memory update and failed-commit-no-memory-change contract.

Verification:
- `cargo test -p agent-doc-orchestration controller_runtime_refreshes_memory_after_write_through_commit`
- `cargo test -p agent-doc-orchestration controller_status_reports_single_process_control_plane_runtime`
- `cargo test -p agent-doc-orchestration project_controller::tests::`
- `make check`
- `cargo install --path .`

Commits/push:
- `src/agent-doc`: `ac213a76 controller: make actor state memory authoritative`, pushed to `origin/main`.
- `agent-loop`: `5d223ff1c agent-doc(agent-doc): update memory authority pointer`, pushed to `origin/main`.

CI:
- `src/agent-doc` latest CI for `ac213a76` was `in_progress` at closeout (`27110402938`, 2026-06-08 01:05:26Z), so I did not wait on it.
Yes, if the goal is a real Codex context reset, the turn needs to stop first.

> **Queue prompt:**
>
> :pushpin: Codex sessions are getting `'/clear' is disabled while a task is in progress.` in between queue items. Do we need to stop the turn, run `/clear`, then restart the queue?


`/clear` is disabled while Codex is still executing an active task, so it cannot be used inside the owner-pane continuation between two queue heads. The clean flow is: finish the current queue item, persist the response with `agent-doc finalize` or `agent-doc write --commit`, let the turn end after `session-check --codex-final-gate`, run `/clear` or start a fresh Codex session, then resume the queue from the new context.

If the harness immediately feeds the next queue head into the same active Codex turn, there is no safe point for `/clear` yet. In that case the options are either continue in the same pane, or change the queue driver so it intentionally stops after closeout when a context reset is desired.

Changed paths: none for this prompt response.

Commands/proof:
- `agent-doc preflight tasks/agent-doc/agent-doc-bugs2.md` recovered and committed the previous `#ctrlplane-memoryauth` response.
- `agent-doc session-check tasks/agent-doc/agent-doc-bugs2.md` confirmed this `/clear` prompt was the next active queue head.

Verification: not applicable; no code changes.

Push: pending root document commit push after this closeout.
Implemented `#ctrlplane-dispatchapi`.

> **Queue prompt:**
>
> :round_pushpin: do [#ctrlplane-dispatchapi]


The project controller now returns typed durable dispatch receipts while keeping existing authorization callers compatible. `DispatchAuthorization` still exposes `accepted_stage`, but now also includes a `receipt` with `receipt_id`, typed result status, stage, proof scope, and `dispatch_start_proven`. Dispatch and operator-command rejections now persist a receipt before returning the rejection, so stale session/pane/generation and blocked/closed actor decisions are durable too.

The SQLite store now records receipt metadata on `dispatch_attempts` (`result_status`, `proof_scope`, `dispatch_start_proven`) and returns the inserted row id. Existing DBs are migrated in place by `initialize_state_db`. `SessionOperatorStatus.dispatch_attempts` now reports the receipt id and proof metadata.

I also added the controller `admin_operation` API as the durable receipt target for admin-class commands as they migrate toward the controller path. It records operation kind, optional document id, typed status, and diagnostic payload in `admin_operations`, then returns a typed admin receipt.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`
- `src/agent-doc/agent-doc-sqlite/src/state_store.rs`
- `src/agent-doc/specs/08b-single-process-control-plane.md`

Verification:
- `cargo test -p agent-doc-orchestration project_controller::tests::`
- `cargo test -p agent-doc-sqlite`
- `make check`
- `cargo install --path .`

Commits/push:
- `src/agent-doc`: `3978a949 controller: return durable dispatch receipts`, pushed to `origin/main`.
- `agent-loop`: `f7064b72e agent-doc(agent-doc): update dispatch receipt pointer`, pushed to `origin/main`.

CI:
- Previous `src/agent-doc` CI was green before closeout.
- New `src/agent-doc` CI for `3978a949` is `in_progress` (`27110969213`, started 2026-06-08 01:27:01Z), so I did not wait on it.

Confidence: high. Escalation: none.
<!-- agent:boundary:a75563a2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#ctrlplane-process]~~
- ~~:round_pushpin: do [#ctrlplane-memoryauth]~~
- ~~:pushpin: Codex sessions are getting `'/clear' is disabled while a task is in progress.` in between queue items. Do we need to stop the turn, run `/clear`, then restart the queue?~~
- ~~:round_pushpin: do [#ctrlplane-dispatchapi]~~
- :round_pushpin: do [#ctrlplane-sessionactor]
- :round_pushpin: do [#ctrlplane-projections]
- :round_pushpin: do [#ctrlplane-crashrecover]
- :round_pushpin: do [#ctrlplane-adminbp]
- :round_pushpin: do [#ctrlplane-simworld]
- ~~do [#ctrlplane-storeactor]~~
<!-- /agent:queue -->
###
<!--
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [x] [#ctrlplane-dispatchapi] #agent-doc-bug Route `route`, `start`, `sync`, `session clear/restart/status`, `repair`, `preflight`, and `write/finalize` through typed controller dispatch/admin APIs that return durable receipts and distinguish accepted-only pane delivery from dispatch-start proof.
- [ ] [#ctrlplane-sessionactor] #agent-doc-bug Move per-document queue-head selection, response cycle state, pending mutations, and `do #id` backlog completion into the session actor so response closeout and tracked backlog mutation commit in the same controller transaction.
- [ ] [#ctrlplane-projections] #agent-doc-bug Convert `session-actors.json`, `sessions.json`, layout JSON, session logs, and ops projections into compatibility outputs only; record projection lag/failure diagnostics in SQLite and remove normal-path write authority from sidecar files once controller rows exist.
- [ ] [#ctrlplane-crashrecover] #agent-doc-bug Implement control-plane crash/restart recovery: rebuild the in-memory actor map from SQLite, reconcile open dispatch receipts by side-effect proof, reattach fresh supervisor leases, emit missing projections, and leave queue/backlog unadvanced when closeout did not commit.
- [ ] [#ctrlplane-adminbp] #agent-doc-bug Add controller-backed admin and backpressure controls for actor inspection, queue pause/resume/drain, stale actor reap, manual handoff with observed generation, projection repair, and typed full-queue rejections.
- [ ] [#ctrlplane-simworld] #agent-doc-bug Add deterministic SimWorld and CLI integration coverage for the control plane: concurrent route/start/sync/clear races, stale session/pane/generation rejection, busy queue promotion, closeout plus backlog completion, crash before/after commit, projection failure, supervisor heartbeat reattach, admin handoff/reap, and Codex/Claude/OpenCode proof differences.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
