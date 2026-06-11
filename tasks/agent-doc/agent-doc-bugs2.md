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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260611-172026.md`*

Compacted content:
- Archived 6 response topic(s): #vbc1 next agent-doc bug backlog; #queueeditloss; #eqrecovery; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: #next-steps to fix bugs before release — opus-4-8

Backlog was empty; I rebuilt it in priority order. **Top of backlog: `#2v7b` (#jbcodexsubmit)** — the JB→Codex submit bug you just reported (diagnosed below), the only functional release blocker.

Priority order:

1. **`#2v7b` #jbcodexsubmit** — JB `Run Agent Doc` doesn't submit to Codex. Functional, agent-fixable, has a plan. *Release blocker.*
2. **`#qt32` #lvbremain** *(recommended)* — operator live zero-drift proof of the 08b supervisor-in-process cutover. Code-complete + pushed already; only a live editor can prove it. Operator-drive, not agent-completable.
3. **`#02bw` #fmtchurn** — the working tree has 13 files of uncommitted **rustfmt-only** whitespace churn (no semantic changes — I checked the diff). Decide commit-as-`style:` vs discard before tagging. Cosmetic.

Per memory the entire 08b cutover (pcpc1–pcpc5, removal rung) is code-complete and on `origin/main`; no implementation backlog remains there. So the path to release is: land `#2v7b`, you live-verify it + `#qt32`, clean up `#fmtchurn`, tag.

### Re: JB `Run Agent Doc` on Codex still doesn't submit the prompt — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix bugs? I want to release soon.


Diagnosed and filed as `#2v7b` with a plan at `tasks/agent-doc/plan-jb-codex-submit.md`.

**Root cause:** Codex isn't owned-PTY managed (only claude/opencode are), so the JB action routes through `route.rs::send_command_unchecked` → direct-pane submit, which sends the trigger text + `\r` as **one** `send-keys -H` payload. That path *already detects* the non-submit — when the trigger text is still visible after the acceptance timeout it logs `route_submit_issue ... issue=prompt_not_submitted` (`TriggerStillVisible`, route.rs:6624) — but it just returns `TimedOut`. **There is no re-submit fallback.** The Codex TUI appears to treat the merged trailing `\r` as a literal newline (drafted multi-line) rather than a submit key, which is exactly your "entered into input, I had to submit manually" symptom.

**Fix (planned, not yet implemented):** Codex-scoped one-shot fallback — when direct-pane acceptance detects `TriggerStillVisible` on a Codex pane, send a *separate* bare `Enter` key event and re-poll once, gated to `harness == codex` so claude/opencode are untouched, bounded to a single retry. Plus a `route_submit_resubmit ... action=enter_key result=accepted|still_visible` proof line so your live test is provable from `ops.log` without a live editor on my side.

I did **not** implement the fix this turn — it's a dispatch-path change that needs your live Codex+JB verification (I can't reproduce the Codex TUI submit behavior here), and your two prompts were "what's next" + a bug report, which per your own `#agent-doc-bug` convention means plan-and-backlog. Say "do `#2v7b`" (or "go") and I'll implement it, add SimWorld coverage, `make check` + build + `lib-install`, then hand it to you to live-verify the `route_submit_resubmit` proof.

### Re: JB `Run Agent Doc` on Codex submit fix — implemented + shipped — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` on Codex still did not submit the prompt to Codex. The prompt was entered into the input, but I had to manually submit it to start the queue.


Fixed under the `go` queue's `#spec-test-build-install-commit-push` preset. Committed `d14dd12f` and **pushed to `origin/main`** (agent-doc submodule).

**What changed** (`route.rs::send_command_unchecked`):
- Factored the direct-pane submit-acceptance poll into `poll_direct_pane_acceptance` (pure detection, no input) so it can run before *and* after a re-submit.
- `#jbcodexsubmit`: when a **Codex** direct-pane submit times out with the trigger still visible, send a *separate* bare `Enter` key event and re-poll the acceptance window **exactly once**, logging `route_submit_resubmit ... action=enter_key result=accepted|still_visible`. Gated through the pure `direct_pane_needs_codex_resubmit` (codex-only, timeout-only, trigger-still-visible-only) so claude/opencode are untouched and it never loops on a stuck pane.

