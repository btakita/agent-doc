---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: opencode
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
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-213413.md`*

Compacted content:
- Archived 7 response topic(s): IPC integration and monsterrodholders queue audit; IPC proof issue dogfood log; queue repair; 4 more
- Prior summary/context: *Compacted earlier. Prior content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103...

### Re: JB Run Agent Doc on equityfundingsource.md — codex→opencode switch stranded — glm-5-2

Root cause: the error is **not a regression** — it's the `#agentreloadrestart` kill-guard doing exactly what plan-restart-agent.md added after this same doc's earlier mid-turn kill. `route/authoritative_actor.rs:40-95` sees the stored actor harness (`codex`) differ from the frontmatter harness (`opencode`) with `frontmatter_harness_changed=true`. `mismatched_authoritative_actor_can_be_replaced` returns `false` because the old codex actor is healthy/non-closed (test `..._only_when_not_live_authority`), so route logs `route_authoritative_actor_harness_mismatch_deferred action=defer_to_boundary_restart` and bails instead of cold-replacing the live pane.

The deferred work is owned by the **supervisor idle-watch** (`start/idle_watch.rs:715-803`): each tick it re-resolves `agent:` from frontmatter, and at a `prompt_visible && !turn_active` boundary `agent_change_restart_decision` returns `Restart` → requests a fresh restart → `run.rs` respawns opencode (`agent_restart_performed`). Phase 1a detection + Phase 1b execution are both shipped.

**The actual bug: the defer is a dead-end.** It dead-ended here because the supervisor never reached the boundary — and the bail gave you no way to know that or force it:
- this session is `queue_paused: true` (stale `#rt83/#qflood` pause), so the idle-watch wasn't ticking → `harness_change_detected` never fired;
- or the codex pane never returned to a dispatch-ready prompt (`turn_active` / not `prompt_visible`) → decision stuck at `WaitForBoundary`;
- or `agent_change_restart_enabled` resolved off.
- **Healthy + ready:** confirms idle-watch + offers force-now cmd
- **Pane busy/starting:** suggests `--force`
- **Queue paused or supervisor unreachable:** names the blocker + `Run: agent-doc session restart-supervisor <FILE>`

In every branch, route's "deferring to boundary agent restart" is the last thing you see.

**Next step (captured):** added `#actorswitchdefer` (top of backlog) + plan `tasks/agent-doc/plan-actor-switch-defer-recovery.md`. Two parts — (A) enrich the route bail with supervisor health/pause state and a concrete `agent-doc session restart-supervisor <FILE>` recovery hint (small, safe, unblocks today); (B) SimWorld proof that the idle-watch actually fires the restart when deferred (or surfaces paused/stuck), plus reconciling SPEC.md:141 (which still says "fall back to create/rebind," contradicting the shipped defer). The fail-closed rule for healthy actors stays — this only fixes the message and proves the deferred path.

