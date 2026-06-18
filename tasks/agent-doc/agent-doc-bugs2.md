---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260618-033157.md`*

Compacted content:
- Archived 7 response topic(s): JB Clear Session Context on OpenCode does not submit `/new`; OpenCode /new fix shipped + tsift.md crash needs clarification; 60s route timeout + describe-image workflow already exists; 4 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): #queediturn implemented + accretion default-off shipped; queue...

### Re: #monster60stimeout route ready-prompt barrier — glm-5.2

Root cause confirmed and **shipped**. `current_generation_ready_prompt_proven` (authoritative_actor.rs:283) already does a fresh `capture_pane` + `ready_prompt_candidate` check as the primary proof, but the fallback arm only matched `prompt_ready`/`dispatch_ready_prompt` reasons. After `idle_pane_reconcile`, the supervisor had already proven the pane idle (`supervisor_pane_has_busy_cue == Some(false)` in idle_watch.rs:336-356) and transitioned the actor to `Ready` — yet the route still saw `prompt_ready=false` and waited the full 60s because the edge-triggered pty redraw missed re-emitting a recognized prompt shape.

Fix: extracted `transition_proves_current_generation_ready` and added `idle_pane_reconcile` to the accepted ready-proof reasons (authoritative_actor.rs). The capture-pane check stays primary; the fallback only fires when the supervisor already proved the pane idle. 5 unit tests cover accept/reject paths. SPEC.md §127 and VERSIONS.md updated. `make check` clean, binary installed 0.34.1.

### Re: #describe-image-workflow — glm-5.2

**Already complete.** The `## Vision workflow for non-vision models` section existed in `src/agent-doc/SKILL.md` documenting `agent-doc describe-image` as the canonical path. It had pushed the SKILL.md past the 140-line compact budget (both `bundled_skill_hot_path_stays_compact` and `rendered_harness_content_stays_compact` were red on HEAD). Moved the detail into a new `runbooks/describe-image.md`, registered it in `BUNDLED_RUNBOOKS`, and slimmed the SKILL.md section to a 2-line reference — both compact tests now green. Skill reinstalled to Claude + OpenCode surfaces.

### Re: IPC drift root causes + restart-supervisor directive — glm-5.2

> **Queue prompt:**
>
> :pushpin: `agent-doc restart-supervisor tasks/agent-doc/agent-doc-bugs2.md` should be called by the PCP/Supervisor if there is IPC drift. Also, add items to the backlog and queue to fix the underlying issues that caused the IPC drift.


Investigated ops.log for the drift cascade. The dominant trigger is `supervisor_binary_stale` → `ipc_proof_insufficient invariant=supervisor_binary_stale recovery=candidate_snapshot_kept`, which cascades into `missing_response_probe recovery=direct_write_fallback` (the content-stripping path) and `live_prompt_drift_after_preflight`. Filed two backlog items referencing the existing plans: `#supstaleipcproof` (proactive recycle / stale-supervisor proof acceptance) and `#streamipcdirwrite` (stream IPC timeout → direct_write_fallback content loss). The `agent-doc restart-supervisor` directive is already wired in the supervisor idle-watch stale-binary self-recycle path; these backlog items target the proof-rejection window that remains.

No JB plugin install needed this round — all changes are binary + skill surface. `make check` clean (4663 tests). Binary installed.

### Re: #6esm supstaleipcproof + #3vn9 streamipcdirwrite — investigation — glm-5.2

