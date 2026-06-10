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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260610-000136.md`*

Compacted content:
- Archived 13 response topic(s): do [#pcpc5cut]; do [#opsproof-falsepos]; Do not use memory to recover; 10 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:18051a77 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: :round_pushpin: do [#s760wire]~~
- :pushpin: do [#xdotool-lvbatch]
- :round_pushpin: do [#lvbatch]
- :pushpin: :round_pushpin: do [#saevon]
- :round_pushpin: do [#bkx9perid]
- :round_pushpin: do [#tmux-switch-lag]
- :round_pushpin: do [#exch-intermix-verify]
- :round_pushpin: do [#postcommit-ipc-worktree-corruption]
- ~~do [#dropqueue-consumed-falsecount]~~
- :round_pushpin: do [#lvbatch]
- Automatically add the env vars to ~/ so I can test. I can restart.
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
- [ ] [#xdotool-lvbatch] Automate the #lvbatch live-verification batch with xdotool keystroke injection so the agent can drive AND verify the IPC/CRDT live-load items without a human typing. Plan: tasks/agent-doc/plan-xdotool-live-verification.md. Verified env: X11 reachable (DISPLAY=:0, no Wayland), xdotool 4.x at /usr/bin/xdotool, live IntelliJ editor targetable by doc-basename title. Guards: type ONLY into a throwaway .agent-doc/live-repro/xdotool-<case>.md scratch doc (never the real working doc), focus-guard active window before every type, run from a FRESH session, time keystrokes by ops.log IPC-apply marker not sleep. Covers #exch-intermix-verify (live_prompt_drift_auto_recovered), #postcommit-ipc-worktree-corruption (assert worktree==HEAD post-closeout), #saevon (flip EARLY_ACK_ENABLED + build/lib-install, expect early-ack pending before apply, no false-success), and the #lvbatch markers. Does NOT cover #s760wire (code, not typing). Deliverable: reusable harness script + SimWorld coverage for offline-assertable parts.
- [ ] [#postcommit-ipc-worktree-corruption] #agent-doc-bug After a clean finalize/commit (session-check OK, HEAD correct), the live IPC listener rewrites the WORKING TREE with a stale/spliced buffer: the latest ### Re: response is deleted from the visible file and its body bullets are spliced into an EARLIER ### Re: block, plus duplicate queue-prompt blockquotes appear. HEAD stays correct; the NEXT preflight misreads the corruption as user content_edit prompt_targets (user_intent_prompt_changes non-empty), risking a response built on a corrupted exchange. Repro: live IntelliJ/IPC-active session, finalize a response that consumes a queue do[#id]; git show HEAD is clean but the working tree drifts post-commit. Commit logs show "IPC reposition boundary signal sent" + "skipping working-tree boundary reposition - IPC listener active". Likely the post-commit boundary-reposition IPC signal or JB PatchWatcher re-applying a stale .agent-doc/patches entry at the wrong buffer offset. Same family as #adoc-queue-ipc-buffer-divergence / supervisor self-race; root fix is the supervisor-in-process migration (tasks/agent-doc/plan-supervisor-in-process.md). Files: write.rs commit reposition signal path; editors/jetbrains PatchWatcher; ffi.rs IPC listener. Add SimWorld coverage asserting working-tree == HEAD after a committed closeout.
- [ ] [#exch-intermix-verify] [recommended] Live-verify the #exch-intermix auto-recovery in a real IPC-listener-active drift repro: drive a closeout that hits live_prompt_drift_after_preflight, confirm agent-doc commit lands the response (no 'manual cleanup' wedge) and the session-log/ops.log shows 'live_prompt_drift_auto_recovered'. Pairs with the #lvb-run operator batch. Submodule 723b998c, binary 0.34.0.
- [ ] [#s760wire] Pre-emptive /clear: predicate ContextResetDecision.shouldClearBeforeDispatch + clear_threshold_for_doc + agent_doc_queue_context_reset opt-in ALL SHIPPED. Blocker = live context-usage % source. SubmitAction.kt is a thin dispatcher (no ctx%); per Shared Foundation it belongs in route.rs. Observed footers vary by harness+config (Claude custom statusline 'ctx:NN%', Codex 'Context NN% used') so TUI-scrape is wrong for the shipped product — derive ctx% from session transcript token usage / context window instead. Remaining (CLI-side, no JB install): add harness-aware transcript-based ctx% reader in route.rs, gate dispatch via shouldClearBeforeDispatch, emit '[s760] clear-decision optIn=.. threshold=.. pct=.. clear=..', send /clear when true; live-verify.
- [ ] [#saevon] Activate early-ack: the protocol is landed + dormant (EARLY_ACK_ENABLED=false in ipc_socket.rs) and now logs "[ipc-socket] early-ack pending emitted before apply" on a successful emit. Remaining: flip EARLY_ACK_ENABLED true, run the live batch (#lvb-run) to confirm the pending ack fires before apply with no false-success / false ack-timeout, then ship. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md. Absorbs review #ipc-early-ack + the early-ack half of #pcp5.
- [ ] [#bkx9perid] Enable per-id response-loss detection at closeout. The dormant pure detector shipped (#z2jy, session_check.rs). Remaining actionable work, fresh NON-DEGRADED session: route captured cycles through a per-id directive_response_materialized check (fire only when a reaped do#id head has no ### Re: in content or any HEAD compact archive) WITHOUT false-positiving a cycle that answers do[#A]+do[#B] under one ### Re: heading; test against a real #ipc-crdt-response-drift repro before enabling at the closeout-governing guard. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record. Absorbs review #bkx9/#compact-reap-no-response-record.
- [ ] [#tmux-switch-lag] #agent-doc-bug Slow tmux pane switch when switching between agent docs in the editor: the target doc pane is briefly visible in the stash window before it swaps into 0:agent-doc, so the operator sees a flash of the stash layout mid-switch. Likely the passive editor-sync fast-handoff path (sync.rs sync --no-autostart) selects/raises the hidden actor pane before moving it out of stash. Actionable, fresh session: make the stash->agent-doc pane move atomic (move first, then select) so no intermediate stash frame is shown; add SimWorld coverage for the switch ordering. Files: sync.rs passive fast-handoff path.
- [ ] [#lvbatch] OPERATOR live-verification batch, replaces gated review #lvb-run. No new JB install needed: plugin 0.2.158 unchanged, binary+cdylib refreshed to HEAD this cycle. Drive one mid-active-turn Codex/IntelliJ pane through the batch; agent checks logs. Markers: #4wxr claim cross-session-reject in claim.rs + ClaimAction.kt; #9adk run/clear coalesced in route.rs and sync.rs; #f5d2/#pcp6 ops.log live_buffer_classify + write.rs visible_write_live_buffer_matches_disk and visible_write_deferred_current_changed; #saev early-ack pending emitted before apply only AFTER #saevon flips EARLY_ACK_ENABLED. Reproduces #ipc-crdt-response-drift on closeout.
- [ ] [#pcpc5live] OPERATOR-RUN 08b cutover. STEP 1 (shadow) + STEP 2 (dual-write) PROVEN 2026-06-09: ops.log has write_authority gate=shadow action=would-route AND gate=dual-write action=routed (event 1781047338). REMAINING: step 3 pcpc5d/e — delete out-of-process supervisor direct-disk writers + demote editor WatchService to read-only; needs live IntelliJ/Codex editor-boundary proof per spec 08b 131-133. This IS the supervisor-in-process migration (tasks/agent-doc/plan-supervisor-in-process.md, #pcpc1..#pcpc5); run as a fresh focused session.
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
