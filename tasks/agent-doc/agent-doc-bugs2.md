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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260529-225736.md`*

Compacted content:
- Archived 6 response topic(s): do [#jb-stale-busy-idle-footer]; [#ipcfullprompt-recur2] reproduced at brandon; Was this a plugin issue?; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: queue auto enabled — opus-4-8

Auto-loop activated (`agent:queue auto`). Queue head is now `#jb-load-tmux-window-verify`, followed by `#queue-head-consume-on-topic-id-regression` and 7 more.

Starting the loop. Note `#jb-load-tmux-window-verify` is a live IntelliJ task (install the signed plugin zip, confirm the Load Tmux Window action autostarts a focused doc's pane) — that can't be driven from a headless session, so when the loop reaches it I'll gate it for live verification rather than falsely mark it done, then continue to the actionable code items behind it.

### Re: do [#jb-load-tmux-window-verify] — opus-4-8

Gated, not done — this is a live-GUI verification task and I can't drive a running IntelliJ from a headless session.

What I verified statically (grounds the gate):
- `LoadTmuxWindowAction.kt` exists, is enable-gated to `.md` docs, registered as `AgentDoc.LoadTmuxWindow` in the popup, and on invoke runs the proven autostart path `SyncLayoutAction.syncLayout(notify=true, noAutostart=false)` → `route::auto_start`. By design, plain doc-open fires only the passive `sync --no-autostart` (no pane growth behind the user), so this action is the intended explicit on-demand loader for a focused doc whose pane is not up.
- Unit coverage present and sound by inspection: `LoadTmuxWindowActionTest` asserts the md-only enable gate and popup registration. Signed zips built through `agent-doc-jetbrains-0.2.137-signed.zip`.

What still needs you (the live part): install `agent-doc-jetbrains-0.2.137-signed.zip`, open a `.md` doc whose tmux pane is NOT up (e.g. corky.md), invoke Load Tmux Window, and confirm it spawns/attaches the pane and starts the harness. If it fails to spawn in that case, capture route/sync logs and file a separate autostart bug.

Keeping `#jb-load-tmux-window-verify` open + gated awaiting that live confirmation.

### Re: do [#queue-head-consume-on-topic-id-regression] — opus-4-8

Fixed and committed. Root cause: `#queue-strike-on-halt` (submodule `baa44a37`) over-narrowed queue-head consumption — it removed the heading-based consumption path from the finalize/`write` flow **and** collapsed `response_topic_matches_queue_head` to exact full-text only. That broke consumption whenever a response heading targets the head's id exactly.

**Fix (submodule `7541af22`):**
- **finalize/`write` path** — new `response_targets_synthetic_queue_head_id`: a *synthetic/preset* head (a prompt carrying a trailing `#preset` id, not a bare `do [#id]` directive) is consumed when the response heading resolves to **exactly** the head id (e.g. `### Re: #spec-test-build-install-commit-push`). Bare `do [#id]` heads still require an explicit `--done`/`--pending-gate`/`--pending-edit` flag, so strike-on-halt is preserved.
- **Codex Stop-hook path** — `response_topic_matches_queue_head` now also matches a topic that resolves to exactly the head id (`### Re: #fix1` for head `do #fix1`), not only exact full text. Trailing modifiers (`#id halt`/`#id deferred`) still never match.

**Verification:** both previously-RED tests now pass — `finalize_consumes_synthetic_queue_prompt_when_response_topic_targets_head_id` and `codex_hook::stop_auto_closes_open_cycle_then_blocks_for_next_auto_queue_head` (the latter was the same regression in the Codex path, also red at clean HEAD). Added unit coverage for the bare-do-directive detector and exact-id resolver. `make check` green (clippy + full suite). Both binaries rebuilt+installed. SPEC.md + specs/07-orchestration-commands.md aligned. Submodule pushed; superproject pointer bumped (`33672c0b6`). CI run `26667229199` triggered (prior two were RED on this regression) — watching for green.

### Re: do [#route-busy-vs-starting-wording] — opus-4-8

Fixed and committed (submodule `6b5a6b4d`). The authoritative-actor `FailClosed` reroute error always read `(waited Ns for <harness> startup)`, which mis-reads a pane that is **busy on an active turn** (e.g. a live Claude turn showing the working spinner / interrupt hint) as a stuck cold start.

**Fix:** the `FailClosed` branch in `route_via_authoritative_actor` now probes the live pane (best-effort `capture_pane`) for a harness busy cue via `HarnessConfig::dispatch_blocker_reason` — the same detector that already distinguishes an active Claude/Codex/OpenCode turn from idle. When a cue is present it words the wait context as `the pane is busy on an active <harness> turn (<cue>), not cold-starting`; otherwise it keeps the `waited Ns for <harness> startup` phrasing. **Behavior is unchanged** — the route still fails closed; only the diagnostic wording differs (UX-only, as scoped).

Extracted `failclosed_wait_context` as a pure helper with unit coverage for both branches. `make check` green, both binaries rebuilt+installed, submodule pushed, superproject pointer bumped (`a992ae098`). No spec change — the fail-closed contract is unchanged and the wording isn't specced at that granularity.

(Persistence note: the first finalize hit a transient unresponsive IPC listener + a ~25-min-stale live-buffer record; recovered by resetting the partial state, clearing the stale record, and re-finalizing via `--force-disk`.)

### Re: do [#codex-queue-head-exact-heading-guidance] — opus-4-8

Guidance shipped (submodule `65ea6ee4`); **gated**, not done, because the item also asks to live-verify a Codex auto-queue advances across items — that needs a live Codex session I can't drive headless.

**What shipped:** a new `runbooks/harness-invocation.md` Codex bullet — **Auto-queue heading discipline**. When a Codex auto-loop turn completes the active `agent:queue auto` head, the Stop-hook auto-consume fires only on an explicit completion signal: either pass `--done`/`--pending-gate`/`--pending-edit` naming the head id, or title the response heading **exactly** with the queue head (`### Re: do [#id]` for a do-directive head, or `### Re: #id` for a preset/synthetic head — the exact-id consumption I restored in `#queue-head-consume-on-topic-id-regression`). A descriptive `### Re: <topic>` heading does **not** auto-consume, so the queue stalls and the hook fails closed on the next non-advancing repeat; trailing modifiers (`#id halt`/`#id deferred`) never consume (`#queue-strike-on-halt`).

`make check` green, binary rebuilt+installed, installed Claude runbook synced, submodule pushed, superproject pointer bumped (`a03d8a768`).

**Still gated on:** a live Codex auto-queue run confirming a descriptive-heading completion now advances when the agent follows the new guidance (or pass `--done`). Headless can't exercise the Codex Stop hook end-to-end.
<!-- agent:boundary:9e3a9e7d -->
<!-- /agent:exchange --> 
###
<!--
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

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#jb-load-tmux-window-verify]~
- ~do [#queue-head-consume-on-topic-id-regression]~
- ~do [#route-busy-vs-starting-wording]~
- ~do [#codex-queue-head-exact-heading-guidance]~
- do [#backlog-auto-marker-misfire]
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [ ] [#codex-queue-head-exact-heading-guidance] [recommended] After #queue-strike-on-halt narrowed the Codex Stop-hook heading-match to exact-topic only, a Codex auto-loop completion with a descriptive heading (### Re: <descriptive topic>) no longer auto-consumes the queue head — only an exact ### Re: do [#id] heading does. Add SKILL.md / harness-invocation guidance telling Codex auto-loop completions to title the response heading exactly with the queue head prompt (or emit an explicit done signal), and live-verify a Codex auto-queue still advances across items. Plan: tasks/agent-doc/plan-queue-strike-on-halt-response.md.
- [ ] [#backlog-auto-marker-misfire] The 'auto' attribute on the agent:backlog marker (queue-only attribute) misfired an auto-run that destructively rewrote the backlog — minted new IDs wrapping 'do [#...]' stubs of already-done items; user reported original task text lost and recovery needed git/sidecar archaeology. Fix: reject 'auto'/unknown attrs on agent:backlog (warn, no mutation); auto-loop must trigger only from the queue 'auto' attribute; never rewrite committed backlog item text in an auto-run. Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md. DO NOT IMPLEMENT.
- [ ] [#pending-add-append-not-prepend] `--pending-add` appends new items below the previous CLI arg within the same finalize cycle, contradicting SKILL.md "Pending capture rule" ("Put new items at the beginning of agent:backlog"). Repro: tasks/agent-doc/open-code.md commit f94e7851c — agent added wave-4, wave-3, wave-2, wave-1 expecting wave-1 on top per SKILL.md; actual backlog was wave-4 on top (pure CLI-arg order). User flagged it as bug; agent had to issue --pending-reorder. Plan: tasks/agent-doc/plan-pending-add-prepend-semantics.md (recommend Option A — strict prepend, drop the implicit adjacency heuristic).
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
- [ ] [#jb-busy-queue-ux-feedback] [recommended] When the authoritative actor is genuinely busy, JB Run Agent Doc correctly queues the dispatch in agent:queue auto instead of injecting a duplicate trigger (monsterrodholders.md, equityfundingsource.md reports), but the plugin surfaces the route/resync log dump as an error. Present queued-behind-a-busy-turn as an informational notification distinct from real failures so expected busy-queuing does not read as a plugin error.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
