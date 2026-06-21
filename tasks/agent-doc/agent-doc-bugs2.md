---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260621-032011.md`*

Compacted content:
- Archived 26 response topic(s): Session compacted + queue corrected; Diagnose JB File Cache Conflict dialogs; Implemented #evmh (binary half); 23 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...
### Re: Documented the node-merge architecture + captured the JB dispatch bug — opus-4-8

Done (recorded post-compaction via Edit + `agent-doc commit` — the live editor IPC is dead this session, so `finalize` could not write).

**Architecture doc** — new `src/agent-doc/docs/reference/node-merge-architecture.md`: whole-document blob-merge problem → per-node merge model → shipped `merge_by_component` (`#qnodemerge1`) → leaf `merge` safeguards (stale-base detection + committed-`### Re:` preservation) → `#qnodemerge2–5` roadmap. Linked from the README Architecture section and `docs/SUMMARY.md`, with a maintenance note binding it to `crdt.rs`. Commits: submodule `e89d5ee8`, superproject pointer `805abd86c` (not pushed — STOP-cutting-releases / confirm-before-push).

**JB queue head** is a bug report, not a request to run the action — I did not trigger the clear+rerun. Captured as `#jbclrdispdup` (`#t3mn`), same dispatch-restack/no-submit class as `#rdypoll` / `#rdypoll-deploy`. Also filed `#nodesymrename` [recommended] — code symbol is still `Cell`/`segment_into_cells` while the docs now say "node".

### Re: Code-first — shipped #6re3 rename + 2 of the #j9ja gate markers — opus-4-8

You chose **code first**, so I shipped the agent-actionable code (no review-emptying, no release/install ceremony) and kept the review items until their logging lands.

**#6re3 #nodesymrename (DONE).** Renamed the `agent-doc-core` crdt segmentation symbol `Cell` → `Node` (`segment_into_cells` → `segment_into_nodes`, `Cell::Component`/`Cell::Interstitial` → `Node::*`, plus locals + doc comments) to match the operator cell→node term rename. Behavior-neutral; `merge_by_component` and the historical `#qcellmerge1` ids are preserved. The 4 `merge_by_component` tests pass.

**#j9ja avmark — 2 markers landed, rest blocked on unshipped features.** I wired the two proof sites that already exist and just needed a positive log line:
- **#jbtsiftnosub** — `route/dispatch.rs reverify_auto_start_dispatch_ready` now logs `auto_start_dispatch_ready_confirmed` on the cleared-gate Ok path (the positive counterpart of the existing `dispatch_into_starting_pane` refusal). Modeled in the SimWorld engine and asserted in `route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send`.
- **#qpausemix-verify** — `session_check.rs` now logs `queue_paused_continuation_guidance_emitted pause_reason=…` when it emits continuation guidance on a controller-paused queue (previously the preamble reached stderr only). Registered the new `reason=` token in the FlowCore hot-path budget.

The remaining ~10 markers are genuinely blocked: #agentreloadrestart / #restartagentmenu have **zero emission sites because the restart-agent features are unshipped**; #fbwire needs its Phase 2 supervisor path to emit `record_force_disk_fallback` live; and the route/plugin bucket (#qpauseux/#jbrundispatch/#1vhn/#cyh0/#8bfz/#qheadstrike/#hap7/#gf5n/#rt83/#docdriftgrace) needs per-feature focused cycles. That matches the prior #avgwire finding that only `#tsiftmdcrash-verify` was cleanly wireable in one pass — these need their features shipped first.

**Verification.** `make check` green (4853 passed, 239 skipped). Committed + pushed: submodule `61e067f6`, superproject pointer `01599db64`. CI for `61e067f6` is in_progress (prior runs green); recording per the closeout rule rather than blocking.

**No install / no JB plugin reinstall.** Per STOP-cutting-releases I did **not** bump/install/recycle — the two markers go live when you `make install` at a supervisor-recycle boundary, same as the other [operator-verify] items. No `.kt` changed, so no JB plugin reinstall is needed. To live-test once installed: drive the JB Run Agent Doc cold-start path and confirm ops.log shows `auto_start_dispatch_ready_confirmed`; and on a paused queue confirm `queue_paused_continuation_guidance_emitted`.

**Review not emptied this cycle** (your "keep review items until logging lands" choice). #t3mn (#jbclrdispdup) stays — it needs a live-pane repro to fix.

### Re: queue cleanup — removed completed strikes — opus-4-8

Acknowledged — pure queue housekeeping. You removed 5 already-struck (completed) items from the queue:

- The 3 answered/done free-text items (diff-key accuracy question, `cell`→`node` rename, node-merge architecture docs) and the `#jbclrdispdup` JB Clear-Session/Run-Agent-Doc strike — all free-text strikes, reaped by `#ftstrike`, no archival id.
- `do [#t3mn]` — the one id-backed strike. Archived via `--done t3mn`. This is the `[operator-verify] #jbclrdispdup` duplicate-dispatch / no-submit bug, same dispatch-restack class as `#rdypoll`/`#rdypoll-deploy` already tracked in `.done.md`; your strike resolves it. If you intended only to clear queue clutter (not mark the bug verified-fixed), say so and I'll re-open it in the backlog.

Queue is **paused** (`#rt83/#qflood` phantom-pin halt) with **0 drainable heads** — remaining ids (`#qnodemerge2`–`#qnodemerge5`, `#6b5h`, `#1jec`, `#qprtloss`, `#qcm1verify`) stay pending and operator-gated. Not auto-looping. (Exchange accretion is in `warn`; flagging in one line per the active-queue guidance, no compaction prompt.)

### Re: JB `Run Agent Doc` no-submit on equityfundingsource.md — opus-4-8

