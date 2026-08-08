---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 1bb0329b-f54a-4fb0-9f50-42a4d59b934f
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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-173725.md`*

Compacted content:
- Archived 10 response topic(s): queuemirrororder; watchsighupwin; loopsuperprojectjob; 7 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): do [#runsubmitclaude]; closing out #runsubmitclaude; restoring...

### Re: fresh-Claude restart startup-miss, submit lag, laggy pane sync — opus-5

> **Queue prompt:**
>
> On monsterrodholders.md, I changed the agent from opencode to claude. On Restart Agent, an error occurred in the tmux pane `❯ [agent-doc] session did not become dispatch-ready in 30s (no_prompt) for claude; recorded startup-miss and failing closed instead of hanging. Run 'agent-doc start /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md' to retry.boost-client main brian@cachyos-x8664`


> **Queue prompt:** On monsterrodholders.md, I changed the agent from opencode to claude. On Restart Agent, an error occurred in the tmux pane `❯ [agent-doc] session did not become dispatch-ready in 30s (no_prompt) for claude; recorded startup-miss and failing closed instead of hanging.`

One root cause explains most of this, and it is now fixed and shipped in **0.35.176**.

**Claude's status line loses its `ctx:N%` token on a fresh session.** `is_claude_status_chrome_line` keyed the entire chrome test on that token:

- established: `Opus 5 ctx:20% ~/…/src/boost-client main brian@cachyos-x8664` → chrome
- **fresh: `Opus 5 ~/…/src/boost-client main brian@cachyos-x8664` → not chrome**

Because that line renders *below* the composer, it won `last_prompt_candidate` and masked the `❯` prompt above it. Every non-cursor-scoped readiness check — `output_prompt_visible`, and through it `current_child_prompt_visible` and `idle_queue_prompt_visibility` — then reported a genuinely idle, dispatch-ready pane as never-ready, until the session accrued enough context for `ctx:N%` to appear.

That is exactly why it only bites on a **fresh spawn**. Restart Agent with an `agent:` change takes the `agent_restart_performed … action=spawn_fresh_harness` path, so context starts at zero every time. Your own log shows all three timeouts were harness switches (codex→opencode, opencode→claude), never an established session. It also explains the tail of the text you pasted: `…to retry.boost-client main brian@cachyos-x8664` is that very status line.

It answers your other two symptoms too. `readiness_source` in your session log is `stable_owned_pane` on **every** successful trigger and never `current_child_pty` — the child-PTY signal was permanently false. **Submit lag** is route burning its readiness-wait before falling through to the cursor-scoped path, and **laggy pane auto-sync** is the idle-queue watch failing to confirm an idle prompt via the same `output_prompt_visible` call.

The chrome test now matches the ctx-less form structurally — trailing `user@host` plus a filesystem-path token, never a `❯`/`⏵` row — so ordinary prose like `ssh brian@host` is untouched. Tests use pane bytes captured live from a real fresh Claude session (red before, green after).

**What I could not prove.** Your `%915` is only **9 rows**, so the status line is scrolled off entirely and this fix cannot be the whole story for that specific 30s miss. The pane fallback needs `current_child_output` ∧ `actor_ready` ∧ `owned_pane_dispatch_ready`; your log shows `actor_ready` at 17:34:03, and I measured a fresh Claude reaching a bare `❯` at the cursor row in 5s in that exact geometry. So one of the other two gates stayed shut, and the log could not say which — `auto_trigger_timeout` recorded only `reason=no_prompt_after_30s`, which is identical for a dead PTY relay, a stuck actor projection, and an unrecognized prompt.

I fixed that blind spot rather than guessing: the timeout now logs every gate plus `blocking_gate=`. The next occurrence names its own cause instead of costing a forensic session. That item is at the top of the backlog.

Measured along the way: Claude reaches its composer in **2.5s** here, so the 30s deadline was never too short.

**Shipped:** `make check` green, `make install` → 0.35.176, supervisors recycled, pushed (submodule + superproject). CI run `31271290023` was still in progress at closeout — recorded, not awaited.

### Re: prompt injection into running turns, flood, and IDEA responsiveness — opus-5

Your three reports share one root cause, now fixed and shipped in **0.35.177** + **0.35.178**.

**The idle-queue watch was injecting into an occupied composer.** `idle_queue_prompt_visibility` guarded only against an *active* turn via `dispatch_blocker_reason`, then short-circuited on `actor_ready`. A completed-turn marker (`✻ Brewed for 7m 59s`) raises no busy cue, so `actor_ready` won and returned `Visible` with nothing inspecting the composer — which held your `keep going`. `dispatch_payload_pending_in_current_input` did not catch it either: it asks whether *this payload* is pending, not whether the composer is occupied at all.

**The sdk.md flood was the same bug in a second shape.** Once several `/agent-doc <FILE>` lines pile up, the bottom-most carries no `❯` glyph, so `is_prompt_line` is false and the projection is `Absent`, not `OperatorDraft`. The watch appended another copy and pressed Enter — which in a multi-line composer inserts a newline instead of submitting, so the draft grew every ~30s (`dispatch_start_unproven` on a loop).

The fix could not simply require an empty composer: a bare `Absent` is a legitimate stale renderer tail where a ready actor must still win (`idle_queue_prompt_visibility_trusts_ready_actor_over_stale_renderer_tail` pins that, and I broke it on the first attempt). So the guard discriminates by composer **chrome** — when the frame is rendered but the body is not an empty prompt, the composer is occupied and is never injected into. Both regression tests use pane text captured live from your sessions.

### Re: is the plugin using the main CPU thread? — opus-5

**No.** Measured across every thread in the IDEA process (pid 702745):

| | share of process CPU |
|---|---|
| G1 GC threads | **41.5%** |
| JIT compiler | 1.7% |
| **EDT / AWT** | **0.2%** |
| other | 56.6% |

The EDT is essentially idle, so this is not a blocked UI thread — it is a **GC storm**. 41.5% of the process's entire CPU budget is garbage collection, which stalls the UI through allocation pressure and stop-the-world pauses rather than through EDT blocking. `kswapd0` is also active, so there is system-wide memory pressure.

**I contributed to this and should have known better.** I ran `make install` three times this cycle, and each run drops the JetBrains plugin into 4 running IDE installations ("Restart running IDEs to activate"). There is a recorded note that installing a plugin into a *running* IDE causes exactly this replica storm and that only an IDE restart clears it. The release checklist says install after every fix; I should have skipped the editor-plugin step while your sessions were live. **Restarting IDEA is the fix** — I have not run `make install` again.

**Also fixed earlier this cycle:** `#freshclaudectxless` (0.35.176) — Claude's status line drops its `ctx:N%` token on a fresh session, so the ctx-less line masked the `❯` composer and every non-cursor-scoped readiness check read an idle pane as never-ready. That is what caused the 30s `no_prompt` startup-miss on Restart Agent, the submit lag, and the laggy pane sync.

