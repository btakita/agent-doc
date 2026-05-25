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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Fixed agent:queue stuck-after-1-turn — auto-strike done items at preflight.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260525-084514.md`*

Compacted content:
- Archived 3 response topic(s): do [#adoc-compact-during-turn-response-loss]; response patchback loss after JB Compact Exchange mid-turn; do [#jbccc3]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: do [#jbccc4] — opus-4-7

Phase 4 of `#adoc-jb-cache-cancel-stuck-cycle` landed as a focused runbook plus the catalog wiring.

#### What changed

- **New `src/agent-doc/runbooks/jb-cache-conflict.md`** — documents the JetBrains File Cache Conflict dialog Accept/Cancel contract, the auto-recovery shape preflight matches (cycle at `WriteApplied`/`Committed`, snapshot ≠ HEAD, working tree matches snapshot modulo transient markers), the `jb_cache_conflict_cancel_auto_recovery_*` audit events, and the conditions under which auto-recovery declines (divergent working tree, wrong phase, commit error) with the manual `agent-doc write --commit` / `agent-doc commit` fallback chain.
- **`src/agent-doc/runbooks/commit.md`** — added a "JetBrains File Cache Conflict" section that points at the new runbook so the closeout-ordering doc stays the single entry point and the JB-specific detail lives in its own focused file (per the "oversized specs should split behind a stable index" project rule).
- **`src/agent-doc/SKILL.md`** and **`.claude/skills/agent-doc/SKILL.md`** — `jb-cache-conflict` added to the runbook full-catalog list so the harness skill surfaces stay aligned.

#### Verification

- File-by-file inspection of the new runbook + cross-links.
- No Rust source changed this turn, so no `cargo test` / `make check` was rerun; Phase 3's test coverage still holds.

#### Closeout

- Submodule commit: `9eb99ff` — `docs: add jb-cache-conflict runbook for #jbccc4`.
- Superproject commit: `f2ec1d760` — `agent-doc: bump submodule for #jbccc4 docs`.

