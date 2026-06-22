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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260622-014427.md`*

Compacted content:
❯ - Archived 5 response topic(s): JB Run Agent Doc "stalled without feedback" during auto-install; clear all gated review items → actionable backlog replacements; JB `Run Agent Doc` stalled without feedback; 2 more
❯ - Prior summary/context: prior compacted content: Archived 1 response topic(s): do [#pcwcwarn]

### Re: #opslogtrack verified live — opus-4-8

The supervisor recycled onto the new binary, so `#opslogtrack` is now live in `.agent-doc/logs/ops.log`. Confirmed directly from the log — two documents interleaved, each attributable:

```
[2026-06-22T01:46:32Z] queue_dispatch_skipped file=tasks/agent-doc/agent-doc-bugs2.md … doc=agent-doc-bugs2 session=b26b9957-… turn=cycle-1782092736424
[2026-06-22T01:46:32Z] queue_dispatch_skipped file=tasks/professional/equityfundingsource.md … doc=equityfundingsource session=62fe1f41-… turn=cycle-1782092755072
```

Every line carries the ISO timestamp plus the appended ` doc=<stem> session=<id> turn=<cycle_id>` suffix, and the `agent-doc-bugs2` vs `equityfundingsource` lines are now distinguishable by `doc=`/`session=` — the multi-document tracking you asked for. Marking `#opslogtrackverify` done.

Note: those same lines show `reason=queue_control_paused` / `queue_dispatch_skipped`, i.e. the supervisor is correctly respecting the halt — the frontmatter shows `queue: start` flipped back, but `admin queue pause` is still active so the supervisor isn't auto-dispatching. If you want the drain to actually resume, run `agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs2.md` (and confirm the JB editor IPC is reconnected). The head is still operator-verify `#tb4q`.
❯ Make the transport healthy. The end part of the response that guides me on how to advance the queue should have been placed in this document.

### Re: make the transport healthy + advance the queue — opus-4-8

> **Queue prompt:** Make the transport healthy. The end part of the response that guides me on how to advance the queue should have been placed in this document.

**Transport repair (agent-side, done this cycle):** the wedge is the JetBrains plugin's editor-IPC listener socket (a supervisor recycle alone didn't fix it — `admin inspect` showed `supervisor:fresh` but `projection_lag=true`). The agent-side lever is refreshing the cdylib the plugin hot-reloads:

1. `cargo build --release` (picks up the `#opslogtrack` source too)
2. `agent-doc lib-install` → `libagent_doc-0.34.35.so` refreshed; 2 controllers marked to recycle
3. `agent-doc admin recycle` → controller marked to recycle at the next idle boundary

The plugin hot-reloads the `.so` on its mtime watch and re-runs `agent_doc_start_ipc_listener`, which should re-establish the listener. If this very response committed through a clean editor-IPC path (no `live_prompt_drift`), the transport is healthy again. If `projection_lag`/`live_prompt_drift` persists, the plugin hasn't picked up the reload — then the reliable fix is an **IDE-side restart** (close/reopen the doc tab or restart IntelliJ); that is the one step I cannot perform from here.

### How to advance the queue (kept in the document, as requested)

1. **Clear the transport** (above) — confirm a clean closeout with no `live_prompt_drift`.
2. **Resume dispatch:** the queue is `admin queue pause`d (the `#qflood` flood-guard). Frontmatter is already `queue: start`. To let the supervisor auto-dispatch again:
   ```
   agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs2.md
   ```
3. **Operator-verify heads at the front** (`#tb4q`, `#k7mf`) need a live editor/pane and can't be agent-drained — drive them yourself, or move them below the agent code items so the drain doesn't stall on them.
4. **Agent-drainable code items** ready to drain on a healthy transport: `#qheadstrike` (kills the answered-head re-surfacing churn — highest leverage), then `#editorbufwin`, `#hap7`, `#fbwire`, `#j9ja`, `#agentreloadrestart`.
5. **Done this session:** `#opslogtrack` (live + verified), `#supinstallvis` filed.

### Re: do [#qheadstrike] — P2 commit-seam strike implemented — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#qheadstrike]


