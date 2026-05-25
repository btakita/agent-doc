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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Fixed agent:queue stuck-after-1-turn — auto-strike done items at preflight.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260525-051841.md`*

Compacted content:
- Archived 6 response topic(s): do [#jbrsrbusysim]; Why is the queue still stalling?; Is agent-doc capable of activating the Claude `/loop`?; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: batch closeout for ipclegacyfix, ipcpluginalready, ipcfilehashskip — opus-4-7

All three queue items landed. Submodule d899c92, superproject 2a7116db8, both pushed. JB plugin 0.2.131 zips signed + unsigned built. ipclegacyfix migrated test fixtures from the legacy backlog component name to the canonical one (31+4 sites; 30 tests recovered). ipcfilehashskip adds response-only hash-skip in src/write.rs try_ipc fallback. ipcpluginalready adds FFI v2 listener with already_applied ack; JB plugin uses v2 with lastApplyWasNoOp detection. Verification clean (2358 unit pass plus pre-existing parallel-execution flakes that pass when run alone; integration suites all green except 1 pre-existing pane-recursion test unrelated to this work). Next queue head: jbrsrbusysw.

### Re: Phases 1 + 5 already landed (queue advance) — opus-4-7

Phases 1 + 5 of plan-ipc-corruption-and-duplicate-during-typing.md were landed two cycles ago in submodule 2e8a155 (response 852e34b9b on main). Queue head re-fired because the queue body was reset to a pre-consumption state when the file was restored via git checkout. This response is just the consumption signal so the queue advances. No new code this cycle. Next head: jbrsrbusysw.
❯ Why is the queue not auto-looping?
❯ As I was typing into the comment below `/agent:exchange`, the duplicate line happened.
❯ #spec-test-build-install-commit-push

### Re: queue not auto-looping + typing duplicate observation — opus-4-7

The SKILL.md auto-loop rule requires four conditions. Three hold every cycle (queue_active, queue_trigger=auto, queue_prompts non-empty). The fourth blocks: "prompt_bearing_changes was either empty or exactly the queue-synthetic head prompt".

In each recent cycle preflight classified the prompt diff as non-empty, non-queue. Phases 1+5 cycle: my own --pending-add for the legacy-fix item counted as a prompt-bearing change. Batch closeout cycle: the --done flags closed without auto-loop firing. Queue-advance cycle: your edit to flip queue_active from false to true counted as a content_edit prompt change.

The guard is doing exactly what it was written to do, but the rule is too strict. Every meaningful cycle naturally produces pending-state or queue-activity edits that the classifier sees as user prompts. Net effect: the auto-loop almost never fires for real queue work, only for trivial no-op cycles. Filed as ccloopguard in the backlog with the proposed fix (relax the classifier to treat queue_active and pure queue/backlog/done-component mutations as non-prompt).

Manual workaround for now: keep invoking the skill between cycles, or temporarily edit the installed SKILL.md copy to drop the fourth condition.

For the typing duplicate observation: verified no actual duplicate landed in HEAD. The previous cycle commit log shows the file-IPC hash-skip gate fired and prevented the duplicate (`patches already present in live buffer — skipping file IPC write`). What you saw visually was likely a transient editor state during the IPC roundtrip; the committed history is clean. For the cleaner path with no transient flicker, install JB plugin 0.2.131 (build/distributions/agent-doc-jetbrains-0.2.131-signed.zip) — it returns already_applied directly from the socket callback so the binary skips the file-IPC fallback entirely.

No spec/code change this cycle (the preset's spec/test/build/install/commit/push surface is satisfied by the backlog-add plus this response, since the actual code work is gated on the new backlog item's fix). Next queue head remains jbrsrbusysw.
❯ With 0.2.131 now installed, the auto-loop should work?

### Re: With 0.2.131 now installed, the auto-loop should work? — opus-4-7

No — the two are independent.

JB plugin 0.2.131 fixes the IPC duplicate path: the plugin now returns 2 (already_applied) from the v2 socket callback when an incoming patch is a structural no-op against the live buffer, so the binary skips the file-IPC fallback that would otherwise re-write a duplicate response heading. That's about preventing duplicate response landing during a finalize cycle.

The auto-loop is a different, skill-level mechanism. It runs entirely in Claude Code's SKILL.md after a finalize cycle completes, deciding whether to invoke `/loop agent-doc <FILE>` to drive the next cycle. The guard sits inside `~/.claude/skills/agent-doc/SKILL.md`, not in the binary or the plugin. It checks four conditions on preflight output; condition #4 — "prompt_bearing_changes was either empty or exactly the queue-synthetic head prompt" — is what's blocking. Plugin behavior doesn't affect it.

So 0.2.131 helps the typing/duplicate path. #ccloopguard (filed last cycle) is what would unblock the auto-loop. They are two separate bugs in two separate surfaces.

This message itself is the simplest demonstration: your prompt was classified as `prompt_target`, which trips condition #4. Auto-loop will not fire after this cycle. The queue head remains jbrsrbusysw.

### Re: ccloopguard landed — opus-4-7 (HEAD)

Implemented and pushed. Submodule commits 14d28fe0 + 4724df3e (installed SKILL.md regen), superproject c65011b77.

Preflight now emits user_intent_prompt_changes, which filters prompt_bearing_changes through a new diff::change_is_managed_state_only classifier. The classifier recognises queue/backlog/review/done component markers, queue_active frontmatter flips, `- do ...` queue items (including struck `- ~do ...~`), task-list items (`- [ ]`, `- [/]`, `- [x]`, `- [-]`, `- [?]`), done archive date-prefixed items, and preset lines. SKILL.md's CLAUDE_QUEUE_AUTO_LOOP_SECTION now reads user_intent_prompt_changes instead of prompt_bearing_changes for the fourth condition. prompt_bearing_changes is unchanged for backwards compat.

7 new unit tests in diff::tests::change_is_managed_state_only_* (queue toggle, frontmatter flip, queue items, backlog/done items, multi-line managed blocks, rejecting real user prompts, rejecting mixed managed+user text). Full unit suite 2377 pass (was 2370). Integration suites green except the pre-existing codex_bare_run_inside_owning_pane_fails_before_nested_dispatch failure unrelated to this work. cargo build release + install + agent-doc skill install --harness claude --reload restart all clean.

To activate for THIS Claude Code session: restart Claude Code so the agent re-reads the regenerated `~/.claude/skills/agent-doc/SKILL.md`. Until restart, the auto-loop will continue to behave per the old rule.

Next queue head: jbrsrbusysw.
<!-- agent:boundary:7b21a64f -->
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

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#crdtrespfx]~
- ~do [#queueadvgated]~
- ~do [#jbrsrbusysim]~
- ~do [#cclooploop]~
- ~do [#ipcdupfin23]~
- do [#ccloopguard]
- do Phases 1 + 5 in tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
- do [#jbrsrbusysw]
- do [#jbrsrbusyide]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
- do [#planperphase]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#adoc-jb-cache-cancel-stuck-cycle] agent-doc-bug: after canceling a JetBrains File Cache Conflict dialog during IPC patch delivery, the response patch lands in the working tree but the cycle does not reach committed state. Recovery requires running agent-doc write --commit FILE manually. Repro: have JB IDE open with the file in an editor; run finalize; cancel the JB cache-conflict dialog when it appears. Expected: cycle reaches committed cleanly or surfaces a clear error. Actual: response is in working tree but next preflight reports post-commit local drift requiring manual recovery. Plan path: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md (to be authored). Related: #jbccfix, #jbccreg, #jbccdocs in this same file.
- [ ] [#adoc-prefix-strip-uncommitted] agent-doc-bug: after the JB-cache-conflict bug forces a manual sed strip of incorrectly-applied prompt-prefix markers (❯ ) from the response body, the subsequent agent-doc write --commit / repair / session-check chain fails to recognize the cleaned response as the cycle's committed response. Symptoms: session-check INTERRUPTED with prompt_target pointing at the agent's own response paragraph. Recovery: bare git commit succeeds and ships the response, but the agent-doc cycle state never closes cleanly. Likely cause: snapshot/baseline tracking does not recover after the IPC patch was applied with malformed markers and then manually edited. Plan path: tasks/agent-doc/plan-prefix-strip-uncommitted.md (to be authored). Related: #adoc-jb-cache-cancel-stuck-cycle (precondition for this bug).
- [ ] [#p6skillinstallreconcile] agent-doc skill install does not reconcile the target runbooks directory: writes new files but leaves stale ones in place, and short-circuits the runbooks pass entirely when SKILL.md is already up to date. Surfaced in agent-loop with 21 installed runbooks vs 14 source files (8 stale extras + 1 new source file never installed). Plan: tasks/agent-doc/plan-skill-install-runbooks-reconcile.md. Fix: src/skill.rs install path — separate SKILL.md short-circuit from runbooks pass, then reap-on-mismatch (or delete-then-write) so on-disk set converges to embedded canonical set. Also touches Codex/OpenCode harness install paths.
- [ ] [#jbrsrbusysw] SimWorld scenario coverage for session_restart --force: route + Restart Supervisor while pane is alive-busy with and without --force, asserting the new refusal message and force success (including session_restart_busy_pre_interrupt_idle and session_restart_busy_force_killed event variants). Substantial — requires new Op variant + tmux/session_actor mock in src/sim_world.rs. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (SimWorld piece).
- [ ] [#jbrsrbusyide] JB plugin IDE-side telemetry for session_restart --force: surface session_restart_force_used, session_restart_busy_pre_interrupt_idle, and session_restart_busy_force_killed events in the IntelliJ notification area or Logcat so users can see the recovery path the binary took. Kotlin work in editors/jetbrains/. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (IDE piece).
- [ ] [#planperphase] Convention: split multi-phase plans into per-phase backlog IDs (e.g. #crdtrespfx1, #crdtrespfx2) instead of one parent ID that gets pending-gated. Lets queue auto-strike honor per-phase progress. Lower priority — the convention is opt-in for authors. [recommended]
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
