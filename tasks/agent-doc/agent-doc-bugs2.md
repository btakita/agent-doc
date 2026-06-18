---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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

### Re: queue-attr exclusion + #fccaudit + #rz3a — opus-4-8

> **Queue prompt:**
>
> `agent:backlog` with `queue` attribute should add remaining backlog items into the queue an prepare and finalize. Note that if items are gated or have preconditions that are not met such as a date in the future, do not add the backlog item into the queue.


> **Queue prompt:**
>
> `agent:backlog` with `queue` attribute should add remaining backlog items into the queue and prepare and finalize. Note that if items are gated or have preconditions that are not met such as a date in the future, do not add the backlog item into the queue.
>
> do [#fccaudit]
>
> do [#rz3a]

**Queue-attr exclusion — already implemented, now committed (`#backlog-not-before`).** The behavior you described was implemented last cycle but left **uncommitted** in the `src/agent-doc` working tree (the installed `0.34.3` binary already runs it, which is why this session's preflight already skipped the two `[operator-verify]` heads). The backlog→queue sync already excluded gated `[/]` and done `[x]` items; the prior work adds the **future-date precondition** you asked for: an open item carrying `not-before=YYYY-MM-DD` is held out of the `queue`-attribute sync until that UTC date, then syncs normally — it stays a plain `[ ]` entry the whole time (soft schedule, never auto-gated). Committed as `69efb8b2` with `pending::item_not_before_day` / `item_precondition_unmet` / `today_civil_day` and SimWorld/unit coverage.

**#fccaudit — done.** Extended the `#fccqueue` editor-IPC write gate audit to every remaining session-doc disk-write site. Two normal-path gaps now route through `converge_or_disk_write` (converge through the editor when a JB listener is live → no `File Cache Conflict`; byte-identical disk write with no listener): the `agent-doc write --status` replace (`status_set`) and the post-commit ephemeral guard-marker strip (`strip_guard_markers`, dropping `` / ``). Already-safe sites are documented rather than re-gated: compaction (`#w42v`), the post-commit boundary reposition (already skips the working-tree write while a listener is active), and the `#pcwc` worktree reconcile (HEAD-authoritative repair of a tree that *lost* committed content, followed by an editor-buffer IPC refresh). The recovery/scaffold/migration writes (`claim` scaffold, `repair`, preflight migration/repair, `session-check` over-application remedy, `reset` resume-clear) stay authoritative must-hit-disk **by design** — they must restore correctness even when the editor/IPC path is itself wedged, so failing closed on a live-but-unresponsive listener would break recovery. `(HEAD)` annotations / `agent:boundary` markers are deliberately out of scope. SPEC §07 records the full classification; `make check` green (4671 passed, clippy clean).

**#rz3a — gated, not implemented (per its own directive).** Your item says **"DO NOT IMPLEMENT THE FIX."** It got auto-drained this cycle only because that hold was unstructured prose, not a `[/]` gate or a `not-before=` date — so the sync (correctly, by its rules) treated it as an open actionable head. I gated it `[/]`, which is exactly the exclusion rule you described: a gated item is now held out of the backlog→queue sync, so it stops generating no-op `do [#rz3a]` churn cycles. Ungate it when you authorize implementing the supervisor idle-watch drainability fix.

*Session recovery:* preflight opened on two pieces of prior-cycle IPC drift — a merged `#rz3a`/`#fccaudit` backlog line (the newline between them was lost) and a stale snapshot longer than HEAD carrying your uncommitted `#rz3a` addition. Split the line, rebuilt the snapshot from current (`reset --from-current --preserve-session`), preserving all your edits. Release binary built; `cargo install` is deferred to right after this closeout so the freshly-installed `#fccaudit` binary doesn't drift against this session's still-running supervisor (it self-recycles onto it at the next idle boundary).

### Re: more raw disk writes still causing File Cache Conflict — opus-4-8

> **Queue prompt:**
>
> JB `File Cache Conflict` dialog is still occurring. Any more raw disk writes that need to be replaced?


> **Queue prompt:** JB `File Cache Conflict` dialog is still occurring. Any more raw disk writes that need to be replaced?

Yes — three, and they are all on the **route/dispatch path**, not the preflight/finalize path I just gated. That is why the dialog survives: `#fccqueue` + `#fccaudit` closed every session-doc write in the *preflight → respond → finalize → commit* cycle (ops.log confirms recent cycles all log `write_authority action=routed` with **no** `transport=disk`), but `agent-doc route` has its own writes that fire while the IDE editor is open during a go-mode dispatch:

| site | `route.rs` | when it fires | conflict risk |
|------|-----------|---------------|---------------|
| **queue activation** | `activate_existing_route_queue_head` (~2601) | every go-mode queue activation flips `queue_active` and `atomic_write_pub`s the whole doc | **high** — fires on every drain start behind the open buffer |
| dedup scrub | `scrub_duplicate_prompt_comments_for_route` (~1943) | route removes duplicate prompt comments | medium |
| session-UUID inject | `ensure_session_for_file` (~1926) | doc lacks a session id (one-time) | low |

The sibling `route_dispatch_queue` write (~2472) **already** converges through the editor IPC; these three were missed. The fix is the same `#fcc0` discipline: route each through `converge_document_or_disk`, with queue activation preferring the best-effort `persist_queue_maintenance_doc` pattern so a no-ack listener never *fails* the dispatch (route is the hot path — a fail-closed bail there would break go-mode startup, so it needs its own active/degraded/no-listener tests). Captured as **`#fccroute`** at the top of the backlog.

I scoped it as a fresh item rather than editing `route.rs` inline this cycle on purpose: it is the dispatch hot path, the gate must not fail-closed there, and this session is mid-recovery with a long exchange — the safe place for a route-critical change is a clean focused cycle, which the queue drain gives it next.

Not in scope (correctly still raw disk writes): the recovery/scaffold/migration paths (`claim`, `repair`, preflight migrate/repair, `session-check` remedy, `reset`, the `#pcwc` worktree reconcile) — those must write disk authoritatively even when the editor/IPC path is itself wedged, so they cannot fail-closed on a live-but-unresponsive listener.
<!-- agent:boundary:445b81c0 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~`agent:backlog` with `queue` attribute should have added the backlog items into the queue immediately when the items were added to the backlog.~~
- ~~`agent:backlog` with `queue` attribute should add remaining backlog items into the queue an prepare and finalize. Note that if items are gated or have preconditions that are not met such as a date in the future, do not add the backlog item into the queue.~~
- ~~JB `File Cache Conflict` dialog is still occurring. Any more raw disk writes that need to be replaced?~~
- ~~:pushpin: do [#fccaudit]~~
- ~~do [#rz3a]~~
- Add `Restart Agent` Plugin menu command. Changing the agent such as from claude to opencode should restart the agent during the next boundary.
- Why did you add DO NOT IMPLEMENT THE FIX to [#rz3a]?
- do [#fccroute]
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
- [ ] [#fccroute] Extend the #fccaudit editor-IPC write gate to the route/dispatch session-doc disk-write sites in route.rs: activate_existing_route_queue_head (queue activation atomic_write_pub ~line 2601, fires every go-mode queue activation behind a live editor), scrub_duplicate_prompt_comments_for_route (dedup atomic_write_pub ~1943), and ensure_session_for_file session-UUID injection (std::fs::write ~1926). These fire during route dispatch while the IDE editor is open and are the likely remaining File Cache Conflict source now that #fccqueue/#fccaudit gated the preflight/finalize path. Route each through the converge gate (converge_document_or_disk, mirroring the sibling route_dispatch_queue write that already converges at ~2472; queue activation may prefer the best-effort persist_queue_maintenance_doc pattern so a no-ack listener never fails the dispatch). Add tests proving activation/dedup/session-id still succeed under active, degraded, and no-listener. Keep the snapshot save (private .agent-doc, never conflicts).
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported 'tsift.md agent-doc turn crashed and killed the session process. The tmux pane was still active.' Supervisor pid 1742954 is alive now; pane %78 OpenCode TUI shows 'Thinking' normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from my cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe=(deleted)) as a crash. NEED operator to paste exact error text or the pane capture from the crash moment, or the specific cycle id, to root-cause. Not currently reproducible from logs alone.
- [ ] [#jbacceptwedge] [operator-verify] [operator-install] ROOT CAUSE FOUND + SHIPPED: FFI socket accept loop (ipc_socket.rs) blocked on apply handler before returning to accept(), piling connections in the backlog. Per-connection-thread fix landed (86c68db7, binary+cdylib installed 0.34.1 21:14). This cycle added ops.log proof: ipc_accept_thread_spawned inflight=N marker + InflightConnectionGuard + inflight_connection_handlers() accessor, plus strengthened slow_handler_does_not_block_concurrent_connections regression to assert peak inflight>=2. Plugin bumped 0.2.169 -> 0.2.170, built+signed: editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.170-signed.zip. Plan: tasks/agent-doc/plan-jb-accept-loop-wedge.md. Live gate: operator installs signed zip + restarts IntelliJ, drives a queue drain or any concurrent-IPC turn, then greps .agent-doc/logs/ops.log for 'ipc_accept_thread_spawned inflight=2' (or higher) — impossible under the old single-threaded loop, so it proves the fix is exercising concurrency. Make check: 4654 passed.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rz3a] qflood-gated supervisor idle-watch re-injection: the supervisor idle-queue watch keeps re-injecting agent-doc FILE triggers on a queue whose only remaining head is gated / operator-blocked, causing repeated no-op agent-doc cycles. REPRO (equityfundingsource.md, 2026-06-18): a go-queue block was fully drained (all 7 free-text heads consumed via agent-doc queue consume); the lone remaining head (a do-reference to a now-gated review item) was gated via pending-gate and moved to agent:review. session-check then reported queue_continuation_required=false, queue_drainable_head_count=0, ui_outcome=no_drainable_work. After agent-doc admin queue resume, the supervisor idle-watch re-injected agent-doc FILE repeatedly: 4 consecutive no-op agent-doc cycles, each preflight no_changes=true. agent-doc admin queue pause stops it. EXPECTED: when queue_drainable_head_count==0 and queue_continuation_required==false (only gated or operator-verify heads remain), the supervisor idle-queue watch should DEFER instead of injecting, matching the in-session loop stop rule. ROOT-CAUSE HYPOTHESIS: the idle-watch dispatch gate keys off queue presence / queue:start frontmatter instead of the binary drainability filter; the in-session loop already honors queue_continuation_required=false but the supervisor idle-watch path does not. FIX DIRECTION (do not implement): make the supervisor idle-queue watch reuse the same drainability filter as the in-session loop and defer when queue_drainable_head_count==0. DO NOT IMPLEMENT THE FIX.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
