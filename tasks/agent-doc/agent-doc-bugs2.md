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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260619-041018.md`*

Compacted content:
- Archived 5 response topic(s): [#1vhn] route dispatching the trigger into a crashed/exited harness shell; queue auto-re-fired despite the pause; do [#8bfz]; 2 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: do [#pk3f] — #semmerge-ack-turn Phase 4 — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#pk3f]


Shipped Phase 4 of the semantic-AST-merge plan (`#pk3f` / `#semmerge-ack-turn`), the rung that turns the `requires_ack` outcomes from the convergence semantic merge into an acknowledgement the agent's *next* turn actually sees. agent-doc `a4bda87f`, superproject pointer `99e64d1bc`, version `0.34.14` built + installed + cdylib refreshed.

**The gap it closes.** `#smconv` (0.34.7) already merges node-disjoint operator↔agent edits and lets the operator win same-node conflicts, but when it couldn't apply the agent's change verbatim — operator deleted an agent-edited node, overrode the same node, or revived an agent-deleted node — it only logged `semantic_merge_ack_pending` to `ops.log`. Nothing carried that into the agent's next response, so the operator silently won with no acknowledgement in the exchange. The `write/ipc.rs` TODO at the ack site was exactly Phase 4.

**What changed (carry-forward via cycle_state, as the backlog specified):**
- `write/ipc.rs` now persists each `requires_ack` `AckRequest` via the new `cycle_state::record_semantic_merge_acks` alongside the existing log marker.
- `CycleState` gains `pending_semantic_merge_acks: Vec<PendingSemanticMergeAck>`. `start_preflight` carries **un-surfaced** acks forward exactly one cycle and marks them surfaced.
- Lifecycle is driven by a per-ack `surfaced` flag, **not** a `cycle_id` comparison — `cycle_id` is millisecond-derived and two back-to-back cycles can collide, which would have leaked or dropped acks. (This was the one subtle correctness trap; the `surfaced` flag sidesteps it entirely.)
- `preflight` surfaces them as a structured `semantic_merge_acks` field **plus** a companion `semantic_merge_ack_pending` `PreflightWarning`, so the skill's existing "surface warnings" path drives the acknowledgement — zero SKILL.md/audit-docs churn.
- New stable `AckReason::token()` is the wire format cycle_state persists (`operator_deleted_agent_edited_node` / `same_node_operator_override` / `operator_revived_agent_deleted_node`).

The merged document is **unchanged** — operator content already won — so this adds only the courtesy acknowledgement; no content is created or lost.

**Verification.** `make check` green (EXIT=0, no FAILED/error). 5 new tests: `ack_reason_tokens_are_stable` (markdown-ast); `record_semantic_merge_acks_tags_current_cycle_and_dedupes`, `start_preflight_carries_prior_cycle_acks_forward_exactly_once` (two-boundary surface-once-then-drop), `semantic_merge_ack_recorded_after_carry_chains_to_next_cycle` (cycle_state); `preflight_output_semantic_merge_acks_roundtrip` (preflight). SPEC (`07-closeout-commands.md`) + VERSIONS + plan updated; latest agent-doc CI green pre-push.

Remaining semmerge phases: `#smqstrike` (Phase 3, queue-node-strike) and `#smsim` (Phase 5, SimWorld matrix) — both still queued.

### Re: JB Run Agent Doc consumes + loses an uncommitted queue item (equityfundingsource.md) — opus-4-8

> **Queue prompt:**
>
> JB `Run Agent Doc` in equityfundingsource.md moves an uncommitted queue item from the queue into the agent prompt. The queue item is gone and the turn stalls uncommitted with incorrect agent input.


Triaged and filed as a new agent-doc bug — **`#qdispatchloss`** (plan: `tasks/agent-doc/plan-uncommitted-queue-item-dispatch-loss.md`, backlog item added after `#qdurcrash`).

**What the bug is.** When you type a queue item into the editor and trigger `Run Agent Doc` before it's committed, the route dispatch path picks that *uncommitted* item (editor-buffer-only — the JB plugin may have synced it to disk, but it's not in a committed snapshot) as the dispatch prompt. Because the head is consumed/activated without a committed-snapshot backing, the consumption never lands, the item is dropped, and the turn stalls uncommitted with the wrong (possibly half-typed) prompt.

