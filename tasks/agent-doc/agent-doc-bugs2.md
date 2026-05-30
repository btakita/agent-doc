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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260530-055632.md`*

Compacted content:
- Archived 3 response topic(s): #codex-auto-queue-stalled-final-gate; JB Run Agent Doc route/resync log paste; do option b
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#bare-write-captured-uncommitted] — opus-4-8

Implemented per `tasks/agent-doc/plan-bare-write-captured-response-outside-commit.md`.

**This cycle also live-verifies `#active-queue-persisted-no-continue` (now `--done`).** The only reason this turn is working on `do [#bare-write-captured-uncommitted]` is that the prior fix's persisted-active queue auto-advanced: preflight synthesized the next head with `queue_trigger: persisted` (no `auto` attr) and `session-check` reported `queue_continuation_required=true`. That is the binary-path continuation the gate was waiting on, observed live in this session. (Scope note: the binary/preflight/session-check continuation is proven live; the Claude Code `/loop` auto-invocation leg remains covered only by the rendered-skill test, since this turn was re-invoked manually.)

**Root cause:** a bare `agent-doc write` (CommitMode::None) that placed/preserved a response body on a session document left the cycle at `response_captured`/`write_applied` and failed `session-check` with no deterministic continuation — forcing a manual `git commit`. `recover_missing_commit_boundary` only repairs responses **already in HEAD**, so a genuinely-uncommitted placed body had no recovery path.

**Fix (submodule `8fd699b1`, pointer `e1b617179`):**
- `write::run_command` — after the write, if a session-document bare write placed a response body, escalate `commit_mode` to `Required` **before** the commit-mode-gated phases (queue consumption, pending gates, commit). IPC `already_applied` proves placement, not closeout, so the next step is still commit. Not in a git repo → fail with a concrete recovery command instead of stranding.
- `bare_write_placed_response_body` detector — two confirmations: (1) cycle open at `response_captured`/`write_applied` (covers both the captured path and the synthetic-cycle stream path with no durable capture), and (2) the working tree carries an assistant response heading HEAD lacks (`detect_bypassed_response_write_between`), so a pending/status-only bare write is never force-committed.
- Reworded the no-response bare-write closeout message — that branch now only handles pre-existing open cycles.

**Tests / verification:**
- `bare_write_stream_on_session_doc_escalates_to_commit_when_response_placed` (was `*_fails_closed_*`) — bare write now succeeds, commits to HEAD, reaches `committed`, session-check clean.
- The three `write_commit_empty_stdin_*` adoption tests reworked: they used a bare write as a setup shortcut to strand a response; that shortcut no longer strands, so they now stage the IPC-patchback precondition directly via `place_uncommitted_visible_response`. The adoption path they guard (`write --commit` empty-stdin adopts a visible uncommitted response) is unchanged and still verified.
- FlowCore hot-path `reason=` budget for write.rs audited 25 → 26.
- `make check` exit 0 and a direct `cargo test --workspace` both green (RTK masked an interim FlowCore-gate failure on the first run — caught via the authoritative log, fixed the budget, re-verified). Both binaries rebuilt + installed (02:22).

### Re: do [#hj7s] — opus-4-8

You queued `#hj7s` and `#prompt-preempts-auto-queue` and chose **one per cycle**, so this cycle does `#hj7s`; the queue advances to `#prompt-preempts-auto-queue` next. (Preflight correctly halted the auto-continuation with `queue_halted: item_modified` because you edited the head.)

**Approach (from your note):** the editor owning the pane is a *legitimate* state — Claude Code's `ctrl+g` edit-in-nvim. So the fix **refuses with a diagnostic and lets the operator close the editor manually** — it does not force-quit it (unlike interrupt-clear's `:qa!`) or SIGINT around it, and `--force` does not bypass it. Plan: `tasks/agent-doc/plan-hj7s-restart-editor-foreground-guard.md`.

**Rust core (the actual bug fix — submodule `6544cf53`, pointer `011c6f81a`):**
- `prepare_restart_live_busy_pane` now builds `live_pane_evidence` once and, **before** the force/non-force branch, refuses via `anyhow::bail!` when `terminal_editor_command` matches the pane's `current_command` (logs `session_restart_editor_holds_pane_refused`). Because it runs before the force branch, both `Restart` and `Interrupt and Restart` refuse cleanly — no more `C-c`/restart-typing into the editor buffer.
- `restart_editor_holds_pane_refusal_message`: parseable header `session_restart refused for <file> because pane <pane> is held by editor <cmd>` + `source=`/`current_command=`/`tail=` fields + close-editor guidance (`:wq`), mirroring the busy/starting refusal convention.