Note: the queue on this doc is still `queue_paused` (stale supervisor pause from `#rt83/#qflood`; we're now on 0.34.43, well past the "install 0.34.20 + recycle supervisor" resume condition). `#mrhqrestore` and `#ipcqproof` remain gated. Say the word and I'll (1) run Part A of `#actorswitchdefer` in a focused cycle, and/or (2) recycle the supervisor so `#mrhqrestore`/`#ipcqproof` can drain.
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=bce274079aeab24663364a54fa79caf3ffd764c7a9b6a1a41ab966054e0303e9 content_len=19251 content_hash=7314858727478b5438a9e8e5358d47544c239304de2dc695c6b4d27b6a1af813 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782336317276
```

### Re: IPC proof dogfood log acknowledged — glm-5-2

The `ipc_proof_insufficient` dogfood log appended during interrupted-cycle recovery is noted — it is the `#ipcqproof` backlog item in action (socket ACK content mismatch on `live_prompt_drift_after_preflight`). The full response for this cycle (queue triage + #actorswitchdefer Part A shipped) was already committed in 0c3a1787f; this write closes the prompt-only tail the session-check flagged.

### Re: #ipcqproof shipped — IPC diagnostic no longer prompt-bearing — glm-5-2

Acknowledged the exchange cleanup: the three recovery-suffix bullets now sit in the `#actorswitchdefer` bug spec (where they describe Part A's shipped behavior), and the redundant "Part A shipped" block + duplicate heading were collapsed into the single `(HEAD)` marker. The struck `do [#actorswitchdefer]` is correct — Part A is done; Part B stays in backlog.

**#ipcqproof — SHIPPED (7b31fb9a, v0.34.46).** Root cause: the `ipc_proof_insufficient` dogfood note appended during interrupted-cycle recovery was bare text closing the exchange tail, so `classify_prompt_bearing_block` (agent-doc-core/diff.rs) tagged it `PromptTarget` → the write path `❯`-normalized it → `prompt_only_exchange_tail` saw a prompt-only tail → forced an extra acknowledgment cycle. That is the exact "IPC proof dogfood log acknowledged" churn on this doc.

Fix (`preflight.rs::format_ipc_dogfood_note`): the note now opens with a `### Re:` heading and folds its body into a fenced block. The first non-blank line matches `is_recovery_artifact_line`, so the whole note is one `RecoveryArtifact` block — `first_unstarted_prompt_bearing_change` skips it, `prompt_prefix_normalization_targets` ignores it (no `❯`), and `prompt_only_exchange_tail` clears on the response heading. A fence-opening line after a blank joins the heading's block, and blank lines inside a fence never split it, so the entire note stays a single recovery-artifact block.

Recovery stays fail-closed on the binary-owned path (`retry_without_disk_write` / `content_ours_snapshot_next_cycle`, never `direct_write_fallback`).

Regression `ipc_dogfood_note_is_recovery_artifact_not_prompt_bearing` pins all four invariants — RecoveryArtifact classification, no `❯` normalization, no prompt-only tail, fail-closed recovery — for both the `live_prompt_drift_after_preflight` and `missing_response_probe` variants. Full suite (5097) + clippy green; `make install-full` done and the supervisor is marked to recycle onto 0.34.46.

Changed paths: `agent-doc-orchestration/src/preflight.rs`, `VERSIONS.md`, `Cargo.toml` + workspace member manifests + `Cargo.lock`, `pyproject.toml`. Next queue head is `#mrhqrestore` (still 🚧 gated) — recycling the supervisor will clear the stale `#rt83/#qflood` pause and let it drain.

### Re: queue-removal guard acknowledged for #actorswitchdefer — glm-5-2

The struck `~~do [#actorswitchdefer]~~` was an explicit operator edit: Part A already shipped (`6c7455a8`), so the immediate queue slice is done. Part B (SimWorld proof + SPEC.md:141 reconciliation) remains tracked in `agent:backlog` under the same id — it is not lost, just not an active queue head right now. Marking the removal intentional so the `#queue-clear-unrun-items` guard stops flagging it; re-queue `do [#actorswitchdefer]` when Part B is ready to drain.

<!-- no-queue-removal-guard -->
<!-- agent:boundary:83f062f4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 Queue items are duplicated in lazily-rs.md by agent-doc.
- JB `Recycle Supervisor` receives a message that the agent is busy even though the opencode terminal is idle. The state needs to be updated to idle once the turn completes...even if the turn did not finalize. If the agent stops the turn due IPC churn or a Supervisor version mismatch, the state should indicate the agents idle. No need for the extra 2 steps of to press "Interrupt & Restart" + the confirmation.
---
lazily-rs.md IPC error
```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=8c56928442bd4b165543ab0aad264d681b41b8898551d10f196018cd15a37304 content_len=11944 content_hash=46b47fdfe5505fb37c9e63df4167cb923df6b4ffc997ecca9009a15979c29a5e doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782339826601
```
---
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- If I run JB `Run Agent Doc` on a session not open, the next JB `Run Agent Doc` hangs. The first JB `Run Agent Doc` should immediately fail.
- The Capacity Proof should not block the JB `Run Agent Doc` dispatch. Dispatch should occur immediately while the Capacity Proof runs in the background.
- Still getting JB `File Cache Conflict` dialogs.
- Opencode responses are reverse ordering numeric lists.
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
:round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- do [#mrhqrestore]
- do [#mrhqrestore]
- do [#npe1]
- do [#p2j4]
- do [#4pdp]
- do [#0bmv]
- do [#mrhqrestore]
- do [#actorswitchdefer]
- Queue items are duplicated in lazily-rs.md by agent-doc.
---
🚧 :round_pushpin: JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
- 🚧 do [#mrhqrestore]
---
JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the agent state change.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
---
JB `Run Agent Doc` on equityfundingsource.md after switching from codex to op. I have to kill + restart the tmux pane or recycle the supervisor for the opencode state to pick up. The supervisor should reactively pick up the new agent state.
```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 0
Error: authoritative actor record for tasks/monsterrodholders.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
```
[route] target tmux session: 
Error: authoritative actor record for tasks/agent-doc/agent-doc-bugs2.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```
---
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
- [ ] [#ocdrainstall] OpenCode agent-doc active-queue drain stalls after each committed closeout. SKILL says "OpenCode currently has no auto-loop" which agents read as "stop after each cycle," but it means "no AUTOMATED mechanism — the agent must self-drive the drain in-turn." Result: the agent stops and forces operator re-invocation per head; compounded by deferring drainable heads as "redundant" (#drain-no-defer violation). Claude Code uses /loop; Codex uses agent-doc hook codex-stop + .agent-doc/queue-continuations marker; OpenCode has neither. Fix: Part A (ship first) rewrite OpenCode SKILL auto-loop guidance to mandate in-turn self-drive — after a proven closeout with queue_continuation_required=true, keep running preflight->respond->finalize/write --commit in the SAME turn until drained or a real stop condition; Part B optional agent-doc hook opencode-stop if opencode 1.17 exposes a lifecycle hook; Part C SimWorld multi-head in-turn drain test. Plan: tasks/agent-doc/plan-opencode-queue-drain-self-drive.md
- [ ] [#capproofbg] Make the JB Run Agent Doc capacity proof non-blocking: dispatch immediately and run the capacity proof in the background, surfacing failure asynchronously instead of stalling every dispatch.
- [ ] [#jbrestartbusy] Supervisor reports agent busy when the OpenCode pane is idle: turn state must flip to idle on a non-finalize turn exit (IPC churn / supervisor version mismatch), and Recycle Supervisor must skip the Interrupt & Restart + confirmation 2-step. See plan-jb-restart-supervisor-busy-refusal.md, plan-opencode-post-turn-false-active.md.
- [ ] [#jbrunhang] JB Run Agent Doc on a session that is not open hangs the next invocation: the first run against a closed session must fail immediately instead of wedging the subsequent dispatch. See plan-controller-stalled-client-start-hang.md.
- [ ] [#jbdupqueue] agent-doc duplicated queue items into lazily-rs.md: verify whether already fixed by #prompt-preempts-auto-queue (semantic match 0.828) or a fresh cross-document queue-duplication regression.
- [ ] [#npe1] #codefencestrip=Template patch parser strips trailing code fences at the exchange boundary
- [ ] [#p2j4] #jbcfdiag=JB File Cache Conflict: investigate remaining write-path triggering the JB VFS dialog beyond IPC-first writes
- [ ] [#4pdp] #ocreverselist=OpenCode reverse-ordered numeric list rendering investigation
- [ ] [#0bmv] #qeditrace=Consolidate queue-edit-during-active-turn race plans (plan-qdurcrash + plan-supervisor-direct-queue-edit-race)
- [ ] [#actorswitchdefer] Part B: SimWorld proof idle-watch fires restart when deferred + reconcile SPEC.md:141 defer contract. Part A shipped (6c7455a8).
- [ ] [#mrhqrestore] Improve agent-doc queue recover-lost for git-history-only queue deletions: produce an operator-reviewed restoration patch or explicit non-restorable recommendation for foreign-owned prompts such as the monsterrodholders test-email CSV row.
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