Implemented `#qheadstrike` P2 — the root fix for answered free-text heads re-surfacing (the `#rt83`/`#qflood` phantom-pin churn that dominated this session).

**What was already there (P1 + P3):** `strike_answered_free_text_queue_heads` already strikes *all* answered free-text heads regardless of position and is pushpin/fence/baseline-aware, with end-to-end tests. The matching was never the problem.

**The gap (P2):** the strike ran only on the finalize write path (`write.rs`). When `finalize` hit `live_prompt_drift` and the cycle closed out through the standalone `agent-doc commit` recovery path (`git.rs commit_with_outcome`) — exactly what's been happening all session under the wedged IPC — the strike never ran, so the answered head stayed unstruck and re-surfaced every cycle.

**Fix (agent-doc submodule `84a939f7`):** run the strike at the top of `commit_with_outcome`, before staging, sourcing the answered response from the durable capture ledger (`cycle_state.capture_id` → capture `response_body`). Now the strike is a property of reaching `committed` regardless of which path committed — `agent-doc commit`, `reset --from-current` → commit, and `--force-disk` all converge. Idempotent (already-struck heads are skipped), so the finalize path's earlier strike is unaffected; best-effort, never blocks a commit.

**Verification:** new test `commit_seam_strikes_answered_free_text_head_from_capture` (recovery-path parity + negative: unanswered head preserved); `make check` green (**4950** tests). Plan updated.

Marking `#qheadstrike` done. The remaining piece is the live recovery-path proof, filed as `[#qheadstrikeverify]` (operator-verify) — it goes live once the supervisor recycles onto `84a939f7`; ops.log will then show `commit_seam_free_text_strike ... struck=N` and answered heads will stop re-surfacing.

### Re: do [#qheadstrikeverify] — verification status — opus-4-8

`#qheadstrike` (P2 commit-seam free-text strike) is **code-complete but not yet live-provable** — this stays `[operator-verify]`.

- **Shipped**: submodule HEAD `84a939f7` ("qheadstrike P2: strike answered free-text heads at the commit seam"), make check green (4950). Superproject pin `7ef5256e4`.
- **Not live**: the installed `agent-doc` (0.34.35) predates `84a939f7` — preflight raises `stale_install`. The strike code path isn't running in any tmux/JB supervisor yet.
- **No proof in ops.log**: `grep commit_seam_free_text_strike .agent-doc/ops.log` → empty. Expected while the binary is stale.
- **Why it can't be agent-completed**: the proof needs (a) the new binary live via `make install` + supervisor recycle, and (b) a live finalize cycle that falls into `live_prompt_drift`/commit-recovery while answering a free-text queue head — both require a live editor/pane. Mid-session `make install` is also contraindicated (finalize client would outrun the running supervisor → exit-75 churn + dropped queue edits), so I did **not** install this turn.

**Operator steps to close it**: from `src/agent-doc`, `make install` (or `cargo install --path . && cargo build --release && agent-doc lib-install`), `agent-doc admin recycle`, then drive one cycle that answers a free-text queue head through a drift/commit-recovery finalize. Confirm ops.log shows `commit_seam_free_text_strike ... struck=N (#qheadstrike)`, the answered head is `~~struck~~`, and `queue_drainable_head_count` drops with no phantom re-surfacing.

(Two unpushed submodule commits `50d8f021`/`84a939f7` and stray `cargo fmt`-only edits in `merge.rs`/`decisions.rs`/`queue_consume.rs` are out of scope here — left untouched per the release-hold + scope discipline.)

### Re: do [#hap7] — per-structural-node CRDT merge wired into the orchestration path — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#hap7]


Done. `#hap7` (`#qcellmerge1`) is implemented, tested, and committed.

**Root cause found:** the per-component / per-keyed-child merge machinery (`merge_by_component` → `merge_aligned_nodes` → `reconcile_component_body`) already existed and was already used by the **FFI/editor** merge path — but the **orchestration** production path (`merge_contents_crdt` / `merge_contents_crdt_with_ops`, used by run/finalize/stream/realtime) still called the **whole-document** `crdt::merge`. That whole-doc blob merge is what let a concurrent operator edit to one queue item duplicate/splice adjacent structure (the virtuousai + efs queue-prompt duplication).