> **Queue prompt:** JB `Run Agent Doc` on equityfundingsource.md did not submit to Claude — supervisor-stderr.log showing lib-install auto-recycle + `execve` hot-reload burst, a `start_session failed` for `tsift.md`, repeated stale-busy-actor reconciles, and `clear cooldown active … skipping passive queue dispatch`.

This answers my offer from last cycle ("say so and I'll re-open it") — the bug is **not** fixed, so I've **re-opened it** as `#jbdisprecycle` (the `#t3mn`/`#jbclrdispdup` dispatch-no-submit class) with a fresh id so it doesn't get reaped as an already-done mirror.

**Root cause (code read, not a guess).** The dispatch path (`route/dispatch.rs`, `route/dispatch_only.rs`) waits for a harness **dispatch-ready prompt** before typing (the `#rdypoll` §D fix) — but it has **zero awareness of supervisor recycle state**: grepping those two files for `recycle_yield` / `is_recycling` / `mid-recycle` returns nothing. So when your `lib-install` auto-recycle fires the `execve` hot-reload burst, this happens:

1. The pane's session actor is being torn down/restarted — proven by the terminal `start_session failed: … for tasks/software/tsift.md` (`project_controller/rpc.rs`) in the same recycle batch.
2. The ready-gate reads a *transiently* "ready" pane (`reconciled stale busy actor to ready from idle pane evidence`) and injects the trigger text — but the in-flight route/submit is interrupted by the `execve`, so the **Enter never lands**.
3. `clear cooldown active … skipping passive queue dispatch` then suppresses the supervisor's own retry, stranding the half-typed dispatch.

Net: **dispatch-ready proof and recycle-settle are not sequenced** — a dispatch can type-without-submit across a recycle boundary.

