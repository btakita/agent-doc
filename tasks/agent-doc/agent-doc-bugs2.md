---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-215339.md`*

Compacted content:
- Archived 3 response topic(s): queue head; do [#pcpconsol]; do [#pcpc5cut]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:8c8ffeba -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- :pushpin: :round_pushpin: do [#s760wire]
- :pushpin: :round_pushpin: do [#saevon]
- :pushpin: :round_pushpin: do [#bkx9perid]
- :pushpin: :round_pushpin: do [#tmux-switch-lag]
- :pushpin: do [#exch-intermix-verify]
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
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
- [ ] [#exch-intermix-verify] [recommended] Live-verify the #exch-intermix auto-recovery in a real IPC-listener-active drift repro: drive a closeout that hits live_prompt_drift_after_preflight, confirm agent-doc commit lands the response (no 'manual cleanup' wedge) and the session-log/ops.log shows 'live_prompt_drift_auto_recovered'. Pairs with the #lvb-run operator batch. Submodule 723b998c, binary 0.34.0.
- [ ] [#s760wire] Wire the pre-emptive /clear feature (predicate ContextResetDecision.shouldClearBeforeDispatch + binary agent_doc_clear_threshold already shipped). Remaining actionable work, fresh session at a live IntelliJ pane: (1) read the live Claude Code context-usage percent from the pane, (2) acquire threshold (preflight session_accretion.clear_threshold) + the agent_doc_queue_context_reset opt-in, (3) call the predicate in SubmitAction pre-dispatch and emit an idea.log decision line "[s760] clear-decision optIn=.. threshold=.. pct=.. clear=..", (4) trigger session clear when true, verify live. Absorbs review #s760/#clear-opt-in-threshold.
- [ ] [#saevon] Activate early-ack: the protocol is landed + dormant (EARLY_ACK_ENABLED=false in ipc_socket.rs) and now logs "[ipc-socket] early-ack pending emitted before apply" on a successful emit. Remaining: flip EARLY_ACK_ENABLED true, run the live batch (#lvb-run) to confirm the pending ack fires before apply with no false-success / false ack-timeout, then ship. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md. Absorbs review #ipc-early-ack + the early-ack half of #pcp5.
- [ ] [#bkx9perid] Enable per-id response-loss detection at closeout. The dormant pure detector shipped (#z2jy, session_check.rs). Remaining actionable work, fresh NON-DEGRADED session: route captured cycles through a per-id directive_response_materialized check (fire only when a reaped do#id head has no ### Re: in content or any HEAD compact archive) WITHOUT false-positiving a cycle that answers do[#A]+do[#B] under one ### Re: heading; test against a real #ipc-crdt-response-drift repro before enabling at the closeout-governing guard. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record. Absorbs review #bkx9/#compact-reap-no-response-record.
- [ ] [#tmux-switch-lag] #agent-doc-bug Slow tmux pane switch when switching between agent docs in the editor: the target doc pane is briefly visible in the stash window before it swaps into 0:agent-doc, so the operator sees a flash of the stash layout mid-switch. Likely the passive editor-sync fast-handoff path (sync.rs sync --no-autostart) selects/raises the hidden actor pane before moving it out of stash. Actionable, fresh session: make the stash->agent-doc pane move atomic (move first, then select) so no intermediate stash frame is shown; add SimWorld coverage for the switch ordering. Files: sync.rs passive fast-handoff path.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#pcpc5cut] 08b migration cutover, decomposed one actionable id per gate (pcpc5a..e: shadow -> dual-write -> read-switch -> authority -> remove out-of-process supervisor + direct-disk writers). DEPENDS ON #pcpconsol. Run in a fresh non-saturated session with live-tmux + CLI + SimWorld + live IntelliJ proof. Plan: tasks/agent-doc/plan-supervisor-in-process.md. Absorbs review #21w2/#pcpc5.
  - [ ] [#pcpc5cut-x78t] DONE (out-of-cycle, plain session, agent-doc 600797b3): document-write authority rungs pcpc5a (shadow), pcpc5b (dual-write), pcpc5c (authority) shipped behind the default-off `AGENT_DOC_WRITE_AUTHORITY` env gate. The central `write::atomic_write` for editor-visible docs now routes through the session-actor ordered write queue (`write_queue`) when advanced to dual-write+, removing the in-process supervisor/finalize interleave (R6 self-race / exit-75) at the root; a thread-local owner-scope re-entrancy guard prevents mailbox deadlock; `.agent-doc/` sidecars never routed; instant rollback by unsetting the env. SimWorld + unit coverage, `make check` green, both binaries rebuilt+installed. Spec realized in 08b §"Document write and watch authority"; VERSIONS.md Unreleased.
  - [ ] [#pcpc5cut-4eyc] REMAINING (live-gated, stay under #xkpf): pcpc5d/pcpc5e — delete the out-of-process supervisor direct-disk writers + demote the editor plugin WatchService (JetBrains/VS Code) to read-only buffer reporting + single controller-owned watcher. These need a live IntelliJ/Codex editor-boundary proof a plain session cannot provide. Operator next step: run a session with `AGENT_DOC_WRITE_AUTHORITY=shadow` to confirm the would-route ops.log targets, then `=dual-write` to exercise the routed path live before the removal rungs.
- [/] [#lvb-run] OPERATOR live-verification session (no agent gate; user runs, files bugs, agent checks logs). All five fixes are code-complete + installed (binary 0.34.0, cdylib lib-installed, JB plugin 0.2.158 unchanged). Install signed JB plugin 0.2.158 + binary, drive ONE mid-active-turn Codex/IntelliJ pane through the batch, capture logs. Grep markers: #4wxr claim stderr "[claim] cross-session-reject pane_id=.. pane_session=.. configured=.."; #9adk idea.log "[run]/[clear] coalesced duplicate"; #f5d2/#pcp6 ops.log "live_buffer_classify decision=.. reason=.." + write.rs "visible_write_live_buffer_matches_disk"/"visible_write_deferred_current_changed"; #saev (after #saevon flips the flag) session-log "[ipc-socket] early-ack pending emitted before apply". Reproduces #ipc-crdt-response-drift on closeout (watch for emptied backlog / orphaned ids / duplicated review / split boundary). Supersedes review #xkpf + #live-verify-batch + the live legs of #f5d2/#pcp6.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
