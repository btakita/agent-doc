---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260622-205008.md`*

Compacted content:
❯ - Archived 1 response topic(s): do [#rtndsync]
❯ - Prior summary/context: prior compacted content: Archived 7 response topic(s): editor stale-state rollback and slow Run Agent Doc; build and ...

### Re: Codex Run Agent Doc submit retries - gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` does not submit with Codex or is very slow to submit. The submit retries should be very frequent. At least 1/sec if not more frequent.

Implemented the routed direct-pane submit retry fix.

What changed:
- The direct-pane submit acceptance timeout is now 1 second instead of 5 seconds.
- The acceptance budget is now 1.5 seconds, keeping polling slack without delaying the next resend.
- The default ENTER resubmit cap is now 30 attempts, so the default recovery window remains about 30 seconds while retrying at least once per second while the `agent-doc <FILE>` trigger remains visibly drafted.
- Added a regression test that enforces the <=1s acceptance timeout and >=30s default retry budget.
- Updated `specs/11-debounce.md` and `VERSIONS.md` with the Codex/Claude/OpenCode direct-pane retry contract.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/flow/routed_reopen.rs`
- `src/agent-doc/agent-doc-orchestration/src/route/dispatch.rs`
- `src/agent-doc/agent-doc-orchestration/src/route.rs`
- `src/agent-doc/specs/11-debounce.md`
- `src/agent-doc/VERSIONS.md`
- parent `src/agent-doc` submodule pointer

Verification:
- `cargo fmt` passed.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p agent-doc-orchestration direct_pane'` passed (`tart-8a15cae551`).
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'` passed (`tart-18ea110685`).
- `tsift --envelope digest-runner --kind log --path . --shell-command 'make install-full'` passed (`tart-e049c3ac37`).
- `agent-doc --version` reports `agent-doc 0.34.35`.

Commits/push:
- agent-doc submodule `ddf454d7` (`fix: retry routed submits every second`) pushed to `btakita/agent-doc main`.
- parent local pointer `3e248c681` (`chore: update agent-doc submit retries`) committed in the current divergent parent checkout.
- parent remote pointer `e98fd76e4` (`chore: update agent-doc submit retries`) pushed to `btakita/agent-loop main` from a clean temporary worktree, so unrelated parent local history and dirty files were not pushed.

Closeout recovery:
- `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` rebuilt stale sidecars after the stream snapshot guard refused the first write.
- CLI `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` then succeeded and committed snapshot recovery as `52f071c90`.
- `agent-doc admin recycle` marked the controller to recycle at the next idle boundary because preflight reported the route-owned supervisor was still mapping an older binary.
- `agent-doc session restart-supervisor /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` refused safely because the current pane is alive-busy, so I did not force-interrupt the running turn.
- The stale-supervisor IPC retry materialized a body-only response without the `### Re:` heading; I repaired that malformed exchange through `agent-doc patch`, then reset sidecars from the repaired current file before committing.

CI:
- Latest `btakita/agent-doc` GitHub Actions `CI` run for `ddf454d7` was `in_progress` when checked (`27983797253`, started `2026-06-22T21:02:40Z`).
- The parent `agent-loop` repo still reports no workflow named `CI`.

Preflight notes:
- Preflight reported semantic-completion ambiguity warnings for unrelated backlog/review IDs; I left those untouched.

Escalation: none.

Confidence: high for the retry cadence and local install/test verification; remote CI was still running at closeout.

### Re: strict closeout and stale supervisor recycle — gpt-5

> **Queue prompt:**
>
> Instead of blindly continuing while the supervisor is stale, you should recycle the supervisor then continue.


> **Queue prompt:**
> Instead of blindly continuing while the supervisor is stale, you should recycle the supervisor then continue.

Implemented this as a product fix and deployed it locally.