**Still open.** `#startupmiss9row` is unfinished — I attempted a live reproduction but `sync` normalises the pane to full height, so I could not recreate the 9-row JB geometry; your reports took priority. `#clearsubmitlabel` is newly filed: your Clear Session Context error reports `result=command_still_visible` while `command_visible=false`, because `TimedOut` conflates "still visible at the deadline" with "never observed" — the label is actively misleading, and the acceptance rule needs a decision I did not want to loosen unilaterally.

**Shipped:** `make check` green at each step, installed, pushed (`2d04e182a`, `5d233700f`), controllers recycled. The haiven-dev compact/delivery barrier cleared with `admin reload-lib`.
<!-- agent:boundary:22e8b690:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#clearsubmitlabel]
- 🚧 do [#startupmiss9row]
- ⏭️ do [#ptsubmitmetric]
- do [#claudesplitsubmit]
- do [#compactstrandedheading]
- do [#hvw0publishproof]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#clearsubmitlabel] session_clear reports result=command_still_visible while command_visible=false; TimedOut conflates 'still visible at deadline' with 'never observed'
- [ ] 🚧 [#startupmiss9row] Reproduce the 30s no_prompt startup-miss in a 9-row JB-layout pane where the status line is off-screen; use the new blocking_gate= diagnostic to attribute it (dead PTY relay vs stuck actor projection vs pane detector)
- [ ] [#ptsubmitmetric] #agent-doc-bug Fix the unmeasurable #passthroughsplitprofile criterion. `outcome=enter_resubmit` is a NON-terminal action (`pass_through_stranded_draft_action_is_terminal` admits only Cleared/DeferredPaneBusy/ExhaustedStillStranded), so production ops.log can never record it and that counter is structurally always zero no matter how many submits drop. The real signal is `enters_sent > 0` on a terminal `outcome=cleared` line — `enters_sent += 1` happens only inside the EnterResubmit branch (agent-doc-controller/src/dispatch.rs:6070). Emit an explicit terminal field (for example `resubmit_required=true|false`) or a rollup command so the decision is readable without hand-parsing ops.log, restate the unblock criterion in those terms on #passthroughsplitprofile, and add a regression test pinning that a repair which resubmitted logs the new field.
- [ ] [#claudesplitsubmit] #agent-doc-bug Decide and implement whether claude joins `harness_uses_split_text_submit` (the split text+Enter profile codex/opencode use, `tmux_submit_profile_for_harness`, 80ms), reading the corrected counter from #ptsubmitmetric. Current reading: 6 post-0.35.164 harness=claude repairs (elapsed_ms >= 150), 5 of which needed at least one resubmit and one of which needed two — that is the stated single-call-send-is-the-cause condition, but on 6 samples rather than the ~20 sample floor. Gather to the floor with the corrected metric first, then either add claude to the split profile or record that the settle-gated repair alone is sufficient. Do not change the profile without the measurement.
- [ ] [#compactstrandedheading] #agent-doc-bug Run `agent-doc compact exchange tasks/agent-doc/agent-doc-bugs2.md` and confirm it no longer fails on the historical stranded duplicate heading. Converted from the removed operator-verify item #operatorverifyresolve, whose premise (a live JetBrains File Cache Conflict awaiting a human Cancel / Keep-memory-changes) is stale: recent session-checks read `status=current authority=cp_model live_editors=1 delivery_converged=true` and the retained closeouts it was waiting on are committing. Only this half was ever untested, and it is agent-testable.
- [ ] [#hvw0publishproof] #agent-doc-bug Verify statically (read the release workflow on github.com/btakita/skill-harness) that the crates.io publish step propagates failure after 32836ad. The loud-failure behavior has never actually executed: the newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports SUCCESS even though the CARGO_REGISTRY_TOKEN 403s and v0.1.3 was published by hand — that success line is the false green 32836ad was meant to kill. Until a tag runs, the every-tagged-release-now-fails-loudly claim on #hvw0 is untested.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#passthroughsplitprofile] #agent-doc-bug [blocked-on-evidence] Decide from evidence whether Claude also needs the split text+Enter submit profile that codex/opencode use (`tmux_submit_profile_for_harness`, 80ms). CRITERION CORRECTED 2026-08-08: the old unblock condition (a nonzero `outcome=enter_resubmit` rate in ops.log) is UNMEASURABLE — EnterResubmit is a non-terminal action and production logs only terminal outcomes, so the counter is structurally always zero. The load-bearing signal is `enters_sent > 0` on a terminal `outcome=cleared` line (dispatch.rs:6070 increments only in the EnterResubmit branch), which means the draft was still visible after the initial send. Current reading: 6 post-0.35.164 harness=claude lines, 5 with enters_sent>0 (one with 2). UNBLOCKED when the corrected counter (#ptsubmitmetric) reaches ~20 post-0.35.164 harness=claude repairs; #claudesplitsubmit then makes the call. Do not change the submit profile without the measurement — the mechanism is inferred, not proven.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare prompt) still refuses as busy. Fixed in 17848a927. This item named binary 0.35.6 and a pending idle-boundary recycle; the installed binary is now 0.35.174 and has recycled many times, so only the live eyeball remains.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green. JB install step DONE — the installed plugin is 0.2.350 in all four IDE configs, superseding the 0.2.205 zip this item named; only an IDE restart is missing, covered by #stickymdpaneverify. VS Code 0.2.38 install is UNVERIFIED — no agent-doc extension found under ~/.vscode/extensions. REMAINING (operator): restart the IDE, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#agentsignals] #agent-doc-feature [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
