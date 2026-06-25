---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260625-004903.md`*

Compacted content:
- Archived 7 response topic(s): #jbrestartbusy; queue resume + flood-pin dedup; live verify (yours, no gate); 4 more
- Prior summary/context: prior compacted content: Archived 11 response topic(s): JB Run Agent Doc on equityfundingsource.md; IPC proof dogfood...

### Re: bounded session-startup wait + isolating why the session hangs — opus-4.8

> **Queue prompt:**
>
> "Add a bounded `session_startup` wait in `route.rs`/`start.rs` with a hard deadline; on expiry, fail closed with `startup_miss` + an actionable error ("session did not become dispatch-ready in Ns") rather than hanging.": Can you retry or isolate why the session hangs so we can further dogfood these sort of errors?


**Root cause of the hang (isolated).** Both startup paths matter, and only one was actually unbounded:

- `route.rs` (already bounded) — `wait_for_agent_ready` polls `while start.elapsed() < timeout` and, on the `fresh_route_start_missing` branch, records a `startup_miss` and fails closed. That side was never the hang.
- `start.rs` auto-trigger thread (the real culprit) — after the harness child was spawned, the thread waited for a dispatch-ready prompt. On the 30s `AUTO_TRIGGER_TIMEOUT` it logged a **provisional** `no_prompt_after_30s` timeout and then **kept polling the child forever** (`monitor.note_no_prompt` only flipped a flag; the loop never returned). A harness that never becomes dispatch-ready — hung TUI, auth wall, stuck network — therefore left the session silently hanging with no recoverable signal. That is the "session hangs" you hit.

**Fix shipped (`#startupdeadline`, agent-doc `6b6451ce`, pointer `219ebb247`).** The 30s deadline is now **hard**:

- New `auto_trigger_no_prompt_action` returns `FailClosed` exactly once when the monitor's deadline expires; the thread then calls `record_session_startup_miss` and `return`s instead of looping.
- `record_session_startup_miss` writes a `startup_miss` marker against the owned pane (`StartupMissOrigin::FreshStart`) and prints the actionable line you asked for:
  `[agent-doc] session did not become dispatch-ready in 30s (no_prompt) for <harness>; recorded startup-miss and failing closed instead of hanging. Run 'agent-doc start <FILE>' to retry.`
- The same fail-closed path now also covers the two other startup waits that previously only logged and bailed: `clear_cooldown` and managed `capability_proof`. So no startup branch can hang silently.
- The stale module-doc ("logs a provisional timeout but keeps watching until the child exits") is rewritten to describe the hard deadline.

**Verification.** `make check` → **5101 passed, 241 skipped**; `cargo clippy -D warnings` clean; `cargo build --release` green. New unit eval `auto_trigger_no_prompt_continues_before_deadline_then_fails_closed` pins continue-before-deadline → fail-closed-once → caller-returns.

**Pre-existing red I had to clear to push.** The suite was already red on HEAD — commit `9444a398` bumped the root package + `agent-doc-orchestration` to `0.34.48` but left `agent-doc-core` / `agent-doc-markdown-ast` / `agent-doc-sqlite` and the root dep pins at `0.34.46`. Cargo's caret resolution masked it in builds, but `test_manifest_uses_publishable_dependency_contract` does exact-version equality. I completed the already-landed 0.34.48 bump across those manifests + `Cargo.lock` (no tag/publish — pure reconciliation).

**Closeout.** Committed + pushed to `origin/main` (agent-doc `6b6451ce`; superproject pointer `219ebb247`). The supervisor auto-installs + recycles the new binary at its idle boundary, so I did not manually install mid-session against the live supervisor. CI run `28140216452` is in progress — recorded as pending; proceeding on local evidence per the test-bearing-turn rule.

