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
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
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
- `cargo test --test finalize_integration finalize_stale_snapshot_blocks_pending_flags_before_mutating_backlog`
- `cargo test -p agent-doc-orchestration protected_prompt_draft_preview_redacts_and_bounds_latest_draft`
❯ - JetBrains `TerminalUtilTest.protected prompt input route refusal is actionable not persistent failure`
- `cargo test -p agent-doc --bin agent-doc route_sim_protected_prompt_refusal_has_no_dispatch_churn`
❯ - `make bump-plugin`
- `make check`
- `make install-full`
❯ - `src/agent-doc` `7585b872` (`fix: harden route and closeout diagnostics`)
❯ - `src/agent-doc` `612b1552` (`test: cover protected prompt route refusal`)
❯ - parent repo `c77917dc0` (`agent-doc: harden route closeout diagnostics`)
❯ - parent repo `e9626e632` (`agent-doc: add protected route sim coverage`)
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-181923.md`*

Compacted content:
❯ - Archived 3 response topic(s): editor state projection wiring; JetBrains Codex Enter submit path; equityfundingsource route difference
❯ - Prior summary/context: prior compacted content: Archived 8 response topic(s): bug handling policy; bug policy config; JB `Run Agent Doc` sti...
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=dc60c831-2fbb-4378-b738-1bc70433d1dc invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=12497 candidate_hash=e36e2b2735dbf032414ee881e91f628e4d6eaedecc021aa8b4e4a904b8ef1389 content_ours_len=14629 content_ours_hash=c7431d78df5e6739c4b3b72c5c292ffb0368eb5d933aece1835469351b06e7e5 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782324585135
```

### Re: do #ipcproofcloseout, #crdtreset1, and #routeblockux1 — gpt-5

Implemented and pushed the code fixes.

Submodule commits:
- `7585b872` `fix: harden route and closeout diagnostics`
- `612b1552` `test: cover protected prompt route refusal`

Parent pointer commits were also pushed, including `e9626e632`.

Verification passed:
- targeted Rust tests
- JetBrains `TerminalUtilTest`
- new SimWorld protected-prompt no-dispatch-churn test
- `make check`
- `make install-full`
- `./gradlew test --tests 'com.github.btakita.agentdoc.TerminalUtilTest.protected prompt input route refusal is actionable not persistent failure'`
- `cargo test -p agent-doc-orchestration protected_prompt_draft_preview_redacts_and_bounds_latest_draft`
- `cargo test --test finalize_integration finalize_stale_snapshot_blocks_pending_flags_before_mutating_backlog`
- `cargo test -p agent-doc --bin agent-doc route_sim_protected_prompt_refusal_has_no_dispatch_churn`
- `612b1552` `test: cover protected prompt route refusal`
- `7585b872` `fix: harden route and closeout diagnostics`

Agent-doc closeout had remained uncommitted because the editor IPC path was blocked by the route-owned supervisor. This headed recovery response preserves the completed implementation evidence and crosses the binary-owned closeout path.

### Re: stalled Run Agent Doc recovery — gpt-5

> **Queue prompt:** After the auto-recycle, agent-doc turn did not restart. JB `Run Agent Doc` stalled with no feedback. No prompt. Only an auto-prompt + an auto-clear both without a submit.

Recovered the stranded closeout first. The prior `response_captured` cycle is now committed, and the follow-up preflight opened a fresh cycle. During recovery, preflight auto-struck the already-resolved `do [#ipcproofcloseout]`, `do [#crdtreset1]`, and `do [#routeblockux1]` queue prompts from the done state.

I dry-ran the emitted orchestration command. It resolved only the already-recorded verification evidence from the previous response: targeted Rust tests, JetBrains `TerminalUtilTest`, SimWorld protected-prompt coverage, `make check`, and `make install-full`. I did not rerun those as new tasks because they were recovery residue, not a new task list.

Next concrete follow-up is now `#promptprefixorch1`: fix prompt-prefixed recovery/evidence lines being classified as user prompt targets and orchestration work after interrupted closeout recovery. Current actionable backlog still also has `#xgdn` and `#sz4a` for route/dispatch proof failures.