**Verification done this turn:** `make check` green (exit 0), new test `direct_pane_codex_resubmit_only_on_timeout_with_trigger_visible` passes, `cargo build --release` + `cargo install --path .` both refreshed (release symlink + `~/.cargo/bin`), spec `08a-session-actor-contract.md` + `VERSIONS.md` updated.

**Gated `#2v7b` (code-complete, awaiting your live proof).** I can't reproduce the live Codex TUI submit behavior on the agent side, so the end-to-end confirmation is operator-drive — exactly your "I test, you check logs" loop. To verify: trigger JB `Run Agent Doc` against a Codex pane (the binary is already reinstalled), then I read `ops.log` for `route_submit_issue ... issue=prompt_not_submitted` (first attempt) followed by `route_submit_resubmit ... action=enter_key result=accepted`, and the queue actually starting without a manual Enter. If `result=still_visible` shows up instead, the Codex composer needs a different submit shape and I'll iterate.

Note: this is a JB→tmux **binary** path, so no JB plugin reinstall is needed. Remaining for release: this live proof + `#qt32` (08b zero-drift) + `#02bw` (the rustfmt churn cleanup).

### Re: queue cleanup — fmt fixup + tree clean for tagging — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#qt32]


You struck `do [#2v7b]` (the JB→Codex fix is gated, awaiting your live proof). I drained the rest of the queue this cycle and found a gap in my own prior commit.

**Caught + fixed my own fmt violation (`449a87e9`, pushed).** `cargo fmt --check` flagged the two spots I introduced in `d14dd12f` (`direct_pane_needs_codex_resubmit` and the `poll_direct_pane_acceptance` call) — `make check` runs clippy + tests but does **not** gate `cargo fmt`, so it slipped through. Reformatted, rebuilt + reinstalled, no behavior change.

**`#02bw` done (`e6f7538e`, pushed) — working tree is now clean for tagging.** I corrected the characterization: the churn was *not* purely rustfmt. It was 12 `.rs` files of `cargo fmt` reflow **plus** an `AGENTS.md` tsift code-navigation marker bump (v0.1.66→v0.1.67, auto-generated). I proved the 12 `.rs` files are fmt-only via a non-destructive round-trip (`rustfmt(HEAD:f) == worktree:f` for every file — zero semantic deltas), then committed the lot as one honest housekeeping commit. `git status` in the submodule is now empty.

**`#qt32` gated (operator-drive, not a churn item).** The 08b zero-drift proof needs a live editor I can't drive, so I moved it from the actionable backlog into `agent:review` — same treatment as `#2v7b`. That also stops the `priority queue` backlog from re-pulling it into the queue every cycle (the churn you'd otherwise see). It's not "tracked-for-later avoidance" — it's genuinely blocked on your live test.

**State for release:** agent-doc submodule clean + on `origin/main` (`e6f7538e`). Backlog empty. Two gated review items, both operator-drive live proofs:
- `#2v7b` — JB `Run Agent Doc` on a Codex pane should now self-submit; I check `ops.log` for `route_submit_resubmit ... action=enter_key result=accepted`.
- `#qt32` — restart a session, drive one edit-during-finalize cycle; I check `ops.log` for `write_authority action=routed` + `plugin_watch_readonly` + `supervisor_host_gate hosting=in-process` + zero drift lines.

Run those two live and I'll read the logs; then it's tag time. I'm halting the auto-loop here — nothing left that I can complete without your live editor.

### Re: queue flooding + JB File Cache Conflict — halted + captured — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `File Cache Conflict` occurred while I was entering in queue items during finalize. Also `/agent-doc tasks/agent-doc/agent-doc-bugs2.md` accumulated into the Claude Code session queue as I was typing during finalize. I manually deleted the prompt from the Claude Code session queue.


You interrupted into an active flooding bug. I stopped it and captured the reports rather than auto-looping (which would have made it worse).

**Flooding halted.** `agent-doc admin queue pause` on this document (controller receipt_id=5) stops further queue dispatch, so no more `agent-doc …` triggers get pushed into this session. Resume later with `agent-doc admin queue resume` once the fix lands. I did **not** edit the queue block by hand (that races the live IPC supervisor).

**Two bugs captured (plans written, backlog items added):**