Root cause addressed: strict `finalize` / session-document `write --commit` could still accept a non-empty body-only `patch:exchange` in a stale-supervisor or IPC retry path, so bullets/status text could materialize without a `### Re:` assistant heading. The write path now rejects that shape before mutation, after the existing empty-body proof so empty shells still get the old “no real response-body write” error. The only body-delta exception is the legitimate streaming case where the live exchange tail already proves a streamed `### Re:` heading after the current prompt, which avoids duplicating the heading during orchestrate finalization.

Stale-supervisor guidance is now aligned across `queue_continuation`, the bundled/installable skill text, and `runbooks/commit.md`: degraded IPC/semantic warnings do not stop a queue drain, but a stale-binary supervisor should recycle/yield at the boundary and then continue on the fresh binary. `make install-full` installed `agent-doc 0.34.36` and auto-marked 2 controllers to recycle at their next idle boundary.

Changed in `src/agent-doc`:
- `agent-doc-orchestration/src/write/materialize.rs`
- `agent-doc-orchestration/src/write/run_entry.rs`
- `agent-doc-orchestration/src/write/normalize.rs`
- `agent-doc-orchestration/src/queue_continuation.rs`
- `src/skill.rs`
- `.claude/skills/agent-doc/SKILL.md`
- `runbooks/commit.md`
- `specs/07-closeout-commands.md`
- `Cargo.toml`, `Cargo.lock`, `pyproject.toml`, `VERSIONS.md`
- `.github/workflows/release.yml` after the first release run exposed the missing path-dependency checkout.

Verification:
- `cargo test -p agent-doc-orchestration strict_template_response_heading`
- `cargo test orchestrate_streams_step_patchback_before_finalize`
- `cargo test finalize_stream_rejects_empty_exchange_shell_before_commit`
- `make check` passed after the version bump.
- `make install-full` passed; `agent-doc --version` reports `agent-doc 0.34.36`.
- Latest CI on `main` is running for `29d4f41e`; the previous latest CI was green before this push.
- The first `v0.34.36` Release run failed because the Release workflow did not clone sibling path dependencies. I fixed the workflow, pushed `29d4f41e`, repointed `v0.34.36` to it, and the superseding Release run has passed the path-dependency checkout steps and is compiling.

Commits pushed in `src/agent-doc`:
- `daf5e727` `fix: require strict closeout response headings`
- `29d4f41e` `ci: checkout path deps in release workflow`

The superproject `src/agent-doc` pointer is included as a sibling closeout commit.
<!-- agent:boundary:fbbac828 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~Instead of blindly continuing while the supervisor is stale, you should recycle the supervisor then continue.~~
- I'm still seeing JB `Clear Session Cache` dialogs. Is the CPC instance up to date? Are all supervisors instances up to date? If this is still possible with the current codebase, these cases should use our node merge workflow.
- It does not seems like the JB `Run Agent Doc` submit retries are being applied correctly. Still does not submit.
- JB `Run Agent Doc` still does not submit. The submit retry poll should be 500ms and attempt for 30s...only if the input contents are the prompt itself. Other automated additions to the prompt should be forbidden. If the user modifies the agent prompt, stop the retry polling.
---
Fix the root cause of this issue that occurred in this document.
```
• The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
  content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
  supervisor onto the installed binary without interrupting this turn.
```
```
• Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
  normalize the interrupted cycle itself before I touch the component through a lower-level patch.

• Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
    or open-cycle resolution, not a metadata operation
    [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` to absorb an already-visible response)

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
  doc component patcher to replace only the malformed exchange component with the complete response text while
  preserving the newly added queue item.
```
```
• The partial IPC apply inserted only the body bullets and auto-struck the head, without the ### Re: header or queue
  prompt. A new queue item was also added while I was closing out; I’m going to repair the malformed closeout first,
  then the loop can move to the next queue head.
```
```
• Session-check still fails because the exchange tail ends with the compacted Session Summary, not the response. I’m
  reordering the exchange so the Session Summary stays first and the completed ### Re: response is the final exchange
  entry.