**Dogfood note:** this very closeout hit the IPC class you're tracking — `finalize --stream` got a 6s socket ack timeout, fell back to file-IPC, then `no_ack` (`recovery=retry_without_disk_write`) because the editor never applied the queued patch (no live-buffer sidecar for this doc; supervisor reports fresh). Recovered via the explicit `finalize --force-disk` escape hatch.

Live dogfood of the hang→`startup_miss` behavior in a real editor/pane needs a human eyeball, so I filed it as `[operator-verify]` **#nsfz** rather than gating the ship.

### Re: state machine for the wait machinery + Lean no-hang proof — opus-4.8

> **Queue prompt:**
>
> Can we have a state machine for the wait machinery? And prove that we will never hang with tests & formal logic (i.e. Lean)? We should never hang for more than 10s. I know there is a dogfood reinstall that we pause for, which is the only exception.


Yes — and it's the right next step, because the wait machinery is currently *not* one thing. I inventoried every blocking wait and they're scattered ad-hoc loops with inconsistent budgets, several of which already violate your 10s ceiling:

| Site | Budget today |
|------|------|
| `start.rs` `AUTO_TRIGGER_TIMEOUT` (the `#startupdeadline` deadline I just shipped) | **30s** |
| `route/startup.rs` `wait_for_agent_ready` (fresh-route ready) | **30s** |
| `route.rs` busy-reroute / shell waits | 20s, 15s, 10s, 8s, 5s, 3s, 2s |
| IPC socket ack (finalize) | 6s socket + 2s file |

So today's worst case is ~30s, not 10s. That's exactly the gap a single state machine closes.

**Design (filed as a plan, not done inline — this is a deep supervisor-critical refactor):** `tasks/agent-doc/plan-wait-machinery-state-machine.md`.

- A new `WaitMachine` with a **pure** `tick(state, signal, elapsed) → state` transition (no I/O — clock/PTY/socket reads live in a thin driver). States: `AwaitingShell`, `AwaitingDispatchReady`, `AwaitingClearCooldown`, `AwaitingCapabilityProof`, `AwaitingIpcAck`, `ReinstallPause`, `FailedClosed`, `Ready`.
- **One** `GLOBAL_HANG_CEILING = 10s` enforced in one place. Any non-exempt state hitting its `max_dwell` returns `FailedClosed` — never "keep polling" (generalizing the `#startupdeadline` fix to *every* wait).
- `ReinstallPause` is the **one typed exemption** you called out (the dogfood reinstall/recycle pause) — the only state allowed past 10s, and even it is bounded by a separate explicit `REINSTALL_BUDGET` and logged distinctly so a pause is never misread as a hang.

**Proving it never hangs (both kinds you asked for):**
1. *Tests* — per-state continue-before-deadline → fail-closed-once unit tests, plus an exhaustive `(state × signal)` property test asserting `Σ dwell ≤ 10s` on every exempt-free path, plus SimWorld coverage for the live hang classes.
2. *Lean* — `formal/wait_machine/` encodes `WaitState` as an inductive type and `tick` as a `Step` relation mirroring the Rust 1:1, then proves `no_hang : ∀ p, Reachable p → ExemptFree p → dwell p ≤ 10s`, with a second lemma that `ReinstallPause` is the sole state that can exceed it. The honest caveat: Lean proves the *model*; a Rust↔Lean `tick`/`Step` mismatch would make it vacuous, so the plan pins a parity test enumerating the Rust transition table against the Lean one (Rust `tick` is the source of truth).