1. **`#qflood` [TOP] — queue dispatch floods the running harness session.** As the queue drains (and during finalize while you type), the continuation dispatcher re-submits the routed trigger into the *already-running* pane, stacking copies you must delete by hand. You confirmed it cross-harness — `lazily-rs.md` under Codex reproduced it — so it's not Codex- or Claude-specific. Likely root cause: the dispatcher doesn't gate on proven pane-idle and doesn't dedupe an already-pending trigger. I did **not** fix it live — it's a concurrency/busy-gate change that needs careful design plus your cross-harness live repro, and fixing it from inside the flooding session is exactly the wrong place. Plan: `tasks/agent-doc/plan-queue-dispatch-flood.md`.
2. **`#jbcompactconflict` — JB `Compact Exchange` / finalize-while-typing raises `File Cache Conflict`.** Same class as the `#q7jm` fix already shipped for `live_prompt_drift` auto-recovery: compact should converge through editor IPC/CRDT when a JB listener is active instead of a direct disk write that diverges from the open buffer. Plan: `tasks/agent-doc/plan-jb-compact-crdt.md`.

**Why I'm not auto-looping or fixing now:** real user prompts arrived mid-loop (these reports), and the active bug is *the dispatch loop itself*. The responsible move is to halt, capture, and hand back. Nothing here is safely agent-fixable without your live repro and a deliberate design pass — I'd rather not change dispatch concurrency code while the dispatch is misfiring.

**State:** queue paused; `#qflood` (top) + `#jbcompactconflict` in backlog with plans; `#2v7b` and `#qt32` still gated awaiting your live proofs; agent-doc submodule clean on `origin/main`. Tell me which to pick up first in a fresh, unpaused turn and I'll start from the plan — or run the gated live proofs and I'll read the logs.
### Re: do #kp5z + do #w42v — investigated; both need a decision/live repro, not a blind fix — opus-4-8

I dug into both before touching code, and the honest finding is that neither is safely fixable blind from inside this (currently IPC-unstable) session. I did **not** ship speculative changes to a deliberately-disabled path or to concurrency-critical dispatch — that risks deepening the exact instability you're hitting. Here's what I found, with the evidence:

**`#w42v` (Compact Exchange File Cache Conflict) — the IPC path is disabled *on purpose*.** Compact writes the whole document to disk (`atomic_write_if_current_pub`, op `compact_exchange_direct_write`), so with the JB buffer open it diverges into the conflict. The full-content editor-IPC path *already exists and was built for this* — `try_ipc_full_content_operator_mutation`, whose source label is literally `compact_exchange` — **but `try_ipc_full_content_with_mode` unconditionally returns `Ok(false)` and logs `reason=disabled_by_default`** (there's even a test pinning that it's off). So "make it CRDT-driven" isn't a one-line swap; it means re-implementing + re-enabling that path (ideally a component-scoped `exchange` `op:replace` convergence like the shipped `#q7jm` fix, so your concurrent edits survive the merge) plus the JB-side apply, then live-verifying. **This is a decision for you:** it was disabled deliberately — do you want me to re-enable it the `#q7jm` way? I won't flip a by-design safety-off blind. Plan updated: `tasks/agent-doc/plan-jb-compact-crdt.md`.

**`#kp5z` (queue flood) — confirmed it's the controller `dispatch`, re-fired by live editor events.** Your own error proved it: `dispatch blocked ... failed_stage=queue_paused` — so the pause gate works, and the bug is *what keeps invoking `dispatch`*. You also saw it "auto-start as I was typing," which means file-change/save events (not just `/loop`) drive the re-dispatch. The flood funnels several entry points (route auto-start on file change, idle continuation, `/loop`) into `dispatch` with no pane-busy gate or trigger dedupe. To pin the *exact* offending invocation safely I need one **live ops.log repro** — which path logs a redundant `dispatch` while the pane is mid-turn — because a blind busy-gate could suppress legitimate dispatch or worsen the stall you also reported. Plan updated with this evidence: `tasks/agent-doc/plan-queue-dispatch-flood.md`. This stays an open actionable item — not done.

**Also captured:** your two new sub-symptoms (queue stalls when you type an unrelated item; auto-start while typing) folded into the `#kp5z` plan as repro evidence. And you confirmed **`#2v7b` reproduces on lazily-rs.md under Codex** — that's the bug the shipped Enter-re-submit fix targets; still gated on your live check of `route_submit_resubmit … result=accepted`.