Marking `#jbccc4` done. Next queue head: `do [#jbccc5]` (Phase 5 — SimWorld inversion + real-file integration coverage).
<!-- agent:boundary:b746731e -->
<!-- /agent:exchange -->
###
<!--
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.
#spec-test-build-install-commit-push
---
JB `Restart Supervisor Process` error:
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to inspect the pane.
```
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#crdtrespfx]~
- ~do [#queueadvgated]~
- ~do [#jbrsrbusysim]~
- ~do [#cclooploop]~
- ~do [#ipcdupfin23]~
- ~do [#ccloopguard]~
- ~do [#p6skillinstallreconcile]~
- ~do [#agent-doc-commit-sibling]~
- ~do [#adcs-binary]~
- ~do [#adoc-jb-cache-cancel-stuck-cycle]~
- ~do [#jbccc1]~
- ~do [#jbccc2]~
- ~do [#adoc-compact-during-turn-response-loss]~
- ~do [#jbccc3]~
- ~do [#jbccc4]~
- do [#jbccc5]
- do [#adoc-baseline-drift-after-user-commit]
- do [#adoc-prefix-strip-uncommitted]
- do [#jbrsrbusysw]
- do [#jbrsrbusyide]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
- do [#planperphase]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#jbccc5] Phase 5 of #adoc-jb-cache-cancel-stuck-cycle: regression coverage. tests/run_integration.rs (or sibling) — integration test that simulates cancel-branch file shape and asserts agent-doc preflight auto-recovers without manual write --commit; SimWorld scenario from Phase 1 must pass after Phase 3 lands. Plan: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md
- [ ] [#adoc-baseline-drift-after-user-commit] agent-doc-bug: when a user commits a manual edit on top of an agent-doc commit, the captured baseline (.agent-doc/baselines/<hash>.md) is not refreshed; preflight, write --commit, and repair all fail with 'captured response baseline no longer matches current document' and there is no clear recovery path the next time the user invokes /agent-doc on the doc. Workaround: agent-doc reset --from-current rebuilds the baseline from the live markdown but drops session ID + snapshot history. Plan path: tasks/agent-doc/plan-baseline-drift-after-user-commit.md
- [ ] [#adoc-prefix-strip-uncommitted] agent-doc-bug: after the JB-cache-conflict bug forces a manual sed strip of incorrectly-applied prompt-prefix markers (❯ ) from the response body, the subsequent agent-doc write --commit / repair / session-check chain fails to recognize the cleaned response as the cycle's committed response. Symptoms: session-check INTERRUPTED with prompt_target pointing at the agent's own response paragraph. Recovery: bare git commit succeeds and ships the response, but the agent-doc cycle state never closes cleanly. Likely cause: snapshot/baseline tracking does not recover after the IPC patch was applied with malformed markers and then manually edited. Plan path: tasks/agent-doc/plan-prefix-strip-uncommitted.md (to be authored). Related: #adoc-jb-cache-cancel-stuck-cycle (precondition for this bug).
- [ ] [#jbrsrbusysw] SimWorld scenario coverage for session_restart --force: route + Restart Supervisor while pane is alive-busy with and without --force, asserting the new refusal message and force success (including session_restart_busy_pre_interrupt_idle and session_restart_busy_force_killed event variants). Substantial — requires new Op variant + tmux/session_actor mock in src/sim_world.rs. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (SimWorld piece).
- [ ] [#jbrsrbusyide] JB plugin IDE-side telemetry for session_restart --force: surface session_restart_force_used, session_restart_busy_pre_interrupt_idle, and session_restart_busy_force_killed events in the IntelliJ notification area or Logcat so users can see the recovery path the binary took. Kotlin work in editors/jetbrains/. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (IDE piece).
- [ ] [#planperphase] Convention: split multi-phase plans into per-phase backlog IDs (e.g. #crdtrespfx1, #crdtrespfx2) instead of one parent ID that gets pending-gated. Lets queue auto-strike honor per-phase progress. Lower priority — the convention is opt-in for authors. [recommended]
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jbccc2] Phase 2 of #adoc-jb-cache-cancel-stuck-cycle: JB plugin undo-on-cancel. editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt — defer the write until user resolves the File Cache Conflict dialog (never queue change beforehand); on Cancel return 0/2 from socket IPC callback; on file-IPC cancel leave patch file in place. Plan: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md
- [/] [#agent-doc-commit-sibling] agent-doc CLI: add --commit-sibling <repo> mode to finalize/write. When the session doc lives in repo A but the user's directive originates a commit in sibling repo B (e.g. session-share session-doc drives a build-party scripts change), agent-doc should be able to stage and commit the sibling repo with a Session-Doc git trailer auto-injected. Trailer format: 'Session-Doc: https://github.com/<session-share-org>/<repo>/blob/<session-share-cycle-sha>/<session-doc-path>'. Today agent-doc only commits inside the session-doc's own submodule, so the agent (Claude) creates sibling commits manually via 'git commit' and the trailer is easy to forget. Companion item #cross-repo-sessiondoc-trailer in tasks/docs.md covers the convention + backfill-updates.sh changes across the four script copies. Plan path: tasks/agent-doc/plan-commit-sibling.md
- [/] [#adoc-jb-cache-cancel-stuck-cycle] agent-doc-bug: after canceling a JetBrains File Cache Conflict dialog during IPC patch delivery, the response patch lands in the working tree but the cycle does not reach committed state. Recovery requires running agent-doc write --commit FILE manually. Repro: have JB IDE open with the file in an editor; run finalize; cancel the JB cache-conflict dialog when it appears. Expected: cycle reaches committed cleanly or surfaces a clear error. Actual: response is in working tree but next preflight reports post-commit local drift requiring manual recovery. Plan path: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md (to be authored). Related: #jbccfix, #jbccreg, #jbccdocs in this same file.
- [/] [#jbrsrbusy] [partial] Phase 1 (binary) landed: agent-doc session restart <FILE> --force flag accepts alive-busy panes, refusal message now ends with the --force hint citing the exact command, and the force path logs session_restart_force_used. Plan phases 2 (JB plugin confirm dialog → invokes --force), 3 (optional harness-aware pre-interrupt with 2s idle wait), and 4 (SimWorld coverage + IDE-side telemetry) split into #jbrsrbusyjb, #jbrsrbusyint, and #jbrsrbusysim. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [/] [#stuckcycledup] [recommended] [partial] Wedged-cycle pattern: cycle_already_committed gate now refuses the short-circuit when the active capture body is absent from HEAD (flow/closeout.rs::cycle_already_committed + new stuck_captured_cycle helper), and preflight emits a stuck_captured_cycle warning. Remaining plan parts split into #stuckcycleorphan, #stuckcyclerepair, and #stuckcyclesuperseded. Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [/] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