**JetBrains surfacing (`TerminalUtil.kt`, plugin `0.2.139`):**
- `EditorHoldsPaneRestartRefusal` + `parseEditorHoldsPaneRestartRefusal` + `notifyEditorHoldsPaneRestartBlocked` (no "Interrupt and restart" action — `--force` won't bypass).
- `runRestartSupervisorCommand` `onFailure` now runs for the force path too (it was `null`, so Interrupt-and-Restart refusals were **silent**); the editor refusal is checked first. Signed zip built.

**Verification:** Rust `restart_editor_holds_pane_refusal_names_editor_and_close_guidance` + Kotlin parse/message tests; `make check` green (exit 0, no FAILED; FlowCore gate ok — `session_actor_cmd.rs` is not a budgeted hot-path file); JB `gradle test buildPlugin signPlugin` green (160 tests). Both Rust binaries rebuilt + installed (10:10).

**Gated, not done:** the Rust guard is fully unit-tested and live in the binaries, but the JB notification needs live IDE verification (install signed `0.2.139`, open `ctrl+g` nvim in a Claude Code pane, click Interrupt-and-Restart, confirm the refusal notification appears and no keys reach the editor). Gated on that.

**Follow-up:** `route.rs` busy-interrupt (`route.rs` ~6427) sends a raw `C-c` into the pane foreground in the reroute path — same shape, but a hard refusal there would change reroute semantics, so it's tracked separately rather than forced into this cycle.
❯ I was able to successfully run JB `Restart Supervisor Process` on a Claude & Codex session. Successful.

### Re: JB Restart Supervisor live-verify — opus-4-8

That's the live verification `#hj7s` was gated on — resolving it to **done**.

Scope of what your test confirms: running `Restart Supervisor Process` successfully on live Claude **and** Codex sessions proves the new guard **doesn't regress the normal restart path** (the main risk of adding an evidence/foreground check before the interrupt — restart still reaches the supervisor and succeeds). The editor-refusal branch itself (nvim holding the pane → refusal + JB notification, no keys into the buffer) is covered by the Rust unit test and the Kotlin parse/message tests rather than this run, since a normal session has no editor foreground. If the `ctrl+g` edit-in-nvim refusal ever misbehaves in practice, just flag it and I'll reopen.

Gate cleared on submodule `6544cf53` / pointer `011c6f81a`, plugin `0.2.139`.

**Queue:** the gated `do [#hj7s]` head was auto-struck this preflight (`source=review_gated`) and the queue is now **inactive** (`inactive_queue_residue`). `do [#prompt-preempts-auto-queue]` is still listed but won't auto-run until the queue is reactivated — re-add `auto` (or trigger it) when you want me to pick it up next, per one-per-cycle.
JB `Clear Session Context` failed on a Codex session. Using `Interrupt and Clear` opened nvim on the Codex session.
```
Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md because pane %28 contains protected prompt input (reason=drafted prompt input, source=authoritative_actor, current_command=agent-doc, tail="gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/boost-client · Context 75% used"). Clear the prompt input manually, or run `agent-doc session interrupt-clear /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` to intentionally interrupt the pane and clear context.
```
#spec-test-build-install-commit-push

### Re: JB Interrupt and Clear opened nvim on a Codex session — opus-4-8

Root cause found and fixed. Two distinct things were happening; only the second is a bug.

**1. `Clear Session Context` refused — by design, not a false positive.** The refusal fires only when the live Codex composer holds non-placeholder text starting with `›`/`>`/`❯` (empty composers and dim idle placeholders like `› Ask Codex to do anything` return ready, not protected). The `tail=` in the error is the status/footer line (`gpt-5.5 xhigh · … · Context 75% used`), which is *not* what the detector keys on — it's just context for the operator. So `session_clear` correctly protected drafted prompt input and pointed you to `interrupt-clear`. I can't retroactively prove what was in that composer, so I didn't touch the detection; if you ever see this refusal on a *visibly empty* Codex composer, capture the pane and reopen — that would be a real false positive.

**2. `Interrupt and Clear` opened nvim — the actual bug, now fixed.** The Codex operator interrupt sequence sent `C-g` first, unconditionally:

```
C-g  →  Escape  →  C-c
```

`C-g` is only safe in one place: a shell `reverse-i-search` / history-search, where it aborts the search (that's the case the route recovery path and its SimWorld test model). In the **normal Codex TUI composer**, `C-g` opens the external editor (`$EDITOR`, i.e. nvim) — exactly what you saw. The editor-recovery net (`Escape` + `:qa!`) then races to back out, which is why it presented as nvim flashing open rather than a clean interrupt.

**Fix (submodule `3acb93f6`, pointer `911e3fbaf`):**
- `send_operator_interrupt_sequence` now takes a `codex_shell_search` flag. `C-g` is emitted **only** when a live capture proves the pane is in a shell reverse-i-search / history-search state; every other Codex state (idle composer, active turn) gets `Escape` + `C-c`.
- New `codex_pane_in_shell_search_state` captures the pane and checks `dispatch_blocker_reason` for those two shell states before deciding.
- Both callers gated: `interrupt-clear` (your path) **and** `restart-supervisor --force`.
- Ordered keys extracted into the pure `operator_interrupt_key_plan` so the decision is unit-tested directly.

**Spec/tests/build:**
- `specs/07-session-tmux-commands.md` + the AGENTS invariant now document the state-scoped Codex interrupt keys (`#codex-interrupt-clear-ctrl-g-opens-editor`).
- 3 new unit tests: codex-composer omits `C-g`; codex-shell-search includes it; opencode/claude unchanged. `cargo test --workspace` green (4257-line artifact, 0 failures), `make check` exit 0 (verified past the RTK exit-mask via the digest runner).
- Both binaries rebuilt + installed (`cargo build --release` + `cargo install --path .`). This is CLI-binary logic, not the cdylib, so no JB plugin reinstall / `lib-install` is needed — the installed `agent-doc` the plugin shells out to already has the fix.
- CI: prior run on this branch green; my push's run is in flight at closeout.

**Live-verify when convenient:** on a Codex session, click **Interrupt and Clear** and confirm it interrupts + clears without nvim opening.

**Follow-up queued (`#codex-route-busy-ctrl-g-opens-editor`):** the route.rs busy-pane reroute interrupt (~`6409`) shares the same unconditional `C-g` and the same latent editor-open; gated separately because changing it touches reroute semantics.
<!-- agent:boundary:0202fb35 -->
<!-- /agent:exchange --> 
###
<!--
JB `Run Agent Doc` on an idle session. Why did `registered in its own project root` occur? 
```
resync: stash pane %32 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 0.5s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 96 for tasks/software/tsift.md is busy on pane %36; queued pending dispatch "re [#tsift-dormant-crates]: Is there functionality in tsift-tokensave and tsift-algorithms that should be used?" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
---
JB `Run Agent Doc`, then cancel run directly in the session, then JB `Clear Session Context`, then JB `Run Agent Doc`:
```
resync: stash pane %32 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 7.8s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 96 for tasks/software/tsift.md is busy on pane %36; queued pending dispatch "What are #next-steps to extract tsift crates?" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#bare-write-captured-uncommitted]~
- ~do [#hj7s]~
- do [#prompt-preempts-auto-queue]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
- do [#next-steps-backlog-capture]
- do [#ah0s] 
- do [#pending-add-suppresses-queue-consume]
- do [#stuck-capture-compact-false-positive]
- do [#ipcfullprompt-recur2]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#codex-route-busy-ctrl-g-opens-editor] [recommended] route.rs busy-pane reroute interrupt (route.rs ~6409) also sends C-g unconditionally to a busy Codex pane before the Escape+C-c fallback — same root cause as #codex-interrupt-clear-ctrl-g-opens-editor: C-g opens the external editor ($EDITOR/nvim) in the normal Codex composer. Gate it on a shell reverse-i-search/history-search capture like send_operator_interrupt_sequence now does (or skip C-g when the busy blocker is an active turn). Tracked separately from the operator interrupt-clear fix because a hard change here alters reroute semantics. Add a route SimWorld regression for the non-search composer case — the existing ctrl-g test (write_mock_busy_registered_agent_doc_recovers_on_ctrl_g) only models reverse-i-search recovery, which is why this regression slipped through.
- [ ] [#prompt-preempts-auto-queue] [recommended] Unanswered exchange prompts must run before active `agent:queue auto` continuation, even when the prompt was accidentally baselined into the `.agent-doc` snapshot. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `What are #next-steps to complete review items?` was visible after the exchange boundary and duplicated in `agent:queue auto`, but the snapshot already contained the exchange prompt, so the next diff only saw queue bookkeeping and the prompt did not run. Fix: add snapshot-independent unresolved exchange prompt detection, make preflight/direct-run prefer it over queue synthetic diffs, prevent route queue snapshot recovery from absorbing unanswered exchange prompts, and defer Codex/Claude queue continuation while such a prompt exists. Plan: tasks/agent-doc/plan-prompt-preempts-auto-queue.md.
- [ ] [#codex-final-response-not-written] [recommended] Codex final response was not written back into the session document after a manual/direct run on `src/boost-client/tasks/monsterrodholders.md` (reported 2026-05-30 after `#nsga4verify` work). Symptom: commits and pushes completed, but `agent:exchange` lacked the assistant close-out; the user had to request a manual document patch. Plan: reproduce with an owned-pane recursive/direct invocation path where `agent-doc` refuses direct recursion, the agent continues manually, and finalizes outside the normal `agent-doc write` capture; add a session-check/finalization guard that detects a completed turn with no new `agent:exchange` response body, then either forces `agent-doc write --commit` or fails closed with the exact recovery command. Regression should cover Codex final responses after fallback/manual handling and ensure the response body lands before the boundary and is included in the committed doc.
- [ ] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [ ] [#ah0s] [recommended] Add explicit-position --pending-add primitives so ordered backlog batches never depend on implicit prepend-vs-argorder semantics: --pending-add-after <id>, --pending-add-before <id>, --pending-add-back/--pending-append, keeping --pending-add as the front-insert default. Backlog is a priority pool with ID-based consumption (not a stack/queue; FIFO lives in agent:queue), so position should be author-set explicitly when it matters. Two agents (open-code.md #adcr waves f94e7851c; corky.md crate-split phases fc8d9b3bc) both reversed args expecting prepend-flip and got reverse-of-intended order. Pairs with #pending-add-append-not-prepend (that item finalizes the default-order decision; this adds the explicit escape hatches). Plan: tasks/agent-doc/plan-pending-add-explicit-position.md.
- [ ] [#pending-add-suppresses-queue-consume] [recommended] Bundling --pending-add in the same finalize as a queue-head-targeting response suppresses queue-head consumption: finalize logs 'skipped consumption because the active prompt did not target the queue head' and treats the pending-add as the active prompt, even when the response heading exactly matches the head (Re: do #ID form, no brackets). Repro 2026-05-29 agent-doc-bugs2.md declining backlog-auto-marker-misfire with a bundled --pending-add — head stayed unstruck. Fix: heading/queue-head match should drive consumption independently of bundled pending mutations, or consume the head when the response heading matches regardless of detected active prompt.
- [ ] [#stuck-capture-compact-false-positive] [recommended] stuck_captured_cycle false-positive after compaction: a capture recorded as committed (cycle-1780100032048, response 'gate→done for codex guidance') is later archived by compact, so preflight reports committed-but-not-in-HEAD and suggests `write --commit` recovery — which would re-inject an already-archived response (it lives in .agent-doc/archives/<hash>-*.md). Fix: when a captured-committed body is absent from HEAD because it was compacted (present in the matching archive), reconcile the capture instead of flagging it stuck. Repro 2026-05-29 agent-doc-bugs2.md.
- [ ] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
- [ ] [#jb-busy-queue-ux-feedback] [recommended] When the authoritative actor is genuinely busy, JB Run Agent Doc correctly queues the dispatch in agent:queue auto instead of injecting a duplicate trigger (monsterrodholders.md, equityfundingsource.md reports), but the plugin surfaces the route/resync log dump as an error. Present queued-behind-a-busy-turn as an informational notification distinct from real failures so expected busy-queuing does not read as a plugin error.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
- [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
- [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [/] [#jb-stale-busy-idle-footer] Three JB Run Agent Doc / Clear Session Context reports queue-instead-of-dispatch or block Clear while the live pane footer shows a Claude Code idle composer: monsterrodholders.md (actor gen 233, pane %19), equityfundingsource.md (gen 35, pane %3), tsift.md (pane %21, footer 22.4K (11%) ctrl+p commands). The tsift.md case is the strongest stale-busy candidate — Clear was blocked though the footer indicates idle. Fix: route.rs / session_actor_cmd.rs busy projection should treat the Claude Code idle-composer footer (ctrl+p commands / context % used) as idle direct evidence to repair a stale busy actor/lease projection before queuing or blocking, mirroring the existing OpenCode context % used idle-composer rule. Relates to #run-agent-doc-stale-busy-replay and #snrun (plan tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md).
- [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788): live editor-buffer convergence after queue halt/drain. New core fn component::converge_queue_auto + agent_doc_converge_queue_auto FFI (C-int ABI for stable JNA); ipc_socket::send_queue_convergence carries queue_auto opening-tag state + queue_active frontmatter; preflight run_queue_maintenance pushes convergence through an active IPC listener after each halt/drain disk write (best-effort, non-fatal). JB plugin 0.2.137 parses queue_auto and applies via NativePatching.convergeQueueAuto in Document-API + VFS paths. Deterministic SimWorld repro queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt + Kotlin ParsePatchJsonQueueAutoTest; make check green; both binaries built+installed (symbol verified in release cdylib); spec 07-orchestration-commands updated. GATED on live verification: install signed plugin 0.2.137, restart the route-owned agent-doc process to load the new binary, then confirm a real IDE-owned session no longer re-diverges (no recurring agent:queue diff / stuck_captured_cycle / inactive_queue_residue) after a consume->halt cycle without a raw git checkout. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
- [/] [#exchange-prompt-dropped-on-merge] User-authored exchange prompt (observed: "go") silently dropped during an IPC content_ours merge while the IDE held ipc.sock — never reaches snapshot/HEAD, assistant never responds, next preflight reports no_changes. Same drift family as #adoc-queue-ipc-buffer-divergence / #duplicate-response-after-commit but the victim is user input. Repro 2026-05-29 on src/boost-client/tasks/monsterrodholders.md (dropped the "go" authorizing #gfwconnect; assistant replied "nothing changed" for two cycles). Plan: tasks/agent-doc/plan-exchange-prompt-dropped-on-merge.md (recommend Option A — union user prompt lines / fail closed + add dropped_prompt_guard to session-check).
- [/] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing. PARTIAL FIX 1 (v0.34.0, submodule 2a6f60e5): prefix-agnostic append dedup (strip_user_prompt_prefix) for the ❯-prefix mismatch shape. PARTIAL FIX 2 (this cycle): L1 buffer-snapshot typing race — component::append_patch_already_present now recognizes when a synthesized patch carries an EARLIER keystroke-snapshot of the live region (per-line prefix alignment via patch_is_typing_snapshot_of) and no-ops instead of re-appending the divergent tail. Confirmed live in tasks/resume.md 2026-05-29: half-typed '- [#s93y]: Add to' snapshot vs live '...Add to resume' broke the old contains() substring check and re-appended the entire tail (close marker + Queue/Backlog/Done duplicated). Boundary 70bccf9b stayed intact and no new boundary was minted, ruling out the reposition path. Tests: append_patch_dedupes_midblock_typing_snapshot, append_patch_typing_snapshot_does_not_collapse_distinct_lines, append_with_boundary_does_not_duplicate_typing_snapshot. Both binaries built+installed; cdylib lib-installed for JB hot-reload. GATED on live re-verify (type a prompt into resume.md during an in-flight cycle). RESIDUAL: the exact editor-side full-tail concatenation downstream of the dedup miss was not statically traced — the dedup no-op short-circuits the append entirely so it should not surface, but confirm live. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#misfire-recovery-snapshot] Pre-auto-run recovery snapshot/tag (mirroring `compact`'s pre-mutation tag) so a destructive auto-run misfire is recoverable without git/sidecar archaeology. Proposed fix #4 of `#backlog-auto-marker-misfire` (now IMPLEMENTED for fixes #1–#3). Parked: only worth doing if a new auto-run path can mutate the backlog in place — currently the misfire is inert (queue-scoped) and surfaced (misplaced_component_attr warning). Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