**Root-cause hypothesis (code).** Route reads the queue head straight from disk — `std::fs::read_to_string(file)` in `inactive_route_queue_head` (`route.rs:2618`), `activate_existing_route_queue_head` (`route.rs:2670`), and the active-head read (`route.rs:2291`) — and treats it as consumable without proving it exists in the committed snapshot. An operator edit that's only in the live buffer therefore gets dispatched and lost. This is the **no-crash sibling of `#qdurcrash`**: same "queue edit lived only in the editor buffer and was never journaled before it was lost," but triggered by dispatch racing your keystrokes rather than by a process crash.

**Fix direction (in the plan, to design):** before consuming a head, prove it's backed by the committed snapshot — commit the inbound queue edit first (shared durable-journal primitive with `#qdurcrash`) — or debounce dispatch until the buffer settles, and never feed a divergent/uncommitted line as the prompt. Add a `route_dispatch_uncommitted_head` ops.log diagnostic so the live gate can prove/disprove.

This is a triage/filing cycle (the document's `#agent-doc-bug` pattern), not an implementation — `#qdispatchloss` joins the queue-durability family (`#qdurcrash`, `#orphanqhead`) for a focused fix cycle.

**Session note:** large exchange + 3 no-op closeouts in the last 30 min (session-accretion `warn`), but the queue is active so I'm not stalling to compact.

### Re: do [#smqstrike] — semmerge Phase 3 verified complete — opus-4-8

`#smqstrike` (route `queue_consume` strike through the merged node tree so a drifted operator queue edit never aborts/loses the head strike) was **already delivered incrementally** by the `#smconv` node-keyed strike work — re-implementing it would have been wasteful and risky. I verified the contract holds across all three write paths and added explicit merged-tree coverage. agent-doc `d7fc816f`, superproject `2f77353d0`.

**Why it's already done (all three paths route the strike by node, not by line):**
- **Editor IPC:** `queue_consume` convergence emits per-node patches via `build_ipc_node_patches_json`/`queue_consume_node_patches` — a `strike` op keyed by `node_key` for the consumed head. Concurrent operator edits to *other* queue nodes are never touched, so they survive.
- **Disk fallback:** `queue_consume_reconciles_diverged_snapshot_instead_of_bailing` — when the post-merge queue diverges from the snapshot (a concurrent operator add), consume **reconciles** (the merged document wins): head struck, the operator's added item preserved, no hard-bail/orphan.
- **content_ours convergence:** the `#smconv` live-prompt-drift path runs `semantic_merge`, which treats `struck` as a **node-disjoint flag-edit**, so a struck head merges cleanly with a concurrent operator add.

**Coverage added:** `smqstrike_struck_head_survives_concurrent_operator_queue_add` (markdown-ast) — base has `do [#a]`, agent strikes it, operator concurrently adds `do [#b]`; asserts the merged tree keeps `~~do [#a]~~` **and** `do [#b]`, reparses to `[a, b]`, and needs no ack (node-disjoint). `make check` green; test + docs only, no behavior change.

**Remaining semmerge phase:** `#smsim` (Phase 5, SimWorld matrix for the operator↔agent concurrent-edit matrix) — the last one.

### Re: do [#smsim] — semmerge Phase 5 done; all 5 phases complete — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#smsim]


`#smsim` shipped — deterministic SimWorld coverage of the operator↔agent concurrent-edit matrix against `semantic_merge`. **This completes all five semmerge phases.** agent-doc `0b0216b2`, superproject `ce4faea60`.

**What landed:**
- `SimWorld::converge_semantic_merge(base, agent, operator, active_component)` — runs the production `semantic_merge_scoped` (the same `exchange`-active scoping the real `#smconv` `try_semantic_merge_convergence` applies), adopts the merged doc, and classifies the transition-matrix outcome into four new coverage counters.
- Four `semmerge_sim_*` scenarios locking the merge/IPC data-loss family:
  1. **Node-disjoint** — agent strikes a queue head while the operator concurrently adds a new item; both survive, no ack.
  2. **Same-node operator-wins (active area)** — agent + operator edit the same `exchange` node differently; operator content wins **and** an ack is raised.
  3. **Turn-active gating** — the *identical* same-node conflict OUTSIDE the active area auto-resolves operator-wins with **no** ack noise (`#smturnactive`).
  4. **Operator-deleted-agent-edited-node** — deletion stands (node not resurrected) and a `OperatorDeletedAgentEditedNode` ack is raised.

`make check` green; test-only (no production behavior change).

**Semmerge plan status — all phases complete:** Phase 1 `#semmerge-owner` ✅ · Phase 2 `#semmerge-converge-adapter` (`#smconv`) ✅ · Phase 3 `#smqstrike` ✅ · Phase 4 `#pk3f`/`#semmerge-ack-turn` ✅ · Phase 5 `#smsim` ✅. The `live_prompt_drift` / `reset --from-current` data-loss class this plan targeted is now node-merged and regression-locked.

Next queue heads: `#cyh0` (FCC zero-disk audit, operator-verify), then the remaining `do #id` items and free-text bug reports.

### Re: do [#cyh0] — FCC zero-disk audit complete; gated on operator live capture — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#cyh0]


Did the agent-doable half of `#cyh0` (audit the candidate session-doc disk-write sites) and **gated** it — the only remaining work is the operator's live FCC-trigger capture, which I can't produce.

**Audit conclusion — every binary-side session-doc disk write in the closeout/queue paths is listener-guarded:**
- `converge_document_or_disk` (`converge.rs:900`) and `converge_or_disk_write` (`:928`) both call `try_editor_converge` first and only disk-write when **no** JB listener is active.
- The live-prompt-drift auto-recovery `atomic_write` (`converge.rs:248`) is blocked by `auto_recovery_disk_write_blocked reason=editor_ipc_unconfirmed` (returns before writing) whenever the editor IPC is active/unconfirmed.
- `queue_consume` / `queue_done_id_mark` route through `converge_document_or_disk`; the bare `atomic_write` sites are the `skip_visible_guard` force-disk **repair** bypass (deliberate operator escape hatch).
- The **ack-content sidecar** writes to `.agent-doc/…` sidecar files, **not** the session document — it cannot raise an FCC on the doc.

So no unguarded binary-side session-doc disk write remains in the hot path. The persistent FCC dialog this session (`postcommit editor buffer refresh failed … IPC ack status error`) is the editor-IPC convergence *failing* and the buffer refresh erroring — a plugin-side surface, not a binary disk write. (My own `git checkout HEAD` recoveries this session WERE session-doc disk writes, but those are manual recovery, not agent-doc code.)

**Gated, not done:** `--pending-gate cyh0`. The remaining piece is genuinely operator-only — capture the exact action that raises the dialog plus the `ops.log` line at that moment (e.g. `write_authority action=… transport=disk`, or a plugin saveDocument/VCS-refresh event), so the specific unguarded/plugin path can be pinned. Likely non-closeout candidates to watch: the post-commit VCS-refresh/save-document signal and editor-IPC convergence the plugin still surfaces as a conflict.

This resolves the `#cyh0` queue head; the drainable tail (`#qpausego`, `#qdurcrash`, …) is now next.
<!-- agent:boundary:832b32f1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#pk3f]~~
- ~~JB `Run Agent Doc` in equityfundingsource.md moves an uncommitted queue item from the queue into the agent prompt. The queue item is gone and the turn stalls uncommitted with incorrect agent input.~~
- ~~:pushpin: do [#smqstrike]~~
- ~~:pushpin: do [#smsim]~~
- ~~:pushpin: do [#cyh0]~~
- :pushpin: do [#qpausego]
- :pushpin: do [#qdurcrash]
- :pushpin: In equityfundingsource.md After a recycle restart + queue continuation, the prompt is not submitted after the restart.
- :pushpin: do [#733r]
- :pushpin: do [#docdriftgrace]
- :pushpin: do [#agentreloadrestart]
- :pushpin: do [#restartagentmenu]
- :pushpin: do [#qpauseux]
- :pushpin: do [#tsiftmdcrash]
- :pushpin: do [#orphanqhead]
- :pushpin: As I was typing in a queue item into virtuousai.md, parts of the document was duplicated, corrupting the document. The CRDT/semantic merge should be constrained only to the structural part that it applies to. Do multiple merges for each structural part as needed. This should remove the possibility of structural corruption and should reduce conflicts. If the operator deletes parts of the structure in the document and there is new agent content for a deleted structure, mention the fact in the exchange but do not attempt to merge...as the in-editor document is the source of truth.
- :pushpin: Turn crashed on virtuousai.md. ![img_49.png](img_49.png)
---
Initial response for a turn renders numeric lists in reverse order:
```
4. **Review back-catalog dry-run** → #review-backcatalog step 1 — read-only segment count, no send.
3. **6/19 ROAS re-audit** → #pmax-pause-decision + #29h5 — read-only PMax + accessories audits; pause/prune stay operator calls.
2. **Post-6/14 GTM-v6 conversion QA** → #convfix-verify (this one **unblocks #nbsl2** non-brand Search) — read-only Ads+GA4 parity.
1. **6/19 offline-conversion re-check** → #2qrx-metrics-lag + #conv-backfill-615-617-verify — read-only Ads action 7649654590 metrics + Woo reconcile.
<!-- agent:boundary:a5b6cae3 -->
```
---
monsterrodholders.md sent duplicate uncommitted response
```
- **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
- **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- **Kept 2 false-positive matches** (real open work): `#2b8q` (live operator-gated NB-Search follow-up — HydroWeb rebalance + phase-2 enable), `#conv-backfill-615-617-verify` (distinct 6/15–6/18 batch, now-due on/after today — not the 5/29–6/14 batch `#2qrx-verify` covered).
- **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done; verify handed to archived `#2qrx-verify`), `#7m2s` (GTM v6 fix shipped 6/14; verify owned by open `#convfix-verify`), `#ef5h` (duplicate of archived `#fishloc-curated-rollout`; adapter blocker tracked by open `#3456`).
- **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).
<!-- agent:boundary:7c96f9ce -->
```
---
:pushpin: JB `Run Agent Doc` on this file
```
[sync] recovered live pane %125 for session b26b9957 via supervisor pid 2252604
[sync] recovered live pane %126 for session 9342c5f7 via supervisor pid 2744553
resync: stash pane %93 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[sync] recovered live pane %125 for session b26b9957 via supervisor pid 2252604
[sync] recovered live pane %126 for session 9342c5f7 via supervisor pid 2744553
resync: stash pane %112 is unregistered but still hosts live supervisor tsift-v0.1 — skipping kill
[route] target tmux session: 0
[route] authoritative actor for tasks/agent-doc/agent-doc-bugs2.md projected busy but the live pane proves a dispatch-ready prompt (generation 547); repairing stale busy projection to ready and dispatching instead of queuing
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=broken editor IPC: content_ours adoption (live_prompt_drift, #semmerge) loses every exchange-finalize response AND consumes queue heads without landing answers (recovered last response via raw git). Remaining heads are already-answered/filed (virtuousai crash=#routedeadshell), cross-doc (equityfundingsource), or operator-gated. Pausing to stop churn. Resume in a clean session (refresh IntelliJ buffer + recycle binary) and take #smconv (the converge.rs fix for this very bug) first. receipt_id=3986 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=17 blocked_head_sha256=c5f1145ffb6ebc69ee2482a58184cc574633d8b472888314df6cbc6159cfe443 trigger_bytes=45 trigger_sha256=ce2c7a444f4ae0b6cfce11a6ceac977be5b3811138cfe31a7252a021e9b2c9ad
```
---
:pushpin: More IPC/CRDT drift on virtuousai.md
```
  Note on the session: this hit heavy IPC/CRDT drift — finalize repeatedly raced the live JetBrains editor buffer
  (live_prompt_drift_after_preflight, then a post-commit worktree merge that spliced my stealth bullets into the wrong
  response). I hand-rebuilt the exchange cleanly and reset the sidecars to land a correct commit. The letter and
  recommendation content were unaffected.
```
```
● The editor IPC is persistently wedged this turn (no_ack) — identical to the previous turn, where the queued response
  landed automatically on the next /agent-doc trigger once the JB plugin applied the patch. I've retried the
  editor/CRDT path (finalize → commit → write --commit); all leave the response safely queued. Per the closeout rules I
  will not force a direct disk write and will not auto-loop on this unproven-IPC state.
```
---
:pushpin: JB `Run Agent Doc`
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3977 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
---
:pushpin: JB `Run Agent Doc` stalled on this document.
```
[sync] recovered live pane %92 for session b26b9957 via supervisor pid 1535801
[sync] recovered live pane %105 for session 9342c5f7 via supervisor pid 1101234
resync: stash pane %93 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[route] target tmux session: 0
[route] authoritative actor for tasks/agent-doc/agent-doc-bugs2.md projected busy but the live pane proves a dispatch-ready prompt (generation 535); repairing stale busy projection to ready and dispatching instead of queuing
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=broken editor IPC: content_ours adoption (live_prompt_drift, #semmerge) loses every exchange-finalize response AND consumes queue heads without landing answers (recovered last response via raw git). Remaining heads are already-answered/filed (virtuousai crash=#routedeadshell), cross-doc (equityfundingsource), or operator-gated. Pausing to stop churn. Resume in a clean session (refresh IntelliJ buffer + recycle binary) and take #smconv (the converge.rs fix for this very bug) first. receipt_id=3960 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control blocked_head_bytes=44 blocked_head_sha256=4ec56b3745f04f438f52f8bffc1d7fe8b9e8e0964028b4db7008fffeb4bf4ff5 trigger_bytes=45 trigger_sha256=ce2c7a444f4ae0b6cfce11a6ceac977be5b3811138cfe31a7252a021e9b2c9ad
```
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[preflight] review: 14 item(s), 14 gated
[route] active closeout for tasks/professional/equityfundingsource.md could not be drained before reroute; existing queue head "Why is the https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=application > Available deal > Application > Application progress > Accepted not checked?" remains queued behind the closeout ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain
```
---
JB `Run Agent Doc` on equityfundingsource.md
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3934 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
- do [#qdispatchloss]
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
- [ ] [#orphanqhead] Orphaned id-backed queue head has no drain path: after a backlog item is reaped via --done, if the matching id-backed queue head [#id] strike was lost to IPC drift, the head is orphaned — --done <id> is then a no-op ("already resolved") and queue consume rejects id-backed heads ("reap via --done"), so the head is unreachable. preflight marks it queue_drainable_head_count=0 but never auto-strikes it, leaving a permanent phantom head that re-fires the auto-loop. Repro: tasks/recruit/virtuousai.md #p3kj on agent-doc 0.34.11; compounded by a wedged editor IPC (live IntelliJ buffer) refusing queue/pending writes (ack_mismatch/send_failed). Fix: auto-strike id-backed heads whose backing item is already done/absent during preflight reconciliation, or allow queue consume --id <id> to strike an orphaned id-backed head.
- [ ] [#qpausego] #agent-doc-bug `agent-doc admin queue pause <FILE>` is accepted (durable receipt, e.g. ids 68/70 at generation 548) but does NOT halt a `go`-mode queue: after pause, preflight still reports queue_active=true / queue_paused=None / queue_trigger=auto and the supervisor/auto-loop keeps re-dispatching the next head (observed 3 consecutive auto-re-dispatches of agent-doc-bugs2.md after an accepted pause). Either the controller pause receipt is not consulted by the auto-dispatch path for a queue whose `agent:queue` header carries `go`, or preflight does not surface the controller pause (queue_paused stays None despite an accepted pause). Investigate: (1) does the supervisor idle-watch / queue auto-dispatch read the controller pause receipt before injecting, and does `go` override it? (2) should preflight expose queue_paused=true and drop queue_continuation_required when a controller pause is active? Today the only effective halt for a go-queue is editing `queue: stop` frontmatter / removing `go` by hand (done this session), which races the live IPC supervisor. Fix so an accepted `admin queue pause` durably suppresses auto-dispatch (and is reflected in preflight) even for go-mode queues.
- [ ] [#qdurcrash] #agent-doc-bug queue-edit durability across a supervisor/pane crash+restart: operator adds a queue item, the turn starts, the supervisor+tmux pane crashes and restarts, and the operator added queue item is GONE (virtuousai.md repro, same crash event as #routedeadshell/#1vhn). Root-cause hypothesis: the operator queue add lived only in the editor buffer / supervisor in-memory CRDT and was never journaled to a crash-durable on-disk store before the process died, so restart reloaded from the last snapshot/HEAD (predating the add) and dropped it. Fix (agent-actionable, binary-side): persist inbound operator queue patches to the on-disk snapshot/journal immediately on receipt (before the supervisor mutates queue state / before ack), and add a supervisor-startup reconcile that merges the on-disk document queue against the reloaded snapshot, so a crash+restart replays the pending queue edit instead of losing it. Verify: add a queue item, kill -9 the supervisor mid-turn, restart, confirm the item survives. Distinct from #semmerge (live-merge drop during a turn) and #routedeadshell (route dispatch into a dead shell).
- [ ] [#qdispatchloss] #agent-doc-bug #qdispatchloss JB Run Agent Doc consumes an UNCOMMITTED operator queue item (editor-buffer-only / synced to disk by the plugin but not git-committed) as the dispatch prompt and loses it: the item is moved into the agent prompt (possibly half-typed = incorrect agent input), consumed/activated without a committed-snapshot backing, then dropped so the turn stalls uncommitted. No-crash sibling of #qdurcrash. Route selects the queue head from disk via std::fs::read_to_string with no committed-snapshot proof (route.rs:2291 active head, 2618 inactive_route_queue_head, 2670 activate_existing_route_queue_head). Fix direction: refuse to CONSUME a head not backed by the committed snapshot (commit the inbound queue edit first, shared journal with #qdurcrash) OR debounce dispatch until the editor buffer settles; add a route_dispatch_uncommitted_head ops.log diagnostic for the live gate. Plan: tasks/agent-doc/plan-uncommitted-queue-item-dispatch-loss.md
- [ ] [#733r] #qheadsync agent-doc-bug: answered/filed queue heads are not reliably reconciled to the queue (free-text reports left un-struck; done-reaping orphans the matching id-backed queue head; manual strikes do not persist through finalize content_ours; backlog-to-queue sync has no sweep for items missed in prior turns and items can be lost). Fix at closeout in the binary. Plan: tasks/agent-doc/plan-answered-queue-head-autostrike.md
- [ ] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block as a raw red Error: toast instead of an actionable queue paused — resume notification. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button rather than a dispatch error. **Operator action: install the updated JB plugin, then verify the paused-queue notification renders (live gate).**
- [ ] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. **Operator action: watch one live re-baseline cycle to verify the silent auto-rebase.** Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported that a tsift.md agent-doc turn crashed and killed the session process while the tmux pane stayed active. Supervisor pid 1742954 is alive; pane %78 OpenCode TUI shows Thinking normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe deleted) as a crash. Not reproducible from logs alone. **Operator action: paste the exact crash error text / pane capture from the crash moment, or the specific cycle id, so it can be root-caused.**
- [ ] [#agentreloadrestart] restart-agent Phase 1 (foundational): make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude->opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). LOGGING (live-gate prove/disprove): emit harness_change_detected old/new, agent_restart_boundary_gate reason, agent_restart_performed to ops.log. SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: focused clean cycle with make check + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#restartagentmenu] restart-agent Phase 2 (after #agentreloadrestart): add a Restart Agent JB plugin menu command — new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path; after Phase 1 re-resolves the harness on restart this brings up a changed agent. Decide alongside-vs-replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. LOGGING (operator-install live-gate prove/disprove): log the action invocation + resolved harness. Plan: tasks/agent-doc/plan-restart-agent.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