**Fix (submodule `33e1eee1`, superproject `68290edf7`):**
- Routed `merge_contents_crdt` / `merge_contents_crdt_with_ops` (diff-guess branch) through `crdt::merge_by_component`, so each `agent:*` component — and each keyed child inside list/exchange components — reconciles against its own base. The op-replay branch stays whole-doc (captured ops carry whole-document offsets).
- **Structural safety net:** if the per-node merged result no longer round-trips to the same top-level component-name sequence as `ours`, fall back to the proven whole-doc merge — never emits a malformed framing that would trip the exchange-tail repair.
- **Operator-order spine (`#qauthorder`):** list components (queue/backlog/review/done) use `theirs` (the live editor/disk side) as the order-union spine so an operator reorder survives the merge; `exchange` keeps `ours` (append-only history).

**Verification:** `make check` green — **4951 passed**, clippy clean. Added `crdt_merge_concurrent_queue_insert_no_structural_duplication` (a concurrent operator queue-insert during an agent response: the new item appears exactly once, no splice into the queue).

Committed locally only (no push/install — per the release-freeze; you're driving install+recycle). Goes live after `make install` + `admin recycle`. Filed `#hap7verify` `[operator-verify]` for the live no-duplication eyeball.
<!-- agent:boundary:9e3fd826 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#hap7]~~
- :pushpin: do [#fbwire]
- :pushpin: do [#j9ja]
- :pushpin: do [#qnodemerge4wire]
- :pushpin: do [#tb4q]
- :pushpin: do [#agentreloadrestart]
- :pushpin: do [#restartagentmenu]
- :pushpin: do [#k7mf]
- :pushpin: Add `Stop Agent` and `Restart Agent` in JB + VSCode menus.
- :pushpin: JB `Run Agent Doc` on monsterrodholders.md stalled after restarting the agent in both Claude Code + Codex.
- :pushpin: `agent-doc tasks/monsterrodholders.md` was added to the codex input but not submitted. Whatever wrote the prompt should also loop to check the input and submit it if not submitted.
- :pushpin: As I was writing queue items during a turn, an extra `/clear\n/agent-doc tasks/agent-doc/agent-doc-bugs2.md\n/clear` was written into the input and submitted.
- :pushpin: do [#qcm1verify]
- :pushpin: do [#s81q]: Rename "Restart Supervisor" to "Recycle Supervisor". Add Restart command alongside "Recycle Supervisor"
- :pushpin: do [#qstrikenote]
- :pushpin: do [#routeretest]
- :pushpin: [#pendaddorder]
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` does nothing. Subsequent JB `Run Agent Doc` eventually dispatches...it seems there's a delay before working. Please enable JB `Run Agent Doc` to dispatch immediately.
- :pushpin: JS `Clear Session Context` then JB `Run Agent Doc` sometimes has the dispatch storm (multiple duplicate prompts) and no submit.
- :pushpin: do [#arsm]
- :pushpin: do [#editorbufwin]
- :pushpin: do [#jbdisprecycle]
- :pushpin: do [#sqedit-race]: Validate if the fix is successful by looking at the logs. No `queue_stale_noise_lines` means success.
- :pushpin: do [#qpauseux]
- :pushpin: do [#8bfz]
- :pushpin: do [#cyh0]
- :pushpin: do [#1vhn]
- :pushpin: do [#jbrundispatch]
- :pushpin: do [#docdriftgrace]
- :pushpin: do [#runexitrestart]
- :pushpin: do [#gf5n]
- :pushpin: do [#supinstallvis]
- :pushpin: do [#1jec]
- do [#qheadstrikeverify]
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
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
- [ ] [#jpmt] [operator-verify] #hap7verify #agent-doc-bug Live-verify #hap7 per-structural-node CRDT merge (agent-doc submodule 33e1eee1, superproject 68290edf7). Code shipped + make check green (4951). Goes live after make install + admin recycle. Proof: in a live session, edit/insert ONE agent:queue item while an agent exchange response is in flight (the virtuousai/efs repro) and confirm the queue item appears exactly once with no adjacent-structure duplication; ops.log shows per-component merges (multiple short '[crdt] merge:' lines), not one whole-doc merge.
- [ ] [#qheadstrikeverify] [operator-verify] #qheadstrike Live-verify the P2 commit-seam free-text strike (agent-doc submodule 84a939f7, superproject 7ef5256e4). Code shipped + make check green (4950); goes live once the supervisor recycles onto the new binary. Proof: answer a free-text queue head in a cycle whose finalize falls into live_prompt_drift/commit-recovery (or reset --from-current -> commit), then confirm ops.log shows `commit_seam_free_text_strike ... struck=N (#qheadstrike)` AND the answered head is struck (~~...~~) with no phantom re-surfacing on the next preflight (queue_drainable_head_count drops).
- [ ] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [ ] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [ ] [#j9ja] Add distinctive SUCCESS ops.log markers at the remaining gated proof sites so operator live tests auto-verify (#optverify): agent_restart_performed (agentreloadrestart), restart-agent menu invoke (restartagentmenu), record_force_disk_fallback (fbwire Phase 2), and the route/plugin bucket. Landed already: auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted.
- [ ] [#editorbufwin] merge-core: P1 the #pcwc post-commit reconcile must preserve unconsumed user queue additions on disk (do NOT overwrite the disk addition with HEAD); P2 auto-recover the commit (automate the manual reset --from-current); P4 refresh the stale supervisor CRDT enabling the drift. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [ ] [#fbwire] Phase 2: wire convergence_gate_decision into the supervisor idle-queue-watch inter-item dispatch (gather committed/editor-converged/ipc-inflight==0/actor-idle; defer next trigger until Dispatch; on ForceDiskFallback do real --force-disk + record_force_disk_fallback). SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-fullboundary-convergence-gate.md
- [ ] [#qnodemerge4wire] Phase 4: thin JB DocumentListener.documentChanged + VS Code onDidChangeTextDocument reporters that split a replacement into delete+insert and call the agent_doc_record_editor_op FFI; verify byte-offset/Yrs unicode-offset semantics on non-ASCII edits. Needs plugin build. Plan: tasks/agent-doc/plan-percell-crdt-merge-isolation.md (Phase 4)
- [ ] [#jbdisprecycle] OPERATOR-TEST (code R1-R4 shipped submodule d21079b7): at a recycle boundary fire JB Run Agent Doc on a 2nd doc during the hot-reload burst; confirm a single dispatch_start_proof WITH submit and no stranded start_session failed. ops.log marker: route_dispatch_submit_recycle_settle.
- [ ] [#sqedit-race] OPERATOR-TEST Phase 4 (Phases 1-3 shipped 3c4c8bbb): after make install + admin recycle, queue prune-noise this doc; confirm queue_stale_noise_lines drops to 0 and sticks across a later preflight. Plan: tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md
- [ ] [#qpauseux] OPERATOR-TEST (plugin render; needs JB plugin build): install updated plugin, verify a paused-queue block renders as an info/action notification with a Resume button, and the churn-stop/no-submit variant surfaces a restart-supervisor/resume action instead of silence (not a raw red Error toast).
- [ ] [#8bfz] single live JB plugin consumer per document (per-doc owner lease; non-owners defer patch apply + saveDocument) so concurrent windows cannot race into File Cache Conflicts. Plugin PatchWatcher + binary lease primitive; operator multi-window live test.
- [ ] [#cyh0] OPERATOR-CAPTURE: the remaining File Cache Conflict has an unidentified trigger (hot-path disk writes are already listener-guarded). Capture which exact action raises the dialog (agent-doc cmd + ops.log line, e.g. write_authority action=... transport=disk). Likely: ack-content sidecar write, VCS-refresh/save-document signal, or an editor-IPC convergence the plugin surfaces as conflict.
- [ ] [#1vhn] route dead-shell: positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the prompt immediately before send; add dead-harness diagnostic + SimWorld. Operator capture supervisor-stderr + scrollback if it recurs. Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
- [ ] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [ ] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [ ] [#gf5n] verification check under hap7: confirm numbered/ordered-list order survives a merge-during-finalize cycle (the reversed-list symptom was a CRDT merge-corruption artifact, not an independent render bug; resolves when hap7 scoped-merge lands).
- [ ] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [ ] [#pendaddorder] Repeatable `--pending-add` block-prepends in flag order instead of composing as the documented "at the beginning (repeatable)" per-item prepends, so ONE `agent-doc write` with N `--pending-add` flags lands items REVERSED vs N sequential writes, and the agent:queue mirror inherits the reversal. Live repro 2026-06-21 equityfundingsource.md: `--pending-add [#msgmail4] --pending-add [#msgmail3] --pending-add [#msgmail2] --pending-add [#msgmail1]` -> backlog AND queue top-down = msgmail4,3,2,1 (caller expected 1,2,3,4 per the per-item-prepend mental model). Plan (DO NOT IMPLEMENT): (1) define ONE semantics for repeatable --pending-add and document it (recommend preserve flag order as final top-down order = what you read is what you get; route strict-order callers to --pending-add-after chains); (2) update help text + SKILL.md/pending-ops "put new items at the beginning" to state the multi-flag order; (3) make the agent:queue mirror order deterministically match the backlog order; (4) add a test asserting multi-flag --pending-add order + queue-mirror parity.
- [ ] [#routeretest] [operator-verify] #agent-doc-bug Live re-test the route/dispatch/stall reports on the POST-RECYCLE fresh binary (06933b72, installed + admin-recycled 2026-06-21). The reported symptoms — JB Run Agent Doc stalled without feedback; agent-doc trigger typed into the Codex/Claude input but not submitted; stray /clear+trigger injected — are ALREADY implemented in shipped code: route/dispatch.rs not_dispatched full-trigger re-send (while acceptance.not_dispatched, ~L856, #jbrundispatch directive 2), send_direct_pane_enter_resubmit_until_stable Enter-resubmit loop (~L779/895), and the dispatch_inject attempt=N ops.log marker (#rdypoll). The operator was most likely observing these on the STALE pre-06933b72 supervisor binary replaced this session. Verify on the fresh binary: drive a JB/Codex Run Agent Doc dispatch and confirm ops.log shows a single dispatch_inject attempt=1 + a submitted trigger (no stacked un-submitted copies, no stray /clear). If a symptom persists on the fresh binary, it is a genuine residual gap to dig into.
- [ ] [#qstrikenote] [recommended] #agent-doc-bug #qstrikeexplain Phase 1 follow-up: explicit deterministic explanation note when #ftstrike auto-strikes a free-text queue head. Phase 2 (do not strike in-flight/mid-typed heads; baseline-gated) SHIPPED agent-doc submodule 06933b72. The strike already requires the response to quote the struck head in a blockquote; add a binary-injected one-line auto-struck note so the operator sees an explicit statement. Decide deterministic-injection vs a finalize-field per all-deterministic-in-binary. Plus a SimWorld case that a strike emits the note. Plan: tasks/agent-doc/plan-queue-strike-explanation.md (Phase 1 + Phase 3 SimWorld).
- [ ] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [ ] [#k7mf] [operator-verify] #g9d7verify #agent-doc-bug Live editor-less repro of #g9d7/#6b5hprimary (agent-doc submodule 260d61b3). Code shipped+pushed: the run_ipc no-delivery bail routes content_ours to the guarded controller-host disk write when there is no plugin-owner lease (live_editor_endpoint_attached=false), reusing decide_editorless_disk_fallback; a live editor still fails closed. After a make install, run agent-doc finalize in a Claude Code pane with NO JB IDE attached and confirm ops.log shows run_ipc_editorless_disk_fallback transport=disk_fallback editor_endpoint=absent plus a committed cycle, with no --force-disk and no recovery=retry_without_disk_write. Operator-gated: needs a real CLI pane.
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
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