### Re: dropped recovery evidence prompts — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` on this document stalled. I recycled + restarted. Now JB `Run Agent Doc` is blocked due to another dispatch.
> ```
> resync: pruned 2 stale session(s)
> [route] target tmux session: 0
> Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md into pane %1 for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the composer contains protected prompt input (drafted prompt input); clear or submit that draft, then rerun agent-doc route snapshot_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1782325177477-direct_pane_protected_prompt_input-codex-_1-1d5c5a3ff075.txt
> ```


The prompt-prefixed command and commit lines are recovery evidence from the already-completed route/closeout work, not a new task list. I preserved them in `agent:exchange`, answered them as prior execution evidence, and did not rerun the dry-run orchestration tasks.

This closeout is specifically for `#exchange-prompt-dropped-on-merge`: commit the visible exchange evidence through the binary-owned path so session-check no longer sees those lines as dropped user prompts.

### Re: exchange-prompt-dropped-on-merge closeout — gpt-5

Confirmed this was recovery residue, not a new implementation task. The visible exchange evidence is now committed, and the emitted orchestration dry-run still resolves only already-run verification evidence and prior commit IDs.

No code files changed in this follow-up. The protected-prompt `Run Agent Doc` queue item is fenced done, and the next actionable queue head is `#sz4a`. There are no new actionable follow-up items beyond the existing `#promptprefixorch1` backlog item.

Closed the cycle for `tasks/agent-doc/agent-doc-bugs2.md`.

Verified the protected-prompt route refusal was already fixed by `src/agent-doc` commits `7585b872` and `612b1552`; no code changes were needed. Targeted checks passed, including the Rust route/IPC regressions and the JetBrains `TerminalUtilTest` case.

`agent-doc session-check` is now OK for cycle `cycle-1782328264470`, and queue continuation is currently `false`, so I did not drain the next pinned queue item. The latest session-doc commit is `da21bf244`. Note: the session document still shows local editor/queue drift in the worktree; I left it uncommitted because the authoritative `session-check` closeout is green.

### Re: #sz4a — gpt-5