Queued as four dependency-ordered phases — **#waitmachine1** (core), **#waitmachine2** (migrate call sites + lower the 30s budgets to the 10s ceiling), **#waitmachine3** (deterministic bound proof), **#waitmachine4** (Lean). Each phase is its own focused cycle per the deep-refactor pacing rule; #waitmachine2 is where your "never >10s" constraint actually lands.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=file_ipc patch_id=c33e70a3-1965-4729-a7cb-c0a94ab663b1 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.json doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1782349156751
```
<!-- agent:boundary:4ced9fb1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~"Add a bounded `session_startup` wait in `route.rs`/`start.rs` with a hard deadline; on expiry, fail closed with `startup_miss` + an actionable error ("session did not become dispatch-ready in Ns") rather than hanging.": Can you retry or isolate why the session hangs so we can further dogfood these sort of errors?~~
- ~~Can we have a state machine for the wait machinery? And prove that we will never hang with tests & formal logic (i.e. Lean)? We should never hang for more than 10s. I know there is a dogfood reinstall that we pause for, which is the only exception.~~
- 🚧 What are #next-steps to use lazily-ky and lazily-js to sync the plugin state machines and state graphs from the FFI plugin?
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
- Still getting JB `File Cache Conflict` dialogs.
- Opencode responses are reverse ordering numeric lists.
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
---
equityfundingsource.md turn has an IPC issue:
`### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc`

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=ca5b06f9-838b-4b60-9ce2-00519736fbb1 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782344406484
```
---
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
- do [#npe1]
- do [#p2j4]
- do [#4pdp]
- do [#0bmv]
- do [#actorswitchdefer]
- ~~:pushpin: do [#jbrunhang]~~
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
- do [#capproofbg]
- do [#rt83qflood]
- do [#nsfz]
- do [#gghx]
- do [#8g9w]
- do [#6mjw]
- do [#spy9]
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
- [ ] [#gghx] #waitmachine1 Wait-machinery state machine core: add agent-doc-orchestration/src/wait_machine.rs with a pure WaitState/tick transition + single GLOBAL_HANG_CEILING (10s) + typed ReinstallPause exemption. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#8g9w] #waitmachine2 Migrate scattered waits (start.rs AutoTriggerMonitor 30s, route/startup.rs wait_for_agent_ready 30s, route.rs busy/shell waits, IPC ack) onto WaitMachine and lower budgets to the 10s ceiling (operator: never hang >10s). Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#6mjw] #waitmachine3 Deterministic bound proof: per-state transition tests + exhaustive (state×signal) property test asserting Σdwell≤10s on every exempt-free path + SimWorld hang-class coverage. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#spy9] #waitmachine4 Lean model: formal/wait_machine/ encoding WaitState/Step mirroring Rust tick, theorem no_hang (exempt-free ⇒ dwell≤10s) + reinstall-pause sole-exemption lemma + Rust↔Lean parity test. Plan: tasks/agent-doc/plan-wait-machinery-state-machine.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#rt83qflood] #rt83/#qflood phantom-pin flood dedup: queue convergence leaves duplicate binary-injected :round_pushpin:/:pushpin: actor-switch pins after a stale-CRDT/supervisor re-emit, flooding the queue with ~7 near-identical copies. Add deterministic dedup that collapses repeated phantom pins (the operator-authored line is position-locked under #queue-operator-pin-position-lock, but binary-injected phantom copies must converge to one) so restarting/resyncing a stale supervisor cannot reflood the queue.
- [ ] [#capproofbg] Make the JB Run Agent Doc capacity proof non-blocking: dispatch immediately and run the capacity proof in the background, surfacing failure asynchronously instead of stalling every dispatch.
- [ ] [#npe1] #codefencestrip=Template patch parser strips trailing code fences at the exchange boundary
- [ ] [#p2j4] #jbcfdiag=JB File Cache Conflict: investigate remaining write-path triggering the JB VFS dialog beyond IPC-first writes
- [ ] [#4pdp] #ocreverselist=OpenCode reverse-ordered numeric list rendering investigation
- [ ] [#0bmv] #qeditrace=Consolidate queue-edit-during-active-turn race plans (plan-qdurcrash + plan-supervisor-direct-queue-edit-race)
- [ ] [#actorswitchdefer] Part B: SimWorld proof idle-watch fires restart when deferred + reconcile SPEC.md:141 defer contract. Part A shipped (6c7455a8).
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
