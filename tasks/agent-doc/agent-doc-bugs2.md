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
queue: stop
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-182549.md`*

Compacted content:
- Archived 1 response topic(s): clear all gates → actionable backlog, + two new bugs triaged
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: go — drained head #lvb-run, found+fixed a stale install — opus-4-8

Processing the queue head `do [#lvb-run]`. Before touching anything I checked the working tree and the install state, and the picture differs from what the queue assumes.

**The queue head `#lvb-run` was not actually runnable — its install prerequisite was silently stale.**
- The item claims "binary 0.34.0, cdylib lib-installed." True by *version*, but the installed `~/.cargo/bin/agent-doc` was built **13:55** and the cdylib **13:53** — both *older* than the diagnostics commit `e9146147` (**14:06**) that adds the `live_buffer_classify` / `early-ack` log markers the verification greps for. Same version string `0.34.0`, no bump, so the staleness was invisible.
- I ran `make install`. Binary now **14:33**, cdylib **14:34**, both carrying the diagnostics. Verified in the installed binary: `live_buffer_classify decision` present, `visible_write_live_buffer_matches_disk` present.
- The 5th marker, `[ipc-socket] early-ack pending emitted before apply`, is **compiled out** (dead-code-eliminated) while `EARLY_ACK_ENABLED=false`. It only appears once **#saevon** flips that flag — so the early-ack leg of `#lvb-run` is gated behind `#saevon`, not the binary.

**`#lvb-run` is ready for you to run — 4 of 5 legs.** Install the signed plugin `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.158-signed.zip`, drive one mid-active-turn pane through the batch, then grep:
- `#4wxr` claim stderr → `[claim] cross-session-reject pane_id=.. pane_session=.. configured=..`
- `#9adk` idea.log → `[run]/[clear] coalesced duplicate`
- `#f5d2/#pcp6` ops.log → `live_buffer_classify decision=.. reason=..` and write.rs → `visible_write_live_buffer_matches_disk` / `visible_write_deferred_current_changed`
- `#saev` (only after `#saevon` flips the flag + reinstall) session-log → `[ipc-socket] early-ack pending emitted before apply`

I **gated `#lvb-run`** (prep complete; awaiting your live pane) rather than marking it done — it is the one legitimate operator-live-verification gate, not an agent gate. If you want the early-ack leg in the same pass, say so and I will flip `#saevon` + reinstall first.

**This cycle also reproduced `#exch-intermix` live.** The first `finalize --stream` hit `IPC proof insufficient ... invariant=live_prompt_drift_after_preflight`: pending mutations landed but the `### Re:` body fragmented out of the exchange (snapshot 12489b vs file 11082b). I recovered per the documented path — `git checkout HEAD -- FILE` → `reset --from-current --preserve-session` → `preflight` → this `finalize --force-disk`. Concrete fresh repro for `#exch-intermix` / `#pcpconsol`.

**Working-tree note (not committed):** the submodule had 22 files of uncommitted churn. I checked all of it — **not feature work**: pure rustfmt-1.9.0 reflow (zero logic), a tsift code-nav block bump (`v0.1.65→0.1.66`), and a `lazily 0.10.1→0.10.2` lockfile bump. `make check`/CI don't enforce fmt, so the reflow is disposable. I left the tree as-is (the `lazily` bump is now baked into the binary I installed). Your call: commit just the `lazily` bump and discard the fmt reflow, or revert all of it.

**Remaining queue:** `#pcpconsol` (root-fix, multi-day fresh session), `#pcpc5cut` (depends on `#pcpconsol`), `#s760wire` (live IntelliJ pane), and `#bkx9perid` (needs a real `#ipc-crdt-response-drift` repro) are each fresh-session/live-pane gated per their own text — not work to drain in this saturated session. The two pure-code items are `#exch-intermix` (finalize auto-recover on `live_prompt_drift_after_preflight`) and `#tmux-switch-lag` (atomic stash→agent-doc pane move). Reorder either to the head and I will take it now; otherwise the highest-value next move is you running `#lvb-run` live.

I did **not** auto-loop — the head is operator-gated.
<!-- agent:boundary:6cd33d0f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" -->
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
- [ ] [#install-stale-guard] [recommended] #agent-doc-bug Warn when the installed agent-doc binary or cdylib is older than the current submodule HEAD even at the SAME version string. This session found the installed binary (built 13:55) + cdylib (13:53) predated the 14:06 diagnostics commit e9146147 (no version bump), so tmux/JB sessions silently ran a pre-diagnostics binary and the early-ack/live-buffer markers were missing/dormant. Actionable: at preflight/status, compare installed binary+cdylib build time against HEAD commit time and emit a stale-install warning recommending make install. Files: preflight.rs / status.
- [ ] [#exch-intermix] #agent-doc-bug RESPONSE INTERMIXING / LOSS in agent:exchange — live repro captured THIS session 2026-06-09: a finalize --stream hit "IPC proof insufficient ... invariant=live_prompt_drift_after_preflight" when the operator re-invoked /agent-doc mid-finalize; the pending mutations (backlog/review) landed but the ### Re: response body was LOST (never reached exchange), and the snapshot/visible-file drifted (snapshot 10587b vs file 9437b) so the next commit was refused as "manual cleanup". This is #ipc-crdt-response-drift on the IPC/CRDT closeout path. ROOT-FIX is #pcpconsol (single ordered write queue removes the supervisor/finalize self-race). NEAR-TERM actionable in a FRESH session: when finalize detects live_prompt_drift_after_preflight, auto-recover (reset --from-current --preserve-session -> re-finalize --force-disk) instead of leaving the response lost + commit wedged. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md.
- [ ] [#pcpconsol] ROOT-FIX (priority per operator). Supervisor-in-process consolidation / single-process control plane (spec 08b): make the supervisor an in-process adapter so the session actor owns ONE ordered write queue + ONE controller-owned filesystem watcher; demote plugin WatchService to read-only buffer reporting + thin EDT apply+ack shim. Decomposed into dependency-ordered phases #pcpc1 to #pcpc5 in tasks/agent-doc/plan-supervisor-in-process.md. Run as a FRESH focused session starting at #pcpc1 (NOT a saturated queue-drain). Root-fixes #ipc-crdt-response-drift and R1/R2/R5/R6. Absorbs review #pf15/#pcp3/#pcp4/#pcp7 + the degraded-routing half of #pcp5. Large/multi-day.
- [ ] [#pcpc5cut] 08b migration cutover, decomposed one actionable id per gate (pcpc5a..e: shadow -> dual-write -> read-switch -> authority -> remove out-of-process supervisor + direct-disk writers). DEPENDS ON #pcpconsol. Run in a fresh non-saturated session with live-tmux + CLI + SimWorld + live IntelliJ proof. Plan: tasks/agent-doc/plan-supervisor-in-process.md. Absorbs review #21w2/#pcpc5.
- [ ] [#s760wire] Wire the pre-emptive /clear feature (predicate ContextResetDecision.shouldClearBeforeDispatch + binary agent_doc_clear_threshold already shipped). Remaining actionable work, fresh session at a live IntelliJ pane: (1) read the live Claude Code context-usage percent from the pane, (2) acquire threshold (preflight session_accretion.clear_threshold) + the agent_doc_queue_context_reset opt-in, (3) call the predicate in SubmitAction pre-dispatch and emit an idea.log decision line "[s760] clear-decision optIn=.. threshold=.. pct=.. clear=..", (4) trigger session clear when true, verify live. Absorbs review #s760/#clear-opt-in-threshold.
- [ ] [#saevon] Activate early-ack: the protocol is landed + dormant (EARLY_ACK_ENABLED=false in ipc_socket.rs) and now logs "[ipc-socket] early-ack pending emitted before apply" on a successful emit. Remaining: flip EARLY_ACK_ENABLED true, run the live batch (#lvb-run) to confirm the pending ack fires before apply with no false-success / false ack-timeout, then ship. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md. Absorbs review #ipc-early-ack + the early-ack half of #pcp5.
- [ ] [#bkx9perid] Enable per-id response-loss detection at closeout. The dormant pure detector shipped (#z2jy, session_check.rs). Remaining actionable work, fresh NON-DEGRADED session: route captured cycles through a per-id directive_response_materialized check (fire only when a reaped do#id head has no ### Re: in content or any HEAD compact archive) WITHOUT false-positiving a cycle that answers do[#A]+do[#B] under one ### Re: heading; test against a real #ipc-crdt-response-drift repro before enabling at the closeout-governing guard. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record. Absorbs review #bkx9/#compact-reap-no-response-record.
- [ ] [#tmux-switch-lag] #agent-doc-bug Slow tmux pane switch when switching between agent docs in the editor: the target doc pane is briefly visible in the stash window before it swaps into 0:agent-doc, so the operator sees a flash of the stash layout mid-switch. Likely the passive editor-sync fast-handoff path (sync.rs sync --no-autostart) selects/raises the hidden actor pane before moving it out of stash. Actionable, fresh session: make the stash->agent-doc pane move atomic (move first, then select) so no intermediate stash frame is shown; add SimWorld coverage for the switch ordering. Files: sync.rs passive fast-handoff path.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#lvb-run] OPERATOR live-verification session (no agent gate; user runs, files bugs, agent checks logs). All five fixes are code-complete + installed (binary 0.34.0, cdylib lib-installed, JB plugin 0.2.158 unchanged). Install signed JB plugin 0.2.158 + binary, drive ONE mid-active-turn Codex/IntelliJ pane through the batch, capture logs. Grep markers: #4wxr claim stderr "[claim] cross-session-reject pane_id=.. pane_session=.. configured=.."; #9adk idea.log "[run]/[clear] coalesced duplicate"; #f5d2/#pcp6 ops.log "live_buffer_classify decision=.. reason=.." + write.rs "visible_write_live_buffer_matches_disk"/"visible_write_deferred_current_changed"; #saev (after #saevon flips the flag) session-log "[ipc-socket] early-ack pending emitted before apply". Reproduces #ipc-crdt-response-drift on closeout (watch for emptied backlog / orphaned ids / duplicated review / split boundary). Supersedes review #xkpf + #live-verify-batch + the live legs of #f5d2/#pcp6.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