> **Queue prompt:**
>
> :pushpin: [#sz4a]


`#sz4a` is closed as already covered by the current route/dispatch implementation, with verification.

The saved pane diagnostic at `.agent-doc/logs/route-submit/1782325723814-direct_pane_dispatch_start_unproven-codex-_3-c75435aee855.txt` shows a Codex pane that stayed at idle chrome with a lone `start` draft after the routed submit attempt. That matches the filed failure class: the pane-input boundary accepted keystrokes, but no routed dispatch-start proof was observed within the bounded proof window.

Current behavior covers this shape:
- Direct-pane routed dispatch retries Enter at least once per second while the trigger remains visibly drafted, then records `accepted_without_dispatch_start_proof` with a saved pane snapshot and a durable `#jbrunautobug #agent-doc-bug` backlog item when proof never appears.
- Dispatch-only Codex reroutes with hook tracking require actual dispatch-start proof; a tracked timeout fails closed instead of reporting route success.
- Managed fresh-start routes may carry `DispatchStartUnproven` into the stronger document-cycle ack check, so a late real start can still prove itself, but a no-start pane is not treated as complete.

Verification passed:
- `cargo test -p agent-doc-orchestration accepted_only_dispatch_start_proof_fails_only_when_required`
- `cargo test -p agent-doc-orchestration dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed`
- `cargo test -p agent-doc-orchestration dispatch_only_codex_requires_start_proof_when_hooks_are_visible`
- `cargo test -p agent-doc-orchestration route_dispatch_bug_report_item_includes_required_evidence`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_is_bounded_while_trigger_remains_visible`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_retries_at_least_once_per_second`
- `cargo test -p agent-doc-orchestration route_submit_observation_marks_accepted_without_dispatch_proof_as_issue`
- `cargo test -p agent-doc-orchestration dispatch_only_direct_pane_accepted_submit_skips_optional_proof_wait`

No code files changed in this follow-up; I marked `#sz4a` done and left the separate `#xgdn` dispatch-only proof item open.

### Re: JB `Run Agent Doc` on monsterrodholders.md did not submit. Please diagnose and fix. — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` on monsterrodholders.md did not submit. Please diagnose and fix.

I diagnosed this as an already-covered historical route failure, not a new unhandled code path in the current tree.

The Monster Rod Holders evidence I found falls into the stale-supervisor and no-submit route classes that are already fixed:
- The older pane capture explicitly says the `monsterrodholders.md` supervisor was running pre-update code and from the wrong project root.
- The later note says `0.34.20` was installed, `agent-doc admin recycle` was requested, and the operator was asked to retry because the cold-start gate was now live.
- Current route-submit artifacts do not show a fresh dedicated `monsterrodholders.md` submit failure. The latest Monster Rod Holders hits are scrollback inside other pane captures or successful `agent-doc(monsterrodholders)` commits, not a new failing route attempt.

The current code covers the reported no-submit shapes:
- stale-binary self-recycle / recycle-yield for supervisors that keep draining while running old code;
- cold-start and restart dispatch-ready gates before injecting `agent-doc <FILE>`;
- redispatch when the routed trigger never lands in the composer;
- one-second Enter retry while a drafted trigger remains visible;
- durable route-dispatch bug filing for future bounded submit/proof failures.

Verification passed:
- `cargo test -p agent-doc-orchestration pane_idle_dispatch_ready_distinguishes_non_dispatch_from_fast_submit`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_is_bounded_while_trigger_remains_visible`
- `cargo test -p agent-doc-orchestration direct_pane_enter_resubmit_retries_at_least_once_per_second`
- `cargo test -p agent-doc route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send`
- `cargo test -p agent-doc route_sim_restart_drain_waits_for_dispatch_ready_prompt_before_send`

No code files changed for this queue item. The actionable fix is already in the installed route/dispatch code, and future recurrence should file a stage-specific `#jbrunautobug` item with a diagnostic path instead of leaving the report as an unstructured free-text queue head.

### Re: exchange merge drift recovery — gpt-5

> No code files changed in this follow-up; I marked `#sz4a` done and left the separate `#xgdn` dispatch-only proof item open.

That line is assistant closeout evidence from the completed `#sz4a` response, not a new user prompt. The same applies to the nearby reordered verification bullets and Monster Rod Holders diagnosis fragments: they came from the already-committed assistant responses for `#sz4a` and the Monster Rod Holders queue head, then the live editor merge moved them around in `agent:exchange`.

Recovery action for this cycle: classify those fragments as prior assistant evidence, not new work. I did not run additional tests or change code in this recovery-only closeout.
<!-- agent:boundary:66f141b4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: [#sz4a]~~
- ~~:pushpin: JB `Run Agent Doc` on monsterrodholders.md did not submit. Please diagnose and fix.~~
- :pushpin: What are #next-steps to fix agent-doc bugs?
- :pushpin: Ensure the VS Code plugin has parity with the JB plugin.
- :pushpin: For tasks added from dogfooding issues, please add a configuration to select the agent document to write the backlog items to. The default is the current document. For agent-loop configure the document to this document.
- :pushpin: Annotate in real-time the current queue item, backlog item, icebox item with 🚧. 🚧 should be prefixed for queue items, and after the `[ ]` or `[/]` or `[x]` for backlog + icebox items. I want to see what is currently in progress during the turn.
- :pushpin: do [#promptprefixorch1]
- do [#xgdn]
<!-- /agent:queue -->
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
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
- [ ] [#promptprefixorch1] Fix prompt-prefixed recovery/evidence lines in agent:exchange being classified as user prompt targets and orchestration tasks after interrupted closeout recovery; preserve them as agent evidence or strip prompt markers during recovery.
- [ ] [#xgdn] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%3 actor_generation=596 editor_attempt_id=1782325712508-18 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