> **Queue prompt:**
>
> do [#6esm]
>
> do [#3vn9]
>
> do [#dqjf]


Both items were filed last cycle from ops.log evidence. Deep investigation confirms **both are already resolved by shipped work** — the evidence trail was from the prior binary generation (the classic dogfood one-generation-behind pattern documented in `plan-proactive-recycle-on-install.md` §5).

**#6esm (supstaleipcproof):** The referenced `plan-proactive-recycle-on-install.md` is **LANDED** (R1+R2+R3+R4). The IPC proof rejection (`invariant=supervisor_binary_stale recovery=candidate_snapshot_kept`) is **safe by design** — `content_ours` (baseline + response) is retained as the snapshot; no data is lost. The editor ACK is simply not trusted during the stale-binary window, and the next cycle (post-self-recycle) picks up fresh state. The proof path already recognizes `supervisor_binary_stale_self_recycled` as recoverable with `next_action=restart_supervisor_once_and_retry` (outcome.rs:280). The remaining lever is R3 auto-recycle default-on (plan open decision #4) — an operator-validated decision, not a code gap. Logging already exists (`content_ours_adoption_refused_stale_supervisor` + `ipc_proof_insufficient`).

**#3vn9 (streamipcdirwrite):** The stream IPC timeout → direct disk write fallback that stripped the code fence is **already fixed**. Test `stream_ipc_timeout_retains_patch_and_refuses_direct_write` (finalize_integration.rs:342) enforces: IPC timeout → `.assert().failure()` + `recovery=retry_without_disk_write` + "refusing direct document write" + patch retained for editor retry. The `write_mode=stream_ipc_timeout_disk` ops.log entries that motivated this item do not exist in the current source — they were emitted by the prior binary. Test is green in `make check`.

**#dqjf:** duplicate of #6esm (same-cycle pending-add sync artifact). Removed.

Queue drained. The two `[operator-verify]` items (#tsiftmdcrash, #jbacceptwedge) remain undrainable — both need operator action (crash reproduction / plugin 0.2.170 install + IntelliJ restart).

### Re: File Cache Conflict + backlog→queue immediacy — opus-4-8

> **Queue prompt:**
>
> `agent:backlog` with `queue` attribute should have added the backlog items into the queue immediately when the items were added to the backlog.


Both queue items trace to one surface — preflight writing the session document to disk behind the open editor — and the dominant bug is **fixed + shipped** (agent-doc `ff4d88ef`, 0.34.2 installed).

**JB `File Cache Conflict` (still occurring) — ROOT-CAUSED + FIXED (`#fccqueue`).** The 08b write-authority cutover routed the *finalize/response* writes through the editor IPC (no external-modification dialog), but `run_queue_maintenance` still persisted its own mutations (queue body sync, opening-tag activation-token strip, `queue:` frontmatter) with four **unconditional `std::fs::write(file, …)`** calls that bypassed that routing. So during a go-mode drain *every* preflight cycle touched the doc on disk while IntelliJ held it open → the conflict dialog. The four sites now route through one `persist_queue_maintenance_doc` gate: with a JB listener active it converges the queue shape through the editor (`converge_live_buffer_queue_shape` → plugin `setText` + `saveDocument`) and **skips the disk write**, logging `write_authority action=routed surface=queue_maintenance`; with no listener it writes to disk exactly as before. No plugin change needed — the convergence patch handler already persists conflict-free. This brings queue maintenance to the same `#fcc0` converge-or-disk discipline the pending/review sites already use.

**`agent:backlog queue` populating the queue — already implemented at cycle boundaries; this fix is what makes it visible.** The go-mode backlog→queue sync (`#backlog-queue-attr-populates-in-go-mode`) appends fresh `queue`-attr backlog ids to the live queue on every preflight, and same-cycle `--pending-add` captures sync at closeout (`#pendingaddqueuesync`) — both covered by tests. What was broken was *persistence*: that sync wrote to disk behind the editor (the same `std::fs::write` above), so its queue updates fought the IDE buffer. `#fccqueue` is exactly what lets a `queue`-attr backlog addition reach the live editor queue without a conflict. (Operator-verify items like `#tsiftmdcrash`/`#jbacceptwedge` are correctly *excluded* from the auto-drain queue — by design, not a missing sync.)

Verification: `make check` 4664 passed, clippy clean; binary built + installed 0.34.2 (`.bin` + `~/.cargo/bin`); pushed to origin/main. CI 27738315509 in progress at closeout — recorded, finishing from local evidence.

Follow-ups in backlog: `#fccaudit` (extend the gate to the remaining preflight/closeout disk-write sites so "no external disk writes while the editor runs" is fully closed) and `#editsync` [recommended] (sub-cycle keystroke-level sync if next-cycle latency proves insufficient).
<!-- agent:boundary:619f8d6e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~`agent:backlog` with `queue` attribute should have added the backlog items into the queue immediately when the items were added to the backlog.~~
- `agent:backlog` with `queue` attribute should add remaining backlog items into the queue an prepare and finalize. Note that if items are gated or have preconditions that are not met such as a date in the future, do not add the backlog item into the queue.
- :pushpin: do [#fccaudit]
<!-- /agent:queue -->
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
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
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#fccaudit] Extend the #fccqueue editor-IPC write gate to the remaining preflight/closeout session-doc disk-write sites (boundary cleanup, compact, git checkout/commit working-tree writes) so NO agent-doc disk write touches the session doc while a JB editor listener is active. Audit every std::fs::write/atomic_write(file) call site against is_listener_active and route each through converge/IPC or document why it is editor-safe. Completes item 2 fully; queue maintenance is done in #fccqueue.
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported 'tsift.md agent-doc turn crashed and killed the session process. The tmux pane was still active.' Supervisor pid 1742954 is alive now; pane %78 OpenCode TUI shows 'Thinking' normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from my cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe=(deleted)) as a crash. NEED operator to paste exact error text or the pane capture from the crash moment, or the specific cycle id, to root-cause. Not currently reproducible from logs alone.
- [ ] [#jbacceptwedge] [operator-verify] [operator-install] ROOT CAUSE FOUND + SHIPPED: FFI socket accept loop (ipc_socket.rs) blocked on apply handler before returning to accept(), piling connections in the backlog. Per-connection-thread fix landed (86c68db7, binary+cdylib installed 0.34.1 21:14). This cycle added ops.log proof: ipc_accept_thread_spawned inflight=N marker + InflightConnectionGuard + inflight_connection_handlers() accessor, plus strengthened slow_handler_does_not_block_concurrent_connections regression to assert peak inflight>=2. Plugin bumped 0.2.169 -> 0.2.170, built+signed: editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.170-signed.zip. Plan: tasks/agent-doc/plan-jb-accept-loop-wedge.md. Live gate: operator installs signed zip + restarts IntelliJ, drives a queue drain or any concurrent-IPC turn, then greps .agent-doc/logs/ops.log for 'ipc_accept_thread_spawned inflight=2' (or higher) — impossible under the old single-threaded loop, so it proves the fix is exercising concurrency. Make check: 4654 passed.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