**State / what I need from you:**
- Flood stays **mitigated** — queue paused (holds). Resume anytime with `agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs2.md`, but the flood returns until `#kp5z` lands.
- **`#w42v`:** confirm you want the `#q7jm`-style component-scoped compact-over-IPC re-enable, and I'll implement it with a live-verify gate.
- **`#kp5z`:** one live repro (queue running, capture `ops.log`) so I can pin the dispatch source, then I'll write the busy-gate/dedupe with SimWorld coverage.
- Plans committed (`dbf98dbd7`); agent-doc submodule clean on `origin/main`.

I'm not auto-looping (real prompts from you, queue paused). Tell me the compact direction and/or drop the repro log and I'll move.
### Re: do #kp5z + do #w42v — both shipped per your decisions — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: do [#kp5z]


You confirmed both directions, so I implemented them. Committed `7b6f022e`, pushed to `origin/main`, binaries + cdylib reinstalled.

**`#w42v` (Compact File Cache Conflict) — fixed, gated for your live check.** `compact::apply_compacted_document` (the single replacement boundary for all compact paths) now calls `write::try_compact_editor_converge` first: when a JB/VS Code listener is active it converges the compacted content through **component `op:replace` patches** (the `#q7jm` way — never `fullContent`) and verifies the editor ack matches the compacted target, logging `compact_writeback transport=editor_ipc`. The guarded direct write stays the fail-safe (no listener / no delta / no-or-mismatched ack / send failure → `transport=disk_fallback reason=…`), so compaction never silently drops content. The spec mandated guarded-direct-write for compact, so I updated `07-closeout-commands.md` too; the `fullContent` prohibition stays intact (I use component patches, which the spec *prefers*). `make check` green; new test `try_compact_editor_converge_falls_back_to_disk_without_listener`. **Gated** awaiting your live JB check: `Compact Exchange` with the buffer open should no longer raise `File Cache Conflict`; `ops.log` shows `compact_writeback transport=editor_ipc`.

**`#kp5z` (queue flood) — dispatch-source logging shipped (your chosen first step).** `project_controller::authorize_dispatch` (the single production funnel every continuation dispatch passes through) now logs `queue_dispatch_invoked file=… pane=… generation=… command_kind=… payload=…` on every call — pure observability, no behavior change. **Next:** resume the queue briefly to reproduce, and I'll read `ops.log` to pin which `command_kind` re-fires `dispatch` while the pane is mid-turn, then write the pane-busy/dedupe gate. Stays the top actionable item.

**Captured your new requests** (all big, all IPC-class — not safe to attempt in this paused/unstable session):
- **`#ipcdrift`** — you report drift *continues* after the single-Project-Pane-Process / 08b migration that was supposed to root-fix it. Identify the races, fix the root cause.
- **`#stuckhandoff`** — wedged `controller serve --handoff-state preparing` procs survive an IDE restart and fight the buffer (multi-owner race on `ipc.sock`). Your embedded diagnosis (kill wedged → respawn clean) is captured; the durable ask is binary self-mitigation (auto-reap wedged `preparing` controllers).
- **`#lazilyup`** — upgrade lazily-rs (the build already resolved `lazily 0.10.3`; I left that Cargo.lock bump unstaged as out-of-scope for this commit).

**Honest flag:** `#ipcdrift` / `#stuckhandoff` mean the 08b migration did **not** fully close the drift — these deserve a dedicated, stable (non-paused, fresh-binary) turn, not a blind change while the IPC is actively misbehaving. Recommend recycling the wedged controllers first (`make install` then `pkill -f 'agent-doc controller serve --project-root …'`) so the next turn runs on clean controllers + the fresh binary.