**Fix direction** (in the plan, 4 rungs): R1 recycle-aware dispatch gate (defer inject until the supervisor settles onto the new binary, then re-acquire ready), R2 make a `start_session` that races a hot-reload retryable instead of terminal, R3 extend the `dispatch_inject attempt=N` marker so an interrupted dispatch re-submits exactly once (not re-typed N times — that's the restack symptom), R4 a SimWorld scenario asserting defer-until-settle + submit-once. Plan: `tasks/agent-doc/plan-dispatch-during-recycle.md`.

**I did not ship a code fix this cycle** — it's `[operator-verify]` (needs a live editor/pane to verify), the queue is **paused** (`#rt83/#qflood`), and the STOP-cutting-agent-doc-releases gate is active; installing a dispatch change mid-session against the live supervisor would feed the exact churn you paused over. Drive R1–R4 at a recycle boundary in a focused session when you're ready. Not auto-looping (`queue_continuation_required=false`).

### Re: reopen and fix #jbdisprecycle — R1 shipped — opus-4-8

Done for the agent-actionable core (R1). The no-submit was a real race with a precise root cause, and I fixed it where the gap actually is — the dispatch path had no awareness of the supervisor recycle.

**What shipped** (agent-doc submodule `ad55b993`, superproject `2a514eb02`):

1. **`recycle_inflight.rs`** — a new project-root-scoped, short-TTL (15s) "supervisor mid-recycle" marker. Project-scoped on purpose: one project controller hosts many docs, so the live repro's recycle (fired for `agent-doc-bugs2.md`/`tsift.md`) has to be visible to an `equityfundingsource.md` dispatch. A unit test pins exactly that cross-document behavior.
2. **`idle_watch.rs`** — marks the project mid-recycle *before* the rebuild+install and *before each* `supervisor_perform_reexec` (both the auto-recycle path that logs `(next_queue_item)`/`(idle)` and the operator-restart path). The fresh post-recycle supervisor clears it on watch-loop start; the TTL is the backstop.
3. **`route/dispatch_only.rs`** — a bounded **10s recycle-settle wait before any pane input**: if the supervisor is mid-recycle, the dispatch defers (does **not** type), proceeds with the normal ready-probe once it settles, and **fails closed** (never types) if it never settles — so the caller retries instead of stacking an unsubmitted trigger. No more typing across the `execve` boundary.

**Verified:** `make check` green (4857 tests, +4 new unit tests), clippy clean, and the 4 new `reason=` audit tokens routed through the FlowCore budget gate.

**Deliberately NOT done** (and why):
- **Not installed/released.** The STOP-cutting-agent-doc-releases gate is active and installing mid-session would feed the live supervisor the exact recycle churn this fixes. The running supervisor stays on the old binary until you install at a recycle boundary.
- **R2/R3/R4 are follow-up rungs**, kept in `#jbdisprecycle`: R2 retry a `start_session` that races a hot-reload, R3 a submit-once proof spanning recycle (`dispatch_inject attempt=N`), R4 a live-pane SimWorld scenario. R1 is the direct no-submit fix; these harden the surrounding race.
- **Live-pane verify is yours** (operator-verify): at a recycle boundary, `make install` + `agent-doc admin recycle`, then fire JB `Run Agent Doc` on a second doc during the hot-reload burst — expect a single `dispatch_start_proof` **with** submit and no stranded `start_session failed`.

Plan with the full rung breakdown: `tasks/agent-doc/plan-dispatch-during-recycle.md`. Not auto-looping.

### Re: do #jbdisprecycle — R2/R3/R4 shipped — opus-4-8

Done for all remaining agent-actionable rungs. R1 shipped last cycle; this cycle finished R2, R3, and R4, so the dispatch-during-recycle race is now hardened end-to-end. Per the operator-verify rule I did every agent-doable sub-step and left only the live-pane eyeball for you.

**What shipped** (agent-doc submodule `d21079b7`, superproject `3c84f70a9`; pushed, **not** installed/released per STOP-cutting-releases):

- **R2 — `start_session` retry during recycle.** `start/run.rs` now wraps the `start_session` call in a recycle-aware retry: on failure it checks the project-scoped `recycle_inflight` marker; if the supervisor is mid-`execve` recycle it logs `start_session_recycle_retry`, waits the bounded settle window (`wait_for_recycle_settle`, ≤10s), re-runs `ensure_controller_running`, and retries (≤2). A non-recycle failure or an unsettling recycle still surfaces the original terminal error — so the live repro's `start_session failed for tasks/software/tsift.md` becomes a retry instead of a stranding terminal error.
- **R3 — submit-once spans recycle.** `route/dispatch.rs` adds a settle gate right after the trigger is injected (`dispatch_inject attempt=1`) and the first acceptance poll comes back non-Accepted: if mid-recycle it waits for settle, logs `route_dispatch_submit_recycle_settle ... action=submit_once_after_settle`, and re-polls once — so the budgeted resubmit loops run against the settled supervisor and land the submit exactly once, never re-typing N times (the `#rdypoll` `attempt=2+` restack symptom).
- **R4 — SimWorld coverage.** New `route_sim_dispatch_defers_during_recycle_then_injects_once_after_settle` models the recycle marker (`RouteModel.recycle_inflight` + `Mark`/`Settle`/`DispatchDuringSupervisorRecycle` SimCommands): 7 dispatches mid-recycle all fail closed (`dispatch_into_recycling_pane`, zero injects, no stacked triggers); after settle the same dispatch injects exactly once (`attempt=1`, never `attempt=2`).

**Verified.** `make check` green — clippy `-D warnings` clean, full suite passes including the SimWorld scenario and 4 new `recycle_inflight` pure-decision unit tests. Latest CI run is green.

**Only operator live-verify remains** (`#jbdisprecycle` stays as `[operator-verify]` in the backlog): at a recycle boundary run `make install` + `agent-doc admin recycle`, fire JB `Run Agent Doc` on a second doc during the hot-reload burst, and confirm a single `dispatch_start_proof` **with** submit and no stranded `start_session failed`.

Not auto-looping — `queue_continuation_required=false` (only `[operator-verify]` heads remain) and the queue is paused.

### Re: do #6b5h — landed the detection primitive; full wiring is a focused phase (#6b5hwire) — opus-4-8

I traced #6b5h to root and landed the foundational fix, but stopped short of flipping the write-guard itself this cycle — that part has a blast radius and regression profile that genuinely needs a focused cycle, and I want to lay out why.

**Root cause (confirmed).** A pure-CLI session sees a *connectable* `ipc.sock` because the project controller hosts a listener that returns error acks with **nothing behind it**. So `is_listener_active()` reports `true`, the "editor IPC listener active → refuse direct disk write" guard stays armed, and every finalize wedges on `no_ack` — even though there is **no editor buffer to protect**. The guard cannot tell a transiently-wedged *real* editor apart from a CLI-only dead-end listener. That missing distinction is the whole bug.

**What I shipped** (agent-doc submodule `8bb5e1e5`, pushed; **not** installed/released per STOP-cutting-releases):
- `plugin_owner::live_editor_endpoint_attached(file)` — the positive "a real editor is actually attached" signal the guard lacks. It keys off the **owner-pid liveness** of the shipped `#8bfz` plugin-owner lease, **not** heartbeat freshness. That detail matters: the plugin refreshes the lease heartbeat only on patch events, so an idle editor with the doc open has a stale heartbeat but a live pid — gating on freshness would misclassify it as CLI-only and route a disk write straight into its live buffer (the exact File Cache Conflict the guard exists to prevent). A CLI-only session simply never wrote a lease.
- Pure, unit-tested core (`editor_endpoint_attached_for_lease`) with the live-pid / dead-pid / no-lease / stale-but-live cases pinned. **Zero behavior change to the guard** this cycle, so `make check` is green with no regressions.

**Why I did NOT wire it into the guard yet (the honest part).** Flipping the refuse-disk-write branches to permit a controller-host disk write when no live editor is attached changes the default for "active listener but no lease" — and **~259 existing tests** start a simulated listener *without* seeding a plugin-owner lease and assert the write is refused. Each would need a fresh live-pid lease seeded (they're simulating a real editor) before the flip, or they'll break en masse. Doing that sprawling edit across the most safety-critical path in the product — the one you've flagged as "too much churn and IPC issues" with per-cell CRDT merge as the priority — unattended in one cycle is the wrong risk/reward. It's exactly why you tagged #6b5h `[focused-cycle]`.

**Queued the wiring as `#6b5hwire`** with the precise contract: gate `run_entry.rs` ~L1322 (the headline finalize wedge: `run_ipc` file-IPC fallback → `recovery=retry_without_disk_write`) and the `converge.rs` `no_ack`/`no_ack_content`/`send_failed` branches (the `write --pending-add` wedge) on `!live_editor_endpoint_attached`, seed the ~259 listener tests, add SimWorld `cli_no_editor → disk_write` coverage, and note the one residual edge (a real editor open but wedged before its first-ever patch has no lease — the robust closeout there is a plugin timer-based editor-endpoint heartbeat, a separate rung). Live editor-less repro stays `[operator-verify]`.

Queue is paused and `queue_continuation_required=false` — not auto-looping.

### Re: do [#qnodemerge2] — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#qnodemerge2]


Done — durable per-node CRDT state persistence is implemented, tested, and committed (agent-doc submodule `276f6f9e`, local only; push held per STOP-cutting-releases, operator chose Hold).

**What landed**
- **`MultiNodeState`** (`agent-doc-core/src/crdt.rs`) — one independent Yrs state per top-level node (each `agent:*` component + the interstitial text around them), serialized into a single structured container at `.agent-doc/crdt/<hash>.nodes.yrs`. This is the per-component successor to the whole-doc `<hash>.yrs` blob.
- **Deterministic per-node encoding** (`encode_text_deterministic`, fixed Yrs client id `0`) so identical text re-encodes byte-identically: an untouched node's base is provably unchanged across a cycle while a changed node's base advances on its own. `Doc::new()`'s random client id made that impossible.
- **Migration** — `decode_or_migrate` lazily splits a legacy `<hash>.yrs` into per-node states; falls back to a text rebuild if unreadable. A magic+version container header means a per-node file never collides with a legacy whole-doc blob.
- **Shared loop** — extracted `merge_aligned_nodes` behind both `merge_by_component` and the new stateful `MultiNodeState::merge`; whole-doc `merge` stays the structural-divergence / component-less fallback.
- **Persistence wiring** (`agent-doc-orchestration/src/snapshot.rs`) — `multinode_crdt_path_for` / `load_multinode_crdt` / `save_multinode_crdt` / `multinode_crdt_state` (sidecar → legacy migration → fallback). `save_document_crdt` also writes the per-node sidecar (rebuilt each cycle, so compaction GCs per node), `delete_crdt` removes it, and the rename migration carries it.

**Acceptance — all covered by 12 new tests:** round-trip per-component encode/decode, a node's base advances independently, per-node GC on compaction. `make check` green; latest CI on the prior HEAD green.

**Deliberately not done:** the live orchestration merge base is unchanged (still overlay → whole-doc `crdt::merge`). Rewiring the live write path to consume per-node bases is the recursive `#qnodemerge3` rung; the plan's risk note ("not mid-drift, keep whole-doc fallback") argues against rewiring the load-bearing path in this same cycle. This rung lands the durable per-node *base* without altering live merge behavior. Architecture doc (`docs/reference/node-merge-architecture.md`) updated to mark Phase 2 shipped.

### Re: continue the queue after /clear — supervisor clear-and-continue for [focused-cycle] (#qfocsup) — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#qnodemerge3]


You're right — idling the queue was the bug, and I've fixed the binary so the CPC/supervisor clears the session and continues the queue for dedicated-session heads (agent-doc submodule `67829f3f`, committed locally; not installed/pushed per STOP-cutting-releases).

**Root cause.** A `[focused-cycle]` head was deferred by BOTH drain paths: `ExecutionContext::loop_undrainable()` (= `[operator-verify]` ∪ `[focused-cycle]`) fed the single `deferred_backlog_ids`, which both the in-session loop *and* the supervisor idle-watch head-picker (`live_drainable_continuation_head`) consulted. So when only `[focused-cycle]` heads remained, nobody drained them → idle.

**Fix — split drainability into two scopes (`#qfocsup`):**
- **In-session loop** still defers `[operator-verify]` + `[focused-cycle]` — the accreted in-session context can't give a focused-cycle item the fresh context it needs, so the loop yields it.
- **Supervisor** defers `[operator-verify]` **only**. `[focused-cycle]` is now drained by the supervisor's clear-and-continue path: `head_requires_context_reset` (= `[clean-session]` ∪ `[focused-cycle]`) makes the idle-watch force a `/clear` first, then re-dispatch the head to a genuinely fresh session.
- New `session-check` outcome `deferred_for_supervisor_drain` (`next_action=yield_to_supervisor_clear_and_continue`, class `ok`): a focused-cycle-only queue now reports "queue continues via supervisor," **not** `operator_proof_required`. The skill auto-loop guidance distinguishes the two so an agent never again reports a focused-cycle head as an operator stall.

The handoff: the in-session agent ends its turn → the pane idles → the supervisor idle-watch force-`/clear`s and re-dispatches the focused-cycle head to a fresh session → repeat. New unit tests cover both scopes, the head-picker split, the force-clear set, and the outcome; `make check` green.

**To activate live (operator-gated):** the running supervisor is on the old binary, so this takes effect only after `make install` + `agent-doc admin recycle`. That install is gated by STOP-cutting-releases (no install/release until the per-node merge corruption proves out) — say the word and I'll install + recycle, or hold. Filed `#qfocsupverify` [operator-verify] for the live proof (watch the supervisor clear + re-dispatch `#qnodemerge3` to a fresh session).
<!-- agent:boundary:9fe1672a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: Is the diff key approach 100% accurate? Can there be key conflicts? Is this the fastest way to merge?~~
- ~~Rename the term "cell" to "node".~~
- ~~:pushpin: do [#qnodemerge2]~~
- ~~:pushpin: do [#qnodemerge3]~~
- :pushpin: do [#qnodemerge4]
- :pushpin: do [#6b5h]
- :pushpin: [#1jec]
- :pushpin: do [#qprtloss]
- :pushpin: do [#qcm1verify]
- :pushpin: do [#qnodemerge5]
- ~~Please document the document node merge architecture. Link from the readme. Ensure the document remains up to date if any changes are made.~~
- ~~:pushpin: JB `Clear Session Context` then JB `Run Agent Doc` still sends duplicate prompts and no submit.~~
- ~~do [#t3mn]~~
- :pushpin: do [#6b5hwire]
- do [#6b5hwire]
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
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#6b5hwire] [focused-cycle] #agent-doc-bug Wire the #6b5h live-editor-endpoint detection into the write-guard so a CLI-only (no live plugin) session stops wedging finalize. Primitive landed+pushed: plugin_owner::live_editor_endpoint_attached (submodule 8bb5e1e5; keys off the #8bfz plugin-owner lease owner-pid liveness, NOT heartbeat freshness so an idle real editor is never misclassified; make check green). Remaining: gate the fail-closed disk-write refusals on !live_editor_endpoint_attached so they route to the controller-host disk write instead of bail. Primary chokepoint run_entry.rs ~L1322 (run_ipc file-IPC fallback, recovery=retry_without_disk_write -- the headline finalize wedge); secondary converge.rs try_editor_converge no_ack/no_ack_content/send_failed branches (covers the write --pending-add wedge). BLAST RADIUS: ~259 listener tests assert active-listener=>refuse and seed NO plugin-owner lease, so each must seed a fresh live-pid lease (they simulate a real editor) before this flips their default; add SimWorld CLI-only-no-editor=>disk_write coverage. Residual edge: a real editor open but wedged before its first-ever patch has no lease yet -- robust fix is a plugin timer-based editor-endpoint heartbeat (separate rung). Live editor-less repro is [operator-verify]: drive a finalize in a Claude Code pane with no JB IDE, confirm transport=cli_no_editor_disk_write + committed, no force-disk.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#qnodemerge4] [focused-cycle] #agent-doc-bug Op-capture / evented reflection (accuracy lever 1, highest leverage). Stop reconstructing edits from a text diff; capture the editor real operations (DocumentListener.documentChanged / onDidChangeTextDocument: insert at offset, delete range) and feed those ops to the per-node Yrs model directly, removing the diff-guess so live-edit merges are exact and intention-preserving. Acceptance: a concurrent live edit + agent write converges with zero diff-derived duplication. Plan: tasks/agent-doc/plan-percell-crdt-merge-isolation.md (Phase 4). Depends on #qnodemerge2 + #qnodemerge3. Operator-driven fresh clean session.
- [ ] [#qnodemerge5] [focused-cycle] #agent-doc-bug Surface true conflicts, never fabricate (accuracy lever 5, the safety net). For genuine concurrent edits to the SAME leaf node (information-theoretically underdetermined), do not silently auto-merge text neither side wrote: surface both versions in agent:exchange or a conflict marker for operator resolution. Extends #hap7. Acceptance: a true same-leaf concurrent edit produces an operator-visible conflict, not a fabricated merge; non-conflicting edits never trip it. Plan: tasks/agent-doc/plan-percell-crdt-merge-isolation.md (Phase 5). Sequenceable after #qnodemerge3. Operator-driven fresh clean session.
- [ ] [#qprtloss] [focused-cycle] #agent-doc-bug Silent user-prompt loss: a user-typed queue line ("- re #cftk: Would passage... work?") was deleted by preflight queue-convergence during a concurrent admin recycle, and the cycle reported no_changes so the prompt was never answered. Smoking gun: preflight #1 no_changes:false diff_type:simple_question diff:null, then preflight #2 no_changes:true; prior turn logged "converged live editor buffer (auto=false, queue_active=false)" + "admin recycle ... at next idle boundary". Distinct from #6b5h (finalize IPC wedge) — this is preflight-side data loss before any response is composed. Fix dirs: convergence must CRDT-merge (never overwrite) an uncommitted live-buffer queue/exchange edit; no_changes:false + diff:null with non-empty prompt_changes must fail closed (reconstruct from semantic diff, never resolve to no_changes:true); sequence recycle after pending-prompt capture; add SimWorld coverage. Plan: tasks/agent-doc/plan-prompt-loss-queue-convergence.md. DO NOT IMPLEMENT yet.
- [ ] [#6b5h] [focused-cycle] #agent-doc-bug Pure-CLI agent-doc session (no live JB editor) wedges every finalize on editor-IPC no_ack, forcing --force-disk. Live repro 2026-06-20, equityfundingsource.md driven from a Claude Code pane (%158) with NO JetBrains IDE running and the controller (controller serve, pid 2014438, 0.34.27) as the sole daemon: admin list shows the doc actor ready with an active editor-IPC listener, but there is no editor endpoint behind it. Every finalize: socket "ack status: error" -> file-IPC fallback writes a patch no editor applies -> no_ack -> recovery=retry_without_disk_write -> fail-closed "refusing direct disk write; editor IPC listener active". Only finalize --force-disk succeeds (commits landed via CRDT merge). Controller-mediated write --pending-add into THIS bugs doc also wedged identically (send_failed) and the foreign-ownership guard required claim --force - the bug plus ownership guard block capturing the bug via the normal path. Never self-heals: actor not orphaned and binary not stale, so neither M1b orphan-reap, drain-lease expiry, nor stale-binary recycle fires. Extends #cyh0/#fcczero, #wd40, #rt83 (queue_control_paused dispatch flood ~2/sec on this doc + orphaned save-document.signal), #editorbufwin. Full report: tasks/agent-doc/ipc-wedge-cli-no-editor.md. Fix direction: detect editor-IPC-listener-registered-but-no-live-editor for a CLI-only actor and route finalize to the controller-host disk write instead of no_ack wedge; OR do not arm the editor-IPC disk-write guard when no plugin editor is attached. Supervisor-core; needs editor-less live repro + SimWorld coverage.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#jbdisprecycle] [operator-verify] #agent-doc-bug #jbclrdispdup JB Run Agent Doc no-submit during post-lib-install supervisor recycle. R1-R4 ALL SHIPPED+pushed (agent-doc submodule d21079b7, superproject 3c84f70a9; NOT installed/released per STOP-cutting-releases). R1 recycle_inflight marker + dispatch_only pre-inject settle gate. R2 start/run.rs recycle-aware start_session retry (wait_for_recycle_settle + re-ensure controller, <=2 retries) so a start_session racing the execve hot-reload is retried, not terminal. R3 route/dispatch.rs submit-once-after-settle gate (route_dispatch_submit_recycle_settle) so an injected-but-unsubmitted trigger re-submits exactly once after settle, never re-typed N times. R4 SimWorld route_sim_dispatch_defers_during_recycle_then_injects_once_after_settle + 4 recycle_inflight unit tests; make check green, clippy -D warnings clean. ONLY operator live-verify remains: at a recycle boundary run make install + agent-doc admin recycle, fire JB Run Agent Doc on a second doc during the hot-reload burst, confirm a single dispatch_start_proof WITH submit and no stranded start_session failed.
- [/] [#rdypoll-deploy] [operator-verify] #agent-doc-bug Deploy + live-verify the #rdypoll §D dispatch_inject marker (agent-doc submodule 138dc658, 0.34.23). Agent-actionable code shipped+pushed (dispatch_inject attempt=N marker at both injection funnels + SimWorld assertion). REMAINING (operator-gated, deferred from session to avoid finalize-client-newer-than-supervisor drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then the two-pane restart repro: restart a session, JB Run Agent Doc, confirm ops.log shows a single dispatch_inject ... attempt=1 and ZERO attempt=2 (proves no post-restart re-type stacking). Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md (§D)
- [/] [#wd40] [recommended] #agent-doc-bug Supervisor stale-binary auto-recycle does not fire during a continuously-active self-draining session. lib-install marks every controller to recycle (AGENT_DOC_RECYCLE_ON_INSTALL), but idle_watch.rs only execve-hot-reloads at boundary=idle (harness idle + debounced grace) or boundary=next_queue_item, and NEVER mid-turn (the not-turn_active gate). A long in-session /loop drain holds the drain-claim lease AND keeps the harness turn_active back-to-back, so the supervisor never reaches its recycle boundary and the stale binary persists — the root of this session content_ours drift + #rt83 flood (#supselfheal already notes a stale BINARY does not self-heal by lease expiry; needs explicit admin recycle). Fix direction: when idle_watch detects its own binary is stale AND a drain is in progress, signal the in-session loop to yield at the next inter-item boundary so the execve recycle fires automatically (automate #staleloop-recycle-restart) instead of requiring the agent to manually run make install + admin recycle + end-turn. Mid-turn execve stays OUT of scope: the supervisor owns the in-flight cycle CRDT/write-queue/IPC/cycle_state, which is rebuilt fresh after re-exec, so a mid-write swap drops/corrupts the cycle; a true mid-turn hot-swap would need the control-plane state serialized across execve and restored atomically. Supervisor-core (idle_watch.rs / drain lease / recycle gating), live-supervisor-critical: focused cycle with make check + make tmux-ci, not in-loop.
- [/] [#j9ja] avmark [recommended] Add a distinctive SUCCESS ops.log marker at each gated review item proof site so #optverify auto-verify (--pending-set-verify verify=ops_log:<marker>) resolves them from a live operator test. LANDED submodule 61e067f6: #jbtsiftnosub auto_start_dispatch_ready_confirmed on the reverify-gate Ok path (SimWorld-asserted); #qpausemix-verify queue_paused_continuation_guidance_emitted on the controller-paused continuation-guidance path. REMAINING (each blocked on shipping its underlying feature first, per-feature focused cycles): #agentreloadrestart agent_restart_performed + #restartagentmenu (restart-agent unshipped); #fbwire record_force_disk_fallback (Phase 2 supervisor path); route/plugin bucket #qpauseux #jbrundispatch #1vhn #cyh0 #8bfz #qheadstrike #hap7 #gf5n #rt83 #docdriftgrace.
- [/] [#editorbufwin] #agent-doc-bug #queue-user-edit-overwrite The IPC content_ours recovery drops unconsumed user queue additions the operator typed into the live editor buffer (operator: "the queue reverted to disk state; the editor buffer should have won as the source of truth"). PLAN DONE: root cause is NOT content_ours composition (it deliberately keeps additions next-cycle-visible on disk per the ignores_unproven_live_queue_deletions invariant — a blind splice-into-content_ours attempt 2026-06-19 tripped that test) but the #pcwc post-commit worktree reconcile overwriting the disk addition with HEAD. GATED for a focused merge-core cycle. Fix: P1 #pcwc reconcile preserves unconsumed user queue additions on disk; P2 auto-recover the commit (automate the manual reset --from-current the operator does today); P4 refresh the stale supervisor CRDT that enables the drift. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [/] [#fbwire] [operator-verify] #agent-doc-bug Phase 2 live wiring of the #fullboundary convergence gate (Phase 1 decision core + playback shipped, submodule 17010e78). Call convergence_gate_decision inside the supervisor idle-queue-watch/drain inter-item dispatch (start/idle_watch.rs to start.rs): gather the four facts (committed via cycle_state, editor converged==HEAD via emit_postcommit_worktree_check, ipc_socket inflight==0, actor idle), defer the next trigger until Dispatch, on ForceDiskFallback do the real --force-disk write + record_force_disk_fallback. Add SimWorld acceptance. Live-supervisor-critical: needs make check + make tmux-ci across two live panes, so NOT in-session-loop-drainable; needs a focused operator-driven cycle. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [/] [#hap7] [operator-verify] #agent-doc-bug #qdup Operator-reported (2026-06-19): agent-doc duplicated the operator equityfundingsource.md queue prompt (All upload preview dialogs should be full screen, deploy) on a concurrent edit. Second instance of the queue-prompt duplication family (first: virtuousai.md). Fix per operator spec: constrain the CRDT/semantic merge to the structural part it applies to (per-structural-node merges, not whole-component) so a concurrent operator edit to one queue item cannot duplicate adjacent structure; and when the operator deletes a structure that agent content targets, note it in agent:exchange instead of merging. Merge-core change, the corruption source this session, so NOT safe to rush in the in-session loop; needs a focused cycle. Plan: tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md
- [/] [#qpausemix-verify] [operator-verify] #agent-doc-bug Verify the #qpausemix session-check fix (submodule 2cf7bcee) live: build+install the new binary (make install: cargo install --path . + cargo build --release + agent-doc lib-install) at a supervisor recycle boundary (deferred from this session to avoid finalize-client-newer-than-supervisor drift), then on a controller-paused queue (e.g. equityfundingsource.md under a qflood pause) confirm session-check stderr now prints the "queue_paused is NOT a contradiction" preamble + recorded reason so an agent no longer stalls deciding operator-set-pause vs transient state. Root cause fixed: session-check printed the bare CONTINUATION_NO_STALL_GUIDANCE constant instead of continuation_guidance(pause_reason); resolution previously only reached preflight JSON.
- [/] [#sqedit-race] [operator-verify] #agent-doc-bug Live-IPC-supervisor race on direct queue edits. Phase 1 (#8bfz single live plugin owner per doc) SHIPPED 237f5fb2. Phase 2 (queue-edit lease) SHIPPED d9a0ae58. Phase 3 (idempotent malformed-queue normalization + render(parse(x)) fixed-point property test) SHIPPED 3c4c8bbb (pushed) — normalization via #qdup-bare-id / #qnoise-multiline-strike / #pushpinaccum; fixed-point proof tests render_parse_is_fixed_point_on_malformed_queue + render_parse_fixed_point_property_over_malformed_family, full suite + clippy green. REMAINING: Phase 4 [operator-gated] make install -> agent-doc admin recycle -> queue prune-noise this doc, confirm queue_stale_noise_lines drops to 0 and the prune sticks across a later preflight (no re-injection). Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [/] [#qheadstrike] [operator-verify] #agent-doc-bug Completed free-text queue heads (no #id) are not struck after a proven closeout. Repro: a single equityfundingsource.md cycle (2026-06-19) answered 4 operator queue heads, reached committed + session-check ok via the reset --from-current -> commit recovery path, yet preflight still showed queue_drainable_head_count=4 / queue_continuation_required=true, so the loop churns and the operator must clear them by hand. Three causes: (1) the finalize strike heuristic only consumes ONE free-text head per cycle; (2) the force-disk / reset->commit recovery paths bypass the strike entirely; (3) queue consume exact-matches and rejects when the live buffer mutated the head (pushpin glyph + item split: "snapshot head prompts do not match document head prompts"). Fix: strike ALL answered free-text heads, move the strike onto the committed seam so recovery paths converge, and normalize head matching. Plan: tasks/agent-doc/plan-freetext-queue-head-autostrike.md
- [/] [#tsiftmdcrash-verify] [operator-verify] Live-verify the #tsiftmdcrash fix (agent-doc submodule a281ae3e): capability-proof give-up no longer SIGTERMs the live hosted harness child. Needs operator install at a supervisor-recycle boundary (make install: cargo install --path . + cargo build --release + agent-doc lib-install — deferred from this session to avoid finalize-client-newer-than-supervisor drift). Then on a managed OpenCode/Codex session force a capability-proof failure (e.g. network blip) and confirm the harness pane stays alive with '<harness>_capability_proof_live_child_preserved reason=dispatch_gated_not_killed' in the session log and NO 'exit_code=143' / 'opencode_exit code=143' kill (the prior crash signature). Root cause was confirmed in tsift-v0.1.log: two opencode_exit code=143 SIGTERMs on pane %78 each at the same instant as opencode_capability_proof status=failed attempts=3. <!-- gate-verify verify="capability_proof_live_child_preserved" set_at="1781930602" -->
- [/] [#runexitrestart] [operator-verify] [recommended] #agent-doc-bug When the harness has cleanly EXITED (supervisor showing "Press Enter to restart, or q to exit") and an operator JB Run Agent Doc / queued prompt-bearing dispatch arrives, the route fails closed (dispatch-only busy -> fail closed) instead of restarting the harness then submitting. route.rs dispatch path has NO clean-exit handling — only session_restart (session_actor_cmd.rs:2235) checks pane_shows_clean_exit_prompt. live_pane_prompt_ready reads the restart screen as not-ready -> AliveBusy, so the dispatch barrier never submits. Fix: the managed route / supervisor idle-queue-watch should detect a clean-exit pane with prompt-bearing work, press Enter to restart the harness, wait for the dispatch-ready prompt, then submit, so a cleanly-exited harness never drops a Run Agent Doc. NOTE: the originally-reported SILENT loss was a stale-supervisor-at-hot-reload artifact already fixed in installed 0.34.18 (idle->ready reconcile now gated on prompt_ready==Some(true) + keyed off a real harness dispatch-ready prompt). Live-supervisor-critical (two-pane tmux verify) -> focused cycle, not in-loop.
- [/] [#jbrundispatch] [operator-verify] #agent-doc-bug JB Run Agent Doc route reliability: (1) after manual pane kill + autostart, Run Agent Doc stalls; (2) some Run Agent Doc ops do not submit to Claude Code (accepted_only dispatch with no submit). Both are route/tmux/startup-miss issues overlapping shipped work (#jbrunstart, startup-miss supersession, #qdispatchloss) and CANNOT be safely fixed blind in the invariant-heavy route.rs. NEEDS a live repro capture at stall time: agent-doc admin detect (cross-doc pane contamination?) + ops.log route trace + which pane state failed, to distinguish a real new bug from already-fixed behavior.
- [/] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block to the operator badly, and can appear not to submit at all. Variants: (1) a deliberate/admin pause renders as a raw red Error: toast instead of an actionable notification; (2) a churn-stop pause (reason: broken editor IPC, pausing to stop churn - the #41a32133 exception that blocks every caller including an explicit operator reopen so the route restarts the stale supervisor first) renders as NOTHING - JB Run Agent Doc just silently stalls with no notification; (3) operator report 2026-06-19: JB Run Agent Doc is not submitting / no visible submission result. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button; for the churn-stop/no-submit variant surface a restart-supervisor / resume action instead of silence. Operator action: install the updated JB plugin, then verify both the paused-queue notification and the churn-stop/no-submit notification render (live gate).
- [/] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. **Operator action: watch one live re-baseline cycle to verify the silent auto-rebase.** Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#gf5n] #numlistrev #agent-doc-bug Operator-reported: numbered/ordered lists rendered REVERSED (4,3,2,1) on an initial turn, boundary after the block. INVESTIGATED 2026-06-19: NOT reproducible in a clean stream/CRDT finalize — a 1-2-3-4 list renders 1-2-3-4 correctly (verified /tmp repro). It manifested only in the drift-heavy monsterrodholders turn, so it is a SYMPTOM of the CRDT merge-corruption family (#hap7/#qdup/#semmerge: the merge inverting list-item order under a live-buffer race), not an independent ordered-list render bug. Resolves when #hap7 scoped-merge lands. Kept open as a #hap7 verification check: confirm numbered-list order survives a merge-during-finalize cycle.
- [/] [#rt83] #qrepin #agent-doc-bug The active-queue stripped diff perpetually reports a phantom pushpin-add of the operator-verify head #qpauseux every preflight even when the doc and snapshot raw queue bodies are byte-identical, keeping no_changes:false and re-triggering the supervisor idle-watch. Investigated: sync_backlog_into_queue (entry_do_id/do_prompt_id), the #mirrorall mirror, and the #7r2s auto-pin (annotate_manual_queue_additions) all correctly recognize and skip the already-pinned bare-id head, so it is NOT a content mutation. Root is in the preflight stripped-diff/boundary-reposition path: commit repositions the boundary in the snapshot but skips the working-tree reposition while a live IPC listener is active, so the post-commit snapshot queue and the post-maintenance doc queue diverge on the pinned operator-verify head position/strip, surfacing a phantom pin add. Cosmetic (no corruption) but it sustains qchurn. Fix: normalize operator-verify mirror pins symmetrically in the stripped diff, or converge the snapshot boundary reposition with the working tree even under a live listener. Needs instrumented per-phase queue-body tracing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after #agentreloadrestart): add a Restart Agent JB plugin menu command — new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path; after Phase 1 re-resolves the harness on restart this brings up a changed agent. Decide alongside-vs-replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. LOGGING (operator-install live-gate prove/disprove): log the action invocation + resolved harness. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1 (foundational): make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude->opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). LOGGING (live-gate prove/disprove): emit harness_change_detected old/new, agent_restart_boundary_gate reason, agent_restart_performed to ops.log. SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: focused clean cycle with make check + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#cyh0] #fcczero #agent-doc-bug [operator-verify] JB File Cache Conflict still occurring; target is ABSOLUTE zero session-doc disk writes except genuine emergencies (everything via editor IPC). Audit (#smconv cycle) confirms the documented hot-path sites are already listener-guarded: post-commit worktree reconcile skips disk with a listener (#pcwcdiskfree), boundary reposition queues via file IPC and only disk-writes when IPC is Unavailable, queue/pending/status maintenance route through converge_or_disk_write (#fccqueue/#fccaudit); remaining disk writes are recovery/scaffold/migration emergencies by design. So the remaining FCC has an UNIDENTIFIED trigger. **Operator action: capture which exact action raises the dialog (the agent-doc command + ops.log line at that moment, e.g. write_authority action=... transport=disk) so the specific unguarded path can be found.** Likely candidates to check: the ack-content sidecar write, the VCS-refresh/save-document signal, or an editor-IPC convergence that the plugin still surfaces as a conflict. Note: this session most FCC came from manual `git checkout HEAD` recovery (a disk write), which #smconv removes the need for by merging drift instead of dropping responses.
- [/] [#8bfz] #fcconeowner #agent-doc-bug elect a SINGLE live IntelliJ plugin consumer per document so concurrent windows/instances cannot race patch application + saveDocument into File Cache Conflicts. Today each IntelliJ plugin process generates its own consumer id jetbrains-<pid>-<uuid> (TypingTracker.kt) and every live instance that has the project open watches .agent-doc/patches/ and consumes patches independently = N live consumers for one doc when N windows are open. #fccreap (0.34.8) reaps DEAD-pid consumer files but does not dedupe LIVE ones. Add single-owner election: the plugin acquires a per-document owner lease (lock file under .agent-doc/, or via the sessions.json registry) and non-owner instances defer patch application + saveDocument for that doc, so exactly one live consumer applies/saves. Owner lease is short-TTL/self-healing so closing the owner window hands ownership to another live instance. Plugin-side change (PatchWatcher + a lease) + binary lease primitive; operator-verify (needs multi-window live test).
- [/] [#1vhn] #routedeadshell agent-doc-bug: JB Run Agent Doc dispatched the agent-doc FILE trigger into a bare zsh % shell on recruit/virtuousai.md (harness had crashed/exited to a shell), so it did not submit (screenshot img_48.png, ~14:03, during a /model opus switch). REPRO (operator 2026-06-18): Run Agent Doc on virtuousai.md -> supervisor + tmux pane CRASHED 2 times -> 3rd attempt stalled -> operator manually ran agent-doc. So the supervisor/pane crash on this dispatch path is REPRODUCIBLE, not a one-off. Route must positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the harness prompt immediately before send to close the crash-mid-dispatch race; add a dead-harness route diagnostic + SimWorld coverage. Separately root-cause WHY the supervisor+pane crash repeats (the /model opus switch/restart is the lead). **Operator action: if it recurs, capture .agent-doc/logs/supervisor-stderr.log + the pane scrollback at the crash moment.** Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
