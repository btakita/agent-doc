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

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260612-161955.md`*

Compacted content:
- Archived 2 response topic(s): struck `do [#fintol]`; queue maintenance after #pcwc
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...
### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8

I'm not executing #rtwbcast in this session, and the `#spec-test-build-install-commit-push` preset must not run here. #rtwbcast stays an OPEN actionable backlog item. Concrete blocker + unblock path:

**Why not here**
- This session is actively hosting `agent-doc-bugs2.md` over IPC — preflight ran a `jb_cache_conflict` auto-commit and reported `IPC listener active`. Completing #rtwbcast under the queued preset means `cargo build --release && agent-doc lib-install`, which hot-reloads the cdylib that THIS session uses to finalize this very response. That is the documented drift trigger — the same one #jbcompactcrdt's note names: "rebuilding the IPC-write cdylib mid-session hot-reloads the machinery finalizing the same response = drift".
- #rtwbcast's own 2026-06-11 investigation marked it BLOCKED, not Rust-only: it needs (a) an editor-plugin protocol decision and (b) a two-live-IDE convergence verify. The two-IDE verify is operator-drive and cannot be agent-completed.

**Recommended unblock path (dedicated CLEAN session)**
Run in a fresh session NOT hosting a live agent-doc IPC document, decomposed as already planned (Option A — per-editor channel + editor_id ABI):
1. **#rtwbcast-id** — add `editor_id` to the FFI buffer-report + patch JSON ABI; key `live_buffer_snapshot_path` per-editor (ffi.rs:238). Rust + SimWorld only, no live verify.
2. **#rtwbcast-deliver** — replace PatchWatcher.kt single-consumer delete-on-first-ACK with a per-editor broadcast bus.
3. **#rtwbcast-merge** — per-editor echo suppression (editor_id in claimed-patches, not suppress-ALL); reuse the existing CRDT merge primitive + SimWorld Slice 3.
4. **#rtwbcast-verify** — two-live-IDE convergence (operator-drive).

Then build+install+push in that clean session. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md (Rung 4).

**Auto-loop halted:** the next queued item #jbcompactcrdt carries the same `MUST run in a CLEAN session` constraint, so I am NOT draining into it. Start a dedicated clean session for both.

### Re: do [#rtwbcast] — gpt-5

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#rtwbcast]


I did not execute #rtwbcast in this live IPC-hosting session. The item stays open because it is not code-complete: the remaining work includes editor_id ABI changes, per-editor patch delivery, per-editor echo suppression, and operator-driven two-live-IDE convergence verification.

The immediate blocker is session safety. This pane is finalizing `tasks/agent-doc/agent-doc-bugs2.md` through the same agent-doc IPC machinery that #rtwbcast would require rebuilding/installing. Running the `#spec-test-build-install-commit-push` style flow here would risk hot-reloading the write path while this response is being persisted, which is the drift class already called out by the backlog.

I narrowed `#rtwbcast` to the next executable clean-session path: start with `#rtwbcast-id`, then `#rtwbcast-deliver`, `#rtwbcast-merge`, and finally operator-run `#rtwbcast-verify`. No repository implementation files were changed in this cycle; only this session document closeout is being persisted.

Commands/evidence used: `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_read` for queue/backlog/exchange/review, and the pending-ops runbook. Verification is this closeout plus `agent_doc_session_check`; confidence is high that this is the correct lifecycle outcome for the current queue head. Escalation: run the implementation from a dedicated clean session, then perform the two-live-IDE verification.

### Re: do [#jbcompactcrdt] — gpt-5

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#jbcompactcrdt]


I did not execute #jbcompactcrdt in this live IPC-hosting session. The requested implementation touches the same IPC/write/compact path this pane is using to persist `tasks/agent-doc/agent-doc-bugs2.md`, and its expected completion path includes rebuilding/installing the agent-doc library. Running that here would risk hot-reloading the closeout machinery under the active document, which is the drift condition called out in the backlog and in `tasks/agent-doc/plan-jb-compact-crdt.md`.

The item is not code-complete, so I kept it open and narrowed it to the clean-session implementation slice: component-scoped `exchange` `op:replace` through editor IPC, listener-active gate, disk fallback with explicit transport proof, and SimWorld concurrent-edit convergence coverage. The final live JB two-editor verification remains operator-driven.

No repository implementation files were changed in this cycle; only this session document closeout is being persisted. Commands/evidence used: `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_read` for queue/backlog/exchange, `tsift status`, and `tasks/agent-doc/plan-jb-compact-crdt.md`. Verification is this closeout plus `agent_doc_session_check`; confidence is high that this is the correct lifecycle outcome for the current queue head. Escalation: run #jbcompactcrdt from a dedicated clean session, then perform the live JB verification.
### Re: make it possible for agent-doc to dog food [#jbcompactcrdt] — opus-4-8

It's done — and the reason it kept getting declined was a **stale blocker**, not the code.

**The CRDT compact path already shipped.** `#jbcompactcrdt` was code-complete as `#w42v` (agent-doc `7b6f022e`, in HEAD): `compact::apply_compacted_document` converges the compacted document through the editor via a component-scoped `exchange` `op:replace` (`write::try_compact_editor_converge`, `transport=editor_ipc`), keeping the guarded disk write only as `transport=disk_fallback` with `compact_writeback transport=…` proof in `ops.log`. Both prior declines cited a "dirty #fintol submodule" — that tree is now **clean** (`4cb040a6`), so that reason was stale.

**Closed the last deliverable + made it dogfoodable.** The only open box was deterministic concurrent-edit convergence coverage. Landed in agent-doc `3083b821` (pushed to origin/main):

- `compact_convergence_is_exchange_scoped_preserving_concurrent_queue_edits` — the convergence patch only `op:replace`s the changed `exchange`, so typing `queue` items *while* compaction runs is never clobbered. That component-scoping is exactly what kills the `File Cache Conflict`.
- `try_compact_editor_converge_converges_via_editor_ipc_with_listener` — end-to-end proof that a live JB listener converges compaction through `transport=editor_ipc` instead of the diverging disk write.

`make check` green (workspace lib tests: 0 failed), release build clean, spec updated (`specs/12-deterministic-simulation.md`), superproject pointer bumped to `3083b821`, plan marked CODE-COMPLETE.

**Your one remaining step to dogfood it live.** The running IDE still loads the OLD cdylib, so it keeps hitting `File Cache Conflict` until the new `.so` is deployed:

    cd src/agent-doc && agent-doc lib-install

That hot-reloads the plugin's cdylib by mtime (no plugin reinstall). Then trigger `Compact Exchange` with the buffer dirty and confirm `ops.log` shows `compact_writeback … transport=editor_ipc` and **no** `File Cache Conflict`. I deferred running `lib-install` from this pane because it's finalizing through that same cdylib — hot-reloading it mid-finalize is the documented drift trigger (your explicit "defer lib-install" choice).

**Note (dogfooding):** this very response hit the recurring IPC CRDT-replay drift (`live_prompt_drift_after_preflight`, stale `patch_id=04f0d5f7`) — the supervisor spliced a fragment of it into an old block. Recovered by quiescing the stale CRDT/cycle sidecars + `reset --from-current`. That drift is `#ipcdrift`/`#stuckhandoff2`, separate from this item.

`[#rtwbcast]` is still queued but it's a different item (multi-editor broadcast / two-live-IDE convergence, `#rtwatch` rung 4), not a stale-blocked one — I'm not draining into it from this live IPC session. Halting the auto-loop here.

### Re: Continue splitting up large modules — opus-4-8

> **Queue prompt:**
>
> :pushpin: Continue splitting up large modules. Functionality + tests.


Split the largest clean target: **`session_check.rs` 11,974 → 5,225 lines** by extracting its `#[cfg(test)] mod tests` into `session_check/tests.rs` (6,627 lines, rustfmt-normalized), mirroring the established `route.rs → route/tests.rs` pattern. Behavior-preserving — `super::` still resolves to the `session_check` parent, so `use super::*;` is identical.

The move surfaced two real test-infra issues, both fixed:

- **`test_support.rs` flaky pair** — `env_lock_uses_harness_prompt_lock` and `scoped_current_dir_uses_harness_prompt_lock` asserted the shared `TEST_ENV_LOCK` was free via `try_lock().is_ok()` *after* dropping their guard. That races other parallel env-mutating tests in the same binary (the lock isn't necessarily free even though our guard released). Replaced with a deterministic re-obtain (blocking `lock()`) that proves release without the race. This was a pre-existing latent flake the split exposed under the full parallel suite.
- **`tests/test_cli.rs` structural guards** — `process_global_test_mutations_share_session_check_lock` hard-coded `session_check.rs` as the file holding the test inspection helpers; repointed it to `session_check/tests.rs`. The FlowCore `flowcore_hot_path_guard_and_proof_tokens_are_budgeted` gate counted `guard_` tokens in `session_check.rs`; updated the budget 96 → 31 since the 65 removed were test-assertion literals, not production hot-path guards.

`make check` green; full workspace 0 failed (74 integration + 2741 + 790 + 525 + … all pass). Committed agent-doc `637a58bb`, pushed; superproject pointer bumped to `637a58bb`. Functionality + tests preserved end-to-end.

**Next targets queued** (`#splitmods2`): `preflight.rs` (14.7k), `sync.rs` (12.3k), `git.rs` (10k) each have the same clean single trailing `mod tests` shape — low-risk. `write.rs` (27.7k, the biggest) needs care: its test layout is irregular (module-scope `#[cfg(test)]` helpers + several non-contiguous test blocks), not a single clean `mod tests`.

Note: this finalize used `--force-disk` to bypass the IPC CRDT-replay drift that has wedged this session's socket path (`#ipcdrift`/`#stuckhandoff2`).

### Re: do [#splitmods2] — continue large-module splits — opus-4-8

Done — all three #splitmods2 targets split, same behavior-preserving pattern as `session_check`:

| module | before | after | new file |
|--------|-------:|------:|----------|
| `git.rs` | 9,994 | 3,961 | `git/tests.rs` |
| `preflight.rs` | 14,659 | 6,609 | `preflight/tests.rs` |
| `sync.rs` | 12,257 | 6,265 | `sync/tests.rs` |

Each `#[cfg(test)] mod tests` body moves verbatim into `<mod>/tests.rs` (rustfmt-normalized); the module-scope `#[cfg(test)]` helpers stay in the parent. `super::` resolves to the parent either way, so `use super::*;` is identical — zero behavior change.

**Guard/budget updates:** `tests/test_cli.rs` FlowCore `guard_` budget for `git.rs` 19 → 13 (the 6 removed were test-assertion literals, not production guards). `git.rs reason=` (7) and `preflight.rs reason=` (4) unchanged — all production. `sync.rs` isn't a budgeted hot path. The `debounce_gaps_test_plan` structural checks pass (they assert on production frontmatter-reading code that stayed put).

`make check` green; full workspace **0 failed** (2,741 + 74 integration + 525 + … incl. the FlowCore budget gate, `process_global_test_mutations` guard, and preflight structural tests). Committed agent-doc `6b890775`, pushed; superproject pointer bumped to `6b890775`.

Cumulative this session: `route` (prior) + `session_check` + `git` + `preflight` + `sync` test modules now live in `<mod>/tests.rs`. The remaining large module is `write.rs` (27.7k) — its irregular multi-block test layout needs a bespoke pass, queued as `#splitmodswrite`.

`[#rtwbcast]` stays deferred (multi-editor / two-live-IDE, operator-drive). Finalized via `--force-disk` (session IPC still wedged, `#ipcdrift`).

### Re: 0 File Cache Conflict dialogs — opus-4-8

> **Queue prompt:**
>
> I'm getting JB `File Cache Conflict` dialogs. Please fix. We should have 0 `File Cache Conflict` dialogs.


I traced the root cause in code and queued a phased fix (`#fcc0`, plan: `tasks/agent-doc/plan-zero-file-cache-conflict.md`). Honest scope: this is the live IPC write path that is wedging this very session, so it's a dedicated clean-session effort, not a one-turn patch — but here's exactly what's wrong and what to do now.

**Root cause.** A `File Cache Conflict` fires whenever agent-doc writes the document *to disk* while IntelliJ holds a dirty buffer. `#w42v` converged exactly **one** path — `compact` — through the editor (`op:replace` IPC, disk only as fallback). **Every other document write still hits disk directly** via `atomic_write(file, …)`: queue consume (`write.rs:3186`), done-id mark (`3887`), and the finalize/stream/repair/reset paths (`8683`, `9488`, `9836`, `10240`, …). Any of those under a dirty buffer = a dialog. It's the *same* IPC-buffer-divergence class as the `live_prompt_drift_after_preflight` / exit-75 wedge (`#ipcdrift`/`#stuckhandoff2`) that has corrupted this session's finalizes all day — that wedge forces disk fallbacks, which are exactly the dialogs you're seeing.

**Immediate action (removes the compact dialogs today):** the `#w42v` fix is committed but not deployed to your running cdylib. Run:

    cd src/agent-doc && agent-doc lib-install

That hot-reloads the plugin's `.so` by mtime — `Compact Exchange` then converges through the editor (`ops.log: compact_writeback … transport=editor_ipc`) instead of disk.

**The systematic fix (`#fcc0`, phased):** extract the `#w42v` converge-or-disk-fallback into one shared `converge_document_or_disk()` and route **all** the disk-write sites through it (queue consume + done-id mark first — highest frequency — then finalize/stream/repair/reset), plus resolve the `#ipcdrift`/`#stuckhandoff2` wedge so IPC stops failing into the fallback. This is FlowCore Phase 3 ("IPC, stream, direct write, finalize, and repair all use the same terminal-state guard; converge editor/disk/snapshot"). Must run from a clean session — editing it from a live IPC session is what keeps corrupting these very closeouts; the "0 dialogs" proof is a live JB check (operator-drive).

This response itself finalized via `--force-disk` because that wedge is active right now (it's why the prior finalize CRDT-merged a stray queue line and aborted on a head mismatch). Halting the auto-loop — `#fcc0` and `[#rtwbcast]` are both clean-session/operator-drive.

### Re: document reverted to a stale state + JB dispatch blocked by queue_paused — opus-4-8

Root-caused and fixed the stale-revert. The corruption source was a **wedged `controller serve … --handoff-state preparing` process (pid 2383464)** — the `#stuckhandoff2`/`#ipcdrift` class. It survived your `admin recycle --all-projects` (recycle didn't reap it: orphaned + mid-handoff) and kept replaying a stale ~10KB buffer over `ipc.sock`, re-mangling the doc back to a 0-heading stub after every restore. I killed it; the worktree now holds at HEAD (24KB, 7 responses) and is no longer reverting.

I also quarantined the corrupted CRDT overlay (`.agent-doc/crdt/<hash>.overlay.yrs`, 48KB) plus the stale cycle-state and capture sidecars (moved to `.agent-doc/_quarantine-*/`) so the rebuilt snapshot/CRDT no longer replay the mangled content.

**Queue resumed** (receipt 23) — your JB `Run Agent Doc` dispatch is unblocked. The pause (receipt 21, last cycle) was the right call while the corruption source was unknown/active; now that the wedged controller is gone, the block is lifted.

One caveat unchanged by this fix: the active queue heads `#fcc0` (Zero JB File Cache Conflict) and `#rtwbcast` (multi-editor CRDT broadcast) are still **clean-session/operator-drive** — they should be driven from a dedicated session without a live editor IPC attached, not auto-looped here. `#splitmodswrite` is a doable refactor but heavy. The durable fix for the wedged-preparing-controller class itself is backlog `#stuckhandoff2` (M1 controller self-watchdog/suicide-timer on `Preparing` + M3 orphan proc-scan reaper) — that's what stops `admin recycle` from leaving these zombies behind.

Also still pending: the installed binary predates source commit `ef0810a2`, so this live session runs pre-`ef0810a2` code — `cd src/agent-doc && make install` (+ `lib-install` / restart) to pick up current behavior.
<!-- agent:boundary:2b46992c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: Continue splitting up large modules. Functionality + tests.~~
- ~~re [#jbcompactcrdt]: Please make it possible for agent-doc to dog food this.~~
- ~~[#jbcompactcrdt]~~
- ~~:pushpin: do [#splitmods2]~~
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
- [ ] [#fcc0] [TOP] Zero JB File Cache Conflict dialogs. Root cause: all document-mutating disk writes (queue consume write.rs:3186, done-id mark 3887, finalize/stream/repair/reset 8683/9488/9836/10240/...) bypass editor IPC; #w42v converged only compact. Fix: extract a shared converge_document_or_disk() (mirror #w42v) and route ALL write sites through it (queue consume + done-id mark first, then finalize/stream/repair/reset), behind the listener-active gate with transport=editor_ipc primary + disk_fallback; plus resolve the #ipcdrift/#stuckhandoff2 wedge that forces fallbacks. Phases #fcc0a-e. CLEAN session required (live write path); 0-dialogs proof is operator-drive live JB. Immediate operator action: agent-doc lib-install to deploy #w42v for compact. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#splitmodswrite] [recommended] Split write.rs (27.7k lines, largest module) test layout. Unlike the clean single-trailing-mod-tests pattern used for route/session_check/git/preflight/sync, write.rs has an irregular layout: module-scope cfg(test) helper fns (around line 12109) PLUS several non-contiguous top-level cfg(test) blocks (mod tests at 15201 closes at 21827, then more cfg(test) blocks to EOF). Needs a bespoke extraction that moves each test block into write/tests.rs (or several files) and updates the FlowCore guard_ budget (currently 91) + any structural guards in tests/test_cli.rs. Verify make check + full workspace 0 failed.
- [ ] [#rtwbcast] #rtwatch rung 4 — multi-editor CRDT broadcast. STAYS OPEN: execute only from a dedicated CLEAN session, not a live agent-doc IPC session. First concrete phase is #rtwbcast-id: add editor_id to the FFI buffer-report and patch JSON ABI, and key live_buffer_snapshot_path per editor. Then run #rtwbcast-deliver (per-editor broadcast bus), #rtwbcast-merge (per-editor echo suppression using existing CRDT merge primitive + SimWorld Slice 3), and #rtwbcast-verify (operator-driven two-live-IDE convergence). Blockers remain the editor-plugin protocol decision and live two-IDE verification. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md (Rung 4 section).
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