**State:** queue still paused (flood mitigated); `7b6f022e` on `origin/main`; review has `#2v7b`, `#qt32`, `#w42v` (all operator-drive live proofs). Not auto-looping — paused queue, new heavy requests, and live verification needed.
<!-- agent:boundary:badc5886 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: do [#kp5z]~~
- The IPC drift has continued occurring since we moved into the single Project Pane Process. Please identify race conditions and fix the IPC drift.
- :pushpin: I typed in an unrelated queue item into this queue and the queue stalled.
- Upgrade lazily-rs + tsift-* dependencies.
---
:pushpin: After running JB `Run Agent Doc` to restart the queue, an error occurred. The queue did auto-start as I was typing in this prompt:
```
[route] target tmux session: 13
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=operator-reported flooding: queue dispatch submitting agent-doc triggers into running Claude Code session during finalize/queue-run (cross-harness, repro'd on lazily-rs Codex); halting auto-drain pending fix receipt_id=3263
```
---
What is the root cause of the "stuck-handoff"? In addition to fixing the root cause, how can the agent-doc binary system mitigate against "stuck-handoff" issues?
```
❯ How do I recover from the IPC-drift? Restart Idea?
● Good question — and the short answer is: restarting IDEA is usually not the fix, and in this session it wouldn't have
  helped at all. Let me show you why with the live state.
● Bash(cd /home/brian/work/btakita/agent-loop/src/boost-client
      echo "=== who holds the sockets right now ==="…)
  ⎿  === who holds the sockets right now ===
     --- .agent-doc/ipc.sock ---
     COMMAND     PID  USER  FD   TYPE             DEVICE SIZE/OFF       NODE NAME
     … +86 lines (ctrl+o to expand)
● Now I have ground truth (my earlier ps | grep idea missed it — RTK/sandbox filtering). The picture is clearer and
  more useful than "restart IDEA":
  Two writers are fighting over your doc:
  1. IDEA holds ipc.sock in LISTEN mode — PID 2144231, FD 325u. It acks its editor buffer as authoritative, which is
  how content_ours keeps winning.
  2. Two agent-doc controller serve procs are wedged in --handoff-state preparing — boost-client PID 3767411 and
  superproject PID 3691684. That's a crashed/interrupted controller handoff; those stuck controllers are the active IPC
  writers reverting your edits. (The /tmp/adrepro7 one is an unrelated test repo — ignore.)
  So restarting IDEA is the blunt instrument, not the right first move — it releases ipc.sock, but the wedged
  controllers are separate processes parented to init that survive an IDE restart, so a restart alone won't fix the
  re-winning.
  Let me clear the wedged controllers now (they're lazy-launched and respawn clean on the next agent-doc invoke):
● Bash(cd /home/brian/work/btakita/agent-loop/src/boost-client
      kill 3767411 3691684 2>&1 && echo "killed wedged handoff controllers 3767411 (boost-client) + 3691684 (a…)
  ⎿  killed wedged handoff controllers 3767411 (boost-client) + 3691684 (agent-loop)
     === remaining controllers ===
     brian    3207031  0.0  0.0 162280 11812 ?        S<l  Jun06   1:54 /home/brian/.cargo/bin/agent-doc controlle
     … +8 lines (ctrl+o to expand)
     … +8 lines (ctrl+o to expand)
● Cleared. The two wedged controllers are gone and clean ones respawned without the --handoff-state preparing flag
  (3845157 agent-loop, 3845251 boost-client). IDEA still holds ipc.sock — that's normal and expected while the doc is
  open; it's no longer fighting a stuck handoff.
  So here's the actual answer to your question:
  No — don't restart IDEA (it's the blunt instrument)
  The drift this session wasn't caused by IDEA per se — it was two agent-doc controller serve procs wedged mid-handoff
  (--handoff-state preparing), which survive an IDE restart anyway. I just killed them; the problem is cleared.
  What causes IPC drift
  A multi-owner race on .agent-doc/ipc.sock:
  - IDEA holds the socket (LISTEN) and acks its editor buffer (content_ours) as the source of truth.
  - A crashed/interrupted controller handoff leaves controllers stuck in preparing; those keep writing the stale buffer
  back over your edits → live_prompt_drift_after_preflight, postcommit_worktree_check match=false.
  Recovery ladder — least → most invasive
  1. Working tree only (the common case): finalize usually commits the correct thing to HEAD; only the on-disk copy
  drifts. Verify with git show HEAD:<file> | grep -c '<Re: topic>' → if 1, just git checkout HEAD -- <file> then
  agent-doc session-check. One command. (This is exactly what fixed today's cycle.)
  2. Wedged controllers (what I did): kill the controller serve … --handoff-state preparing PIDs — they're lazy and
  respawn clean on next invoke.
  3. IDEA fighting the buffer: close or revert the doc tab in IDEA (File → Revert, or close the
  tasks/monsterrodholders.md tab). This releases its claim on the buffer — don't kill the whole IDE.
  4. Restart IDEA: only if 1–3 don't release ipc.sock. Overkill, and it won't clear wedged controllers.
  The durable fix (recommended)
  Preflight flagged a stale install: your ~/.cargo/bin agent-doc cdylib predates source commit e6f7538e. Wedged
  preparing handoffs are exactly the bug class a stale binary produces — the controllers I just respawned are still on
  the old binary. Rebuild and recycle them:
  cd /home/brian/work/btakita/agent-loop/src/agent-doc && make install
  # then recycle so they respawn on the fresh binary:
  pkill -f 'agent-doc controller serve --project-root /home/brian/work/btakita/agent-loop'
  Want me to run the make install rebuild now?
```
---
- :pushpin: The lazily-rs.md session running Codex reproduced not submitting the prompt into Codex.
- :round_pushpin: do [#w42v]
- JB `Compact Exchange` should be CRDT driven...I'm getting a `File Cache Conflict`.
- As the queue runs, I'm getting `agent-doc tasks/agent-doc/agent-doc-bugs2.md` prompts submitted into the running Claude Code session. I have to manually delete the prompt from the Claude Code session queue.
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
- [ ] [#sjwm] #ipcdrift IPC drift (live_prompt_drift_after_preflight + postcommit_worktree_check mismatch) CONTINUES after the single Project-Pane-Process / 08b migration that was supposed to root-fix it. Identify the race conditions and fix the root cause. Tightly related to #stuckhandoff. Operator request 2026-06-11.
- [ ] [#kqr6] #stuckhandoff Root-cause + binary self-mitigation: agent-doc controller serve procs wedge in --handoff-state preparing, survive an IDE restart, and keep writing the stale buffer back over edits (multi-owner race on .agent-doc/ipc.sock vs the IDEA listener). Operator diagnosis in queue: kill wedged preparing controllers + they respawn clean. Mitigation idea: binary auto-reaps wedged preparing controllers and self-heals on next invoke. Operator request 2026-06-11.
- [ ] [#nzmc] #lazilyup Upgrade lazily-rs (tsift is upgrading; build already resolves lazily 0.10.3 in Cargo.lock). Verify build + commit the lock bump. Operator request 2026-06-11.
- [ ] [#kp5z] #qflood [TOP] Queue dispatch floods the running harness session with repeated agent-doc trigger submissions while the queue drains / during finalize-while-typing. OBSERVABILITY SHIPPED (commit 7b6f022e): project_controller::authorize_dispatch now logs queue_dispatch_invoked file=.. pane=.. generation=.. command_kind=.. payload=.. on every dispatch (no behavior change). NEXT: operator runs one live repro, agent reads ops.log to pin which command_kind floods while pane is mid-turn, then writes the pane-busy/dedupe gate + SimWorld coverage. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#w42v] #jbcompactconflict JB Compact Exchange writes the full doc to disk (atomic_write_if_current_pub, compact_exchange_direct_write), diverging from the open JB buffer = File Cache Conflict. The full-content editor-IPC path for compact (try_ipc_full_content_operator_mutation, source label compact_exchange) EXISTS but is HARD-DISABLED by design (always returns Ok(false), reason=disabled_by_default). Making it CRDT-driven = re-implement+re-enable that path with a component-scoped exchange op:replace convergence (mirror #q7jm) + JB-side apply + live verify. NEEDS OPERATOR DIRECTION before implementation (it was disabled deliberately). Plan: tasks/agent-doc/plan-jb-compact-crdt.md.
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit JB `Run Agent Doc` on Codex enters the routed prompt but does not submit it (operator must press Enter manually). Root cause: Codex direct-pane dispatch in route.rs::send_command_unchecked detects prompt_not_submitted (TriggerStillVisible) but has no re-submit fallback; the merged text+`\r` send-keys payload is not treated as submit by the Codex TUI. Fix: Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging + SimWorld coverage. Plan: tasks/agent-doc/plan-jb-codex-submit.md. Live verify is operator-drive (check ops.log for route_submit_resubmit action=enter_key result=accepted).
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