```
---
JB `Run Agent Doc` is still either stalling or very slow. Completely useless at the moment.
```
[route] target tmux session: 0
[route] Still waiting for codex (1s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (3s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (4s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (6s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (7s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (9s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (11s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (12s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (14s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (15s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (17s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (18s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (20s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (21s)... last line:   reverse-i-search:
[route] Still waiting for codex (23s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (24s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (26s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (27s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (29s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (30s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (32s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (33s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (35s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (36s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (38s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
[route] Still waiting for codex (39s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (41s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (42s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (44s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (46s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (47s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (49s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (50s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (52s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 6% use
[route] Still waiting for codex (53s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 9% use
[route] Still waiting for codex (55s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 9% use
[route] Still waiting for codex (56s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 9% use
[route] Still waiting for codex (58s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 9% use
[route] Still waiting for codex (59s)... last line:   gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 9% use
Error: dispatch-only codex reopen refused to inject into pane %24 for tasks/professional/equityfundingsource.md because the latest run is still booting and never reached a dispatch-ready prompt (timed_out); wait for the pane to become ready and reroute again ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=wait_for_dispatch_ready_prompt
```
---
JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The session switched from claude to codex, but the actor record did not switch.
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/software/agent-loop.md is bound to harness claude-code, not codex
```
---
- :pushpin: do [#1jec]
- :pushpin: do [#fbwireverify]
- :pushpin: do [#qnodemerge4verify]
- :pushpin: do [#tb4q]
---
:round_pushpin: Fix the root cause of this issue that occurred in this document.
```
• The write recovery path hit the same stale-supervisor proof guard: the patch was delivered, but the materialized
  content was missing the response body. I’m checking the admin/session controls for a non-destructive way to get the
  supervisor onto the installed binary without interrupting this turn.
```
```
• Because the malformed response is already in the exchange, I’m checking whether agent-doc repair --apply-recovery can
  normalize the interrupted cycle itself before I touch the component through a lower-level patch.

• Ran agent-doc repair --apply-recovery /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  └ [repair] recovery [open_cycle] not auto-applied: auto-apply withheld — recovery requires a preserved response body
    or open-cycle resolution, not a metadata operation
    [repair] run: finish the response, then `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` (or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/
    agent-doc-bugs2.md` to absorb an already-visible response)

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The repair command will not auto-apply this state; it needs the response finished first. I’m going to use the agent-
  doc component patcher to replace only the malformed exchange component with the complete response text while
  preserving the newly added queue item.
```
---
- do [#cancelturnverify]
- do [#qcm1verify]
- do [#arsm]
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
- [ ] [#jjds] [operator-verify] Confirm in a live editor/pane that a stale-binary supervisor recycles at the next idle boundary and resumes queue drain on agent-doc 0.34.36.
- [ ] [#cancelturnverify] [operator-verify] #agent-doc-bug Live-verify Cancel Turn (agent-doc submodule b4471d05; committed+pushed, make check green, JB 0.2.176 + VS Code 0.2.31 built, NOT installed). After installing the plugins: (1) while a turn is RUNNING, invoke Cancel Turn (JB menu / VS Code command) and confirm the turn is interrupted (ops.log cancel_turn_performed action=interrupt_active_turn) and the agent stays open; (2) while the agent is IDLE, invoke Cancel Turn (even repeatedly) and confirm it is a NO-OP (ops.log cancel_turn_noop reason=idle_no_active_turn) and the agent is NOT closed — the core safety requirement. Verify per harness: Codex/OpenCode (Escape/C-c, never raw Ctrl+C that would close on idle) and Claude.
- [ ] [#qnodemerge4verify] [operator-verify] #agent-doc-bug #qnodemerge4wire Phase 4 live non-ASCII verification (agent-doc submodule 4650054c; base-hash FFI + JB 0.2.173 + VS Code 0.2.29 reporters committed+pushed, make check + JB gradle + tsc/tests green, cdylib lib-installed). REMAINING operator-gated: (1) install the JB plugin zip from disk (build/distributions/agent-doc-jetbrains-0.2.173-signed.zip) + restart IDEA, and install the VS Code .vsix (editors/vscode/agent-doc-0.2.29.vsix) + reload window — the Kotlin/TS reporter changes need a plugin reinstall, the cdylib FFI is already hot-reloaded. (2) In a live editor, type a NON-ASCII edit (e.g. insert/delete around "café 日本 😀") into a session doc while an agent merge runs; confirm ops.log shows editor_op_recorded kind=insert/delete with BYTE offsets (not UTF-16) and that the concurrent merge aligns to the real edit boundary (no character interleaving / mojibake). The base-hash gate + replay==theirs gate make a misaligned op fall back to the diff-guess safely, so a wrong offset cannot corrupt — verification confirms the ops are ACCEPTED (editor_ops_for_base match), not merely safe.
- [ ] [#fbwireverify] [operator-verify] #agent-doc-bug #fbwireverify Live two-pane make tmux-ci acceptance for #fbwire/#fullboundary Phase 2 (agent-doc submodule 85b173fb; committed+pushed, make check green 4956, clippy clean; NOT installed per STOP-cutting-releases). After make install (cargo install --path . + cargo build --release + agent-doc lib-install) + agent-doc admin recycle at a recycle boundary, drive two live panes: (1) a slow-ACK editor drain must never pile IPC inflight > 1 — confirm ops.log shows convergence_gate_defer lines (unmet=inflight_drained) before each dispatch and NO inflight>1; (2) a wedged (no-ACK) editor must trip the bounded CONVERGENCE_GATE_TIMEOUT_MS (30s) → ERROR-level convergence_gate_force_disk_fallback severity=error + a loadable .agent-doc/playback/<hash>/<cycle>.json artifact, and the drain proceeds (never permanently stuck). Operator-gated: needs two live editor/CLI panes the driving session cannot host.
- [ ] [#arsm] [operator-verify] #pcwcwarnverify #agent-doc-bug Live-verify the #pcwcwarn post-commit stale-exchange reconcile (agent-doc submodule 5f3fc07d, superproject db7fc3594). Code shipped+pushed+make-check-green (4947). Runs in the binary commit path + editor IPC, so it goes live once the supervisor adopts the new build (admin recycle done). REMAINING (operator-gated, needs a live editor): drive one edit-during-finalize cycle with a stale > Queue prompt blockquote left inside the exchange in a live JB/VS Code buffer; confirm ops.log shows postcommit_exchange_reconciled reason=stale_editor_exchange AND zero recurring postcommit_worktree_check match=false drift on the next cycle (no manual git checkout HEAD needed).
- [ ] [#tb4q] [operator-verify] #qfocsupverify #agent-doc-bug Live-verify #qfocsup supervisor clear-and-continue for [focused-cycle] heads (agent-doc submodule 67829f3f; committed, make check green, NOT installed per STOP-cutting-releases). After make install + agent-doc admin recycle: with a [focused-cycle] head at the queue head and no in-session-drainable work, confirm the supervisor idle-watch force-clears the session (/clear) and re-dispatches the head to a fresh session instead of going idle. Proof: session-check ui_outcome=deferred_for_supervisor_drain on the yielding turn, then ops.log context_reset + go_drain_dispatch + a fresh agent draining the head.
- [ ] [#qcm1verify] [operator-verify] #agent-doc-bug Install + live-verify the #qnodemerge1 component-scoped merge (agent-doc submodule 52458f47). Code shipped+pushed, make check green (4853). Operator-gated remainder (mid-session install causes finalize-client-newer drift; STOP-cutting-releases gate): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), agent-doc admin recycle, then drive one edit-during-finalize cycle (type into a queue item while the agent writes an exchange response) and confirm ZERO cross-component splice + no console-output-in-queue corruption.
- [ ] [#1jec] [operator-verify] #yzerverify #agent-doc-bug Live-verify the #yzer/#evmhplugin plugin reconnect re-read (agent-doc 0.34.28 / submodule f6b8ae17, plugin 0.2.172). Code shipped+pushed: binary FFI agent_doc_reconnect_buffer_decision (disk==HEAD via ffi_show_head + buffer==prior-commit via ffi_show_prior_blobs) + pure decide_reconnect_buffer + PatchWatcher.registerRoot reconcile-on-connect (applyReconnectReread) + SimWorld coverage; make check green (4832). Operator-gated remainder (install is unsafe mid-session = finalize-client-newer drift): at a supervisor-recycle boundary run make install (cargo install --path . + cargo build --release + agent-doc lib-install), reinstall plugin 0.2.172, agent-doc admin recycle. Then repro: open a session doc in JB, kill/restart the supervisor so the binary commits while the plugin is disconnected, and on reconnect confirm ops.log shows 'reconnect_buffer_decision decision=reread_disk #yzer' and the editor buffer == HEAD with NO File Cache Conflict; a buffer holding genuine unsaved user edits must instead log decision=keep_buffer and be preserved (editor wins).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#ftqpartialdup] #agent-doc-bug Investigate partial free-text queue item duplication in equityfundingsource.md: pasted repro shows a full queue prompt duplicated as a truncated second line ending "... on that". Confirm whether the #hap7/#gf5n structural-node merge fixes cover same-node partial duplication; if not, add a same-node/free-text conflict guard and regression.
- [/] [#recyclerename] #agent-doc-bug Rename the JB + VS Code "Restart Supervisor Process" menu action to "Recycle Supervisor", and ensure a distinct "Restart" command sits alongside it (operator request attached to #s81q, struck when #s81q was gated but NOT implemented — this re-files it honestly). Restart Agent already exists as the harness-restart command; this is specifically renaming the supervisor-process action to "Recycle Supervisor" (matching the admin recycle semantics) + confirming Restart sits alongside. JB: plugin.xml text rename of AgentDoc.RestartSupervisorProcess (action IDs stay stable, only text changes per the convention) + VS Code command title. Confirm alongside-vs-replace with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#arrcodexproof] [recommended] #agent-doc-bug #agentreloadrestart Phase 1b follow-up: re-run the managed codex capability proof when a boundary restart switches TO codex. Today spawn_managed_capability_proof_thread runs once before the supervisor loop with the launch harness; build_harness_launch_spec re-resolves args/env/network on restart but capability_proof_required is discarded at the swap, so a mid-session switch to codex spawns fresh but skips the managed capability proof. Wire the proof (re)start into the harness-change restart path. make check + a focused check.
- [/] [#supinstallvis] [recommended] #agent-doc-bug #supinstallfeedback follow-up: the supervisor auto-install status (surface_supervisor_auto_install_status, idle_watch.rs) is surfaced ONLY as a transient tmux display-message on the status line, which is easy to miss while the pane scrollback keeps showing the misleading post-exit "Press Enter to restart" keepalive — so an operator watching the pane body still perceives a ~1-min stall. Residual: make the rebuild state visible in the pane BODY/scrollback (not just the status line) without disturbing the keepalive Enter-to-restart reader, e.g. a pane title set + a single non-input-consuming visible line, or render it through the JB/VSCode plugin notification surface (overlaps #qpauseux notification rendering). Decide the safe pane-write mechanism with the operator before implementing (send-keys risks being consumed by the keepalive). Plan TBD.
- [/] [#runexitrestart] route/idle-watch should detect a cleanly-exited harness pane (Press Enter to restart) with prompt-bearing work, restart the harness, wait for dispatch-ready, then submit, so a cleanly-exited harness never drops a Run Agent Doc. Two-pane live verify. NOTE the originally-reported silent loss was already fixed in 0.34.18.
- [/] [#docdriftgrace] OPERATOR-WATCH one live re-baseline cycle: binary should classify mid/between-turn drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing only structural conflicts on the in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [/] [#jbrundispatch] OPERATOR-CAPTURE live repro at stall (admin detect for cross-doc pane contamination + ops.log route trace + which pane state failed) to distinguish a real new route/submit bug from already-fixed behavior. Overlaps jbrunstart / startup-miss supersession / qdispatchloss; cannot be fixed blind in route.rs.
- [/] [#cyh0] OPERATOR-CAPTURE: the remaining File Cache Conflict has an unidentified trigger (hot-path disk writes are already listener-guarded). Capture which exact action raises the dialog (agent-doc cmd + ops.log line, e.g. write_authority action=... transport=disk). Likely: ack-content sidecar write, VCS-refresh/save-document signal, or an editor-IPC convergence the plugin surfaces as conflict.
- [/] [#8bfz] single live JB plugin consumer per document (per-doc owner lease; non-owners defer patch apply + saveDocument) so concurrent windows cannot race into File Cache Conflicts. Plugin PatchWatcher + binary lease primitive; operator multi-window live test.
- [/] [#qpauseux] OPERATOR-TEST (plugin render; needs JB plugin build): install updated plugin, verify a paused-queue block renders as an info/action notification with a Resume button, and the churn-stop/no-submit variant surfaces a restart-supervisor/resume action instead of silence (not a raw red Error toast).
- [/] [#jbdisprecycle] OPERATOR-TEST (code R1-R4 shipped submodule d21079b7): at a recycle boundary fire JB Run Agent Doc on a 2nd doc during the hot-reload burst; confirm a single dispatch_start_proof WITH submit and no stranded start_session failed. ops.log marker: route_dispatch_submit_recycle_settle.
- [/] [#editorbufwin] merge-core follow-up after P1 ship (submodule 9f842ba2): #pcwc now preserves raw user queue additions when replay normalization neutralizes the queue component. Remaining P2: auto-recover/commit the preserved queue addition instead of leaving detect-and-warn local drift/manual reset; add regression and live verify stale supervisor CRDT refresh path if still needed. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md
- [/] [#s81q] [recommended] #agent-doc-bug #stopvscodemenu Extend #restartagentmenu (JB Restart Agent; plan tasks/agent-doc/plan-restart-agent.md) per operator request to also add a Stop Agent command and mirror BOTH Stop+Restart into the VSCode extension menus. Delta beyond #restartagentmenu: (1) StopAgentAction.kt + plugin.xml registration on the JB side; (2) VSCode command-palette + editor/title menu contributions for stop+restart wired to the same TerminalUtil/route paths. Decide alongside-vs-replace the existing Restart Supervisor Process action; do not remove without operator confirmation. Bump pluginVersion + build signed/unsigned zips (gated by STOP-cutting-releases). Confirm scope before implementing.
- [/] [#restartagentmenu] restart-agent Phase 2 (after agentreloadrestart): RestartAgentAction.kt JB menu cmd mirroring RestartSupervisorProcessAction.kt + plugin.xml registration + TerminalUtil restart wiring; bump pluginVersion; build signed+unsigned. Log action invocation + resolved harness. Decide alongside-vs-replace Restart Supervisor Process with operator. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#agentreloadrestart] restart-agent Phase 1: supervisor restart re-resolves harness from current frontmatter (agent claude->opencode takes effect on restart) + boundary auto-restart when resolved harness differs, behind agent_doc_agent_change_restart (default ON). Log harness_change_detected old/new + agent_restart_boundary_gate + agent_restart_performed. SimWorld + make tmux-ci. Plan: tasks/agent-doc/plan-restart-agent.md
- [/] [#j9ja] [/] [blocked] #optverify SUCCESS ops.log markers so operator live tests auto-verify. SHIPPED this cycle: convergence_gate_converged_dispatch (agent-doc submodule 2367d883 — #fbwire Phase 2 success-after-defer in idle_watch.rs, emitted only when the gate had been deferring; auto-verifies #fbwireverify slow-ACK). Already exists: route_dispatch_submit_recycle_settle (route/dispatch.rs:831), auto_start_dispatch_ready_confirmed, queue_paused_continuation_guidance_emitted. BLOCKED (code sites do not exist yet): agent_restart_performed — add inside #agentreloadrestart Phase 1b restart EXECUTION (today only harness_change_detected + agent_restart_boundary_gate note=phase1b_execution_pending exist); restart-agent menu-invoke marker — add inside #restartagentmenu RestartAgentAction. Un-gate + close when those two parent features land and their markers are added.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
