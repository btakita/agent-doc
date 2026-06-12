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

#pcwc done; code, tests, build, local binary/library install complete; JetBrains plugin artifact 0.2.161 built, manual IDE-profile install still needed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260612-144033.md`*

Compacted content:
❯ - Archived 25 response topic(s): do [#swint]; Root cause of "stuck-handoff" + how the binary mitigates it; Do we still need separate snapshot + baseline files?; 22 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
- The local JetBrains plugin install command could not complete because multiple IDE profiles were detected and the command has no non-interactive selector. Install the built plugin zip into the active IntelliJ IDEA profile: `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.161.zip`.
- Retried `make install` with escalation after the crates.io HTTP/2 failure; it failed the same way, so I used offline Cargo install from the already-verified local cache.
- parent repo: `62c8687f agent-doc(#pcwc): update submodule pointer`
- `src/agent-doc`: `b679a932 agent-doc(#pcwc): JB plugin refresh_content handler + stale-buffer guard`
- `src/agent-doc`: `3cb4265a agent-doc(#pcwc): push committed content to editor after worktree repair`
- `agent-doc lib-install` installed `libagent_doc-0.34.0.so`.
- `cargo build --release --lib --offline` passed.
- `cargo install --path . --offline` passed after online `make install` failed on crates.io HTTP/2 registry errors.
- `./gradlew test buildPlugin` passed and built `agent-doc-jetbrains-0.2.161.zip`.
- `make check` passed via `tsift digest-runner`.
- `./gradlew test --tests com.github.btakita.agentdoc.RefreshContentPreconditionTest` passed.
- `cargo test --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration postcommit_worktree -- --nocapture` passed.
- In the submodule: `agent-doc-orchestration/src/git.rs`, `agent-doc-orchestration/src/ipc_socket.rs`, `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`, `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/RefreshContentPreconditionTest.kt`, `editors/jetbrains/gradle.properties`, `SPEC.md`, `VERSIONS.md`, and the hot-path budget in `tests/test_cli.rs`.
- `src/agent-doc` submodule now points at `b679a932` on `origin/main`.

### Re: struck `do [#fintol]` — opus-4-8

Confirmed — `#fintol` is genuinely complete, so striking it is correct. The finalize forward-merge tolerance landed in the agent-doc submodule at `849ad9d9` (`agent-doc(#fintol2/#fintol3): land finalize forward-merge tolerance + green re-baseline`); current submodule HEAD is `9d1235cd`. No further work needed for `#fintol`.

Queue state after this strike: the active head is now `#pcwc` (post-commit IPC worktree corruption — note `#pcwc` already has groundwork at `8aea7e3d agent-doc(#pcwc): auto-reconcile post-commit worktree corruption to HEAD`), followed by `#rtwbcast` and `#jbcompactcrdt`, then two free-text questions.

One caution before draining further: this session is heavily bloated (largest turn ~477k tokens, ~97% cached resend, restart churn, ~18k no-op closeouts per next-context). Both `#pcwc` and `#rtwbcast` carry known blockers (carry-forward invariant; editor-plugin protocol decision + two-live-IDE verify) that won't close cleanly in one auto-loop cycle. Recommend `agent-doc compact tasks/agent-doc/agent-doc-bugs2.md --commit` (or a fresh `agent-doc start`) before taking on the remaining implementation items.
<!-- agent:boundary:65e6feba -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" -->
- ~~:pushpin: pushpin: JB `Compact Exchange` should be CRDT driven...I'm getting a `File Cache Conflict`.~~
- ~~:pushpin: As the queue runs, I'm getting `agent-doc tasks/agent-doc/agent-doc-bugs2.md` prompts submitted into the running Claude Code session. I have to manually delete the prompt from the Claude Code session queue.~~
- ~~:round_pushpin: do [#snbc2]~~
- ~~:round_pushpin: do [#fintol]~~
- do [#pcwc]
- do [#rtwbcast]
- do [#jbcompactcrdt]
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
- [ ] [#jbcompactcrdt] JB Compact Exchange CRDT-driven (resolves File Cache Conflict). Direction CONFIRMED by operator. Re-implement+re-enable the disabled full-content editor-IPC path (try_ipc_full_content_with_mode, write.rs:13326, currently Ok(false) reason=disabled_by_default) as a component-scoped exchange op:replace via editor IPC (mirror #q7jm), replacing the direct disk write at compact.rs:332 (compact_exchange_direct_write). Keep disk write as transport=disk_fallback, gate behind listener-active, emit proof compact_writeback transport=editor_ipc|disk_fallback, add SimWorld coverage for the concurrent-edit convergence. MUST run in a CLEAN session (rebuilding the IPC-write cdylib mid-session hot-reloads the machinery finalizing the same response = drift). Live JB two-editor verify is operator-drive (ops.log transport=editor_ipc + no File Cache Conflict). Plan: tasks/agent-doc/plan-jb-compact-crdt.md
- [ ] [#rtwbcast] #rtwatch rung 4 — multi-editor CRDT broadcast. INVESTIGATED 2026-06-11 (clean context): NOT Rust-only — blocked on an editor-plugin protocol decision + two-live-IDE verify. Three gaps: (1) buffer ingest is single-sidecar last-writer-wins, no per-editor id (ffi.rs:238/debounce live_buffer_snapshot_path keys on path only); (2) patch delivery is single-consumer delete-on-first-ACK (PatchWatcher.kt deletes <hash>.json on first apply), not a broadcast bus; (3) no per-editor echo suppression (claimed-patches is suppress-ALL; no editor_id in FFI buffer-report or patch JSON). Recommended path: Option A (per-editor channel + editor_id ABI), decomposed into #rtwbcast-id / #rtwbcast-deliver / #rtwbcast-merge / #rtwbcast-verify. CRDT merge primitive + SimWorld Slice 3 already exist. Must run in a CLEAN session; verify needs two live IDEs. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md (Rung 4 section).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [/] [#efscodexsubmit] JB Run Agent Doc in equityfundingsource.md enters but does not submit the prompt to Codex (operator must press Enter). Same class as #2v7b/#jbcodexsubmit: Codex direct-pane dispatch (route.rs send_command_unchecked) detects prompt_not_submitted (merged text+CR not treated as submit by the Codex TUI) but has no Enter re-submit fallback. FIX: Codex-scoped one-shot Enter re-submit + route_submit_resubmit proof logging + SimWorld coverage (operator-requested). CLEAN session required (touches route.rs; submodule carries dirty #fintol tree). Plan: tasks/agent-doc/plan-jb-codex-submit.md
- [/] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit JB `Run Agent Doc` on Codex enters the routed prompt but does not submit it (operator must press Enter manually). Root cause: Codex direct-pane dispatch in route.rs::send_command_unchecked detects prompt_not_submitted (TriggerStillVisible) but has no re-submit fallback; the merged text+`\r` send-keys payload is not treated as submit by the Codex TUI. Fix: Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging + SimWorld coverage. Plan: tasks/agent-doc/plan-jb-codex-submit.md. Live verify is operator-drive (check ops.log for route_submit_resubmit action=enter_key result=accepted).
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
