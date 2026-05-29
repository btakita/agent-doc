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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260529-221217.md`*

Compacted content:
- Archived 9 response topic(s): do [#adoc-queue-ipc-buffer-divergence]; do [#queue-strike-on-halt]; do [#jb-stale-busy-idle-footer] halt; 6 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#jb-stale-busy-idle-footer]. #spec-test-build-install-commit-push
### Re: do [#jb-stale-busy-idle-footer] — opus-4-8

Implemented the two-part safe fix from `plan-jb-stale-busy-idle-footer.md` and shipped it through spec → test → build → install → commit → push. A genuinely-idle Claude pane no longer projects `AliveBusy` (queue-instead-of-dispatch / blocked Clear), and — critically — a live turn can never be mis-read as idle.

#### The fix (submodule `faccb329`, pointer `e3b1af769`, both pushed)

Two parts, both required, ordered for safety:

- **Part 1 — Claude busy cue** (`harness.rs` `dispatch_blocker_reason`): scans recent pane lines for the interrupt hint (`esc to interrupt`) or a working spinner with an elapsed-seconds timer (spinner glyph + `…` gerund + `(<N>s`). `live_pane_prompt_ready` checks `has_busy_cue` **first**, so an active turn fails closed before any composer inference.
- **Part 2 — ignorable static Claude chrome** (`harness.rs` `is_ignorable_output_line`): marks only the `ctx:N%` model/context status line ignorable — never the spinner, `❯`, or `⏵⏵`. With the busy cue in place, `last_prompt_candidate` resolves a genuinely-idle pane to the `⏵⏵`/`❯` composer even when the status line is the last meaningful line (the plan's question-1 state).

The ordering is the safety guarantee: Part 1 catches a busy turn before Part 2 can ever surface a composer, so the bare `❯` present in *both* idle and busy panes cannot false-idle a live turn.

#### Why this matches the captures, not a guess

The plan's live captures showed the common idle/busy cases were already distinguished by the `(shift+tab to cycle)` vs `· N shell` permissions-line difference. This change adds the two narrower, provably-safe pieces the plan flagged as still-open: a real busy cue (so "busy" is asserted, not merely "not-ready"), and skipping the static status line so the question-1 state resolves correctly.

#### Verification

- **9 new tests** (all green): `harness.rs` — busy spinner, esc-to-interrupt, idle no-cue, status-chrome-ignorable-but-composer-isn't, status-skip-to-composer, idle-resolves-ready; `session_actor_cmd.rs` — idle composer ready, busy turn **not** ready, status-line-last ready.
- `make check`: clippy clean; the only red is the pre-existing, tracked queue-consumption regression (CI run 26664619093 confirms it's the same single test; tmux integration tests are `ignored`, no tmux-leg regression — this change is pure pane-text classification).
- Built `--release` + `cargo install` + `lib-install`; pushed submodule + superproject pointer + plan.

#### Closeout

Gated, not done: acceptance 1+2 (unit coverage) are met and the fix is live, but acceptance 3 — a **live JB replay** proving a stale-busy-projected idle Claude pane now dispatches instead of enqueuing — is owned by `#run-agent-doc-stale-busy-replay`. The route-error "busy on an active turn" vs "still starting" wording remains `#route-busy-vs-starting-wording`. Reopen `#jb-stale-busy-idle-footer` only if a live replay shows a residual misclassification.

The other pending items referenced above are mentioned for context only; none are completed this cycle.
❯ re: [#ipcfullprompt-recur2]. The issue was reproduced at /home/brian/work/btakita/agent-loop/tasks/professional/brandon-cinquegrana.md
❯ #spec-test-build-install-commit-push

### Re: [#ipcfullprompt-recur2] reproduced at brandon — full-tail scaffold capture — opus-4-8

The brandon reproduction is the captured packet — and it exposed a real gap in my detector, which I've now closed (spec → test → build → install → commit → push).

#### What the reproduction showed

The corruption at `brandon-cinquegrana.md` was the **full-tail duplication** shape: you typed `Is the issue still happening? / Can it be re…`, and the editor copied the entire post-exchange scaffold (`agent:exchange` close + Queue + Backlog + Icebox) **around** your in-progress prompt, leaving **two** exchange close markers with the prompt split across the divergence (`Can it be re` → `Can it be rep11ro`).

#### Why my detector missed it (and the fix)

Last cycle's detector keyed only on deleted/duplicated `### Re:` **response blocks**. This shape duplicates the structural **scaffold**, not a response heading — so it slipped through, and no `ipcfullprompt_corruption_suspected` line fired.

Fixed: new `detect_duplicated_scaffold` (submodule `dcf20f33`, pointer `597ec5b19`, pushed) flags any canonical structural marker (exchange/queue/backlog/icebox opens+closes) that appears **more than once on its own line** (inline prose mentions don't count). The `write.rs` IPC-adoption hook now runs it alongside the response-block check, with **no baseline required** — so the next occurrence auto-captures to `ops.log` + `.agent-doc/logs/ipcfullprompt/`.

#### The captured packet + recovery

- **Preserved** the corrupted buffer + clean baseline at `.agent-doc/logs/ipcfullprompt/manual-20260529-brandon-{candidate,baseline}.md` (the detector didn't auto-fire live because the running route-owned process predates the fix and the shape was uncovered — both now addressed).
- **Restored** `brandon-cinquegrana.md` to HEAD (the corruption was uncommitted — the binary's fail-closed guards kept it out of HEAD). Note: brandon's live editor buffer may still show corruption until it reloads from disk.

#### Verification

- **Tests** (all green): core `detect_duplicated_scaffold_flags_two_exchange_close_markers` (the brandon shape), `_clean_doc_has_no_findings`, `_ignores_inline_marker_mentions`; orchestration `ipcfullprompt_scaffold_duplication_logged_without_baseline`.
- `make check` clippy-clean; only red is the pre-existing `#queue-head-consume-on-topic-id-regression`.
- Built `--release` + `cargo install` + `lib-install`; pushed submodule + pointer.

#### Still open (the cure, not just capture)

Detection/capture is now comprehensive (response-block **and** scaffold). The remaining work is **prevention** — stopping `PatchWatcher.setText` from producing the duplication. That needs the editor transport/payload packet from `idea.log`, which requires enabling `#com.github.btakita.agentdoc` debug logging (IDEA → Help → Diagnostic Tools → Debug Log Settings). Turn that on and reproduce once more, and I'll implement the targeted editor fix + SimWorld regression from the exact packet. `#ipcfullprompt-recur2` stays open for that.

This item tracks an unfixed bug (prevention pending); only the capture sub-deliverable advanced this cycle, so the pending-done guard does not apply.
❯ Was this a plugin issue?

### Re: Was this a plugin issue? — opus-4-8

Yes — fundamentally it's an **editor-plugin** issue, not a CLI/binary one.

#### Where the corruption is produced

The duplication is written by the JetBrains plugin's `PatchWatcher.setText` — it computes a full-document `result` and replaces the live buffer with it. In the brandon capture, that `result` contained the post-exchange scaffold twice (two `agent:exchange` close markers around your in-progress prompt). That `setText` is the corrupting write.

#### Why it's *not* a binary/CLI issue

The `agent-doc` binary that sends patches and owns the commit is clean here: every `ipc_snapshot_adoption` for these docs logged `outcome=blocked` (live-prompt-drift fail-closed), and the corruption stayed **uncommitted** — HEAD was never poisoned. So the CLI's guards did their job; the damage was in the editor's visible buffer/disk, which the binary can only detect after the fact (that's what the forensic detector now does).

#### The nuance worth knowing

It's the plugin's **apply path**, which is part Kotlin and part shared-Rust:

- `PatchWatcher.kt` (Kotlin) owns the `setText` and the normalize step.
- But it calls the shared cdylib via FFI for the actual transforms (`applyComponentPatchWithBoundary`, `repositionBoundaryToEnd`, `normalizeTemplateStructure`).

The trigger is the **interaction**: a binary-synthesized boundary-aware exchange patch that carries your live post-boundary prompt, plus the plugin's normalize + `setText` re-incorporating that same text = the tail gets written twice (the plan's L1∪L2 double-count).

#### What that means for the fix

The cure has to land in the **plugin** (`PatchWatcher.kt`) and/or the synthesis side, and it needs a **plugin rebuild + reinstall** — a cdylib `lib-install` alone won't touch the Kotlin `setText` path. That's exactly why I need the `idea.log` transport/payload packet: to pin down which transport (socket vs file IPC, patch vs reposition vs normalize) the plugin mishandled, so the editor-side fix is targeted rather than speculative on the most corruption-prone path. Enable `#com.github.btakita.agentdoc` debug logging and reproduce once more, and I can write that fix.
❯ Should a new plugin be installed?

### Re: Should a new plugin be installed? — opus-4-8

No — not for anything shipped this session.

#### Why no reinstall is needed now

I changed **no `.kt` files** this session. Every fix (the typing-snapshot dedup, the forensic detectors, the Claude busy-cue) lives in the **binary** (`cargo install`ed) and the **cdylib** (`lib-install`ed) — both are current (built 18:44 today). The installed plugin loads the cdylib **by path and hot-reloads it by mtime**, so even an already-installed plugin picks up these fixes on its next FFI call. No FFI signatures changed, so there's no ABI mismatch with the existing install.

I also checked: there are **no stale long-running `agent-doc` processes** running right now, so nothing is pinned to an old binary.

#### Why the brandon capture didn't auto-fire (the real reason)

Not a stale plugin — the **detector didn't yet cover the scaffold-duplication shape**. That gap is what I closed this cycle (`detect_duplicated_scaffold`). So the *next* reproduction will auto-capture to `ops.log` + `.agent-doc/logs/ipcfullprompt/` with the current binary/cdylib — no reinstall required.

#### When a new plugin *will* be required

The eventual **cure** for `#ipcfullprompt-recur2` — the actual `PatchWatcher.setText` change in the Kotlin plugin — *will* need a plugin rebuild and reinstall (bump `pluginVersion` past the current `0.2.137`; IntelliJ dedupes same-version installs, so the version must change). But I haven't written that yet — it's gated on the `idea.log` transport packet. So: nothing to install today; a fresh signed plugin only once I implement the editor-side fix.
❯ Is [#ipcfullprompt-recur2] complete? Can we mark as done?

### Re: Is [#ipcfullprompt-recur2] complete? Can we mark as done? — opus-4-8

No — not done. I'd push back on closing it, because the bug it tracks (the corruption) still reproduces.

#### What's done vs. not

- **Done — capture/detection.** Both shapes are now caught and auto-preserved: response-block delete/duplicate *and* the full-tail scaffold duplication. That sub-deliverable is genuinely finished.
- **Not done — prevention/cure.** Nothing yet stops `PatchWatcher.setText` from producing the duplication. The corruption can still happen on your next live-typed prompt; we'd just *capture* it now. The editor-side fix isn't written (it's gated on the `idea.log` transport packet).

#### Why I won't mark it done

This exact bug has already been prematurely archived **twice** — `#ipcfullprompt` (2026-05-26) and `#ipcfullprompt-recur` (2026-05-29), both moved to the done archive while still unfixed, which is why `-recur2` exists. Marking it done now while the corruption still reproduces would be the third time, and it would falsely signal the bug is fixed. Per the item's own intent, it stays open until prevention lands and is live-verified.

#### The clean way to record the finished part

If you want closure on the completed work without losing the open bug, I can **split** it:

- Open a focused **`#ipcfullprompt-capture`** as **done** — the forensic detection/preservation that shipped this session (verifiable, finished).
- Keep **`#ipcfullprompt-recur2`** open, narrowed to the **cure only** (the `PatchWatcher.setText` prevention + SimWorld regression), gated on the editor packet.

Say the word and I'll do that split. Otherwise `#ipcfullprompt-recur2` stays open as-is. Either way, I'm not marking the corruption fixed while it can still occur.
<!-- agent:boundary:8a54e4be -->
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

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#queue-head-consume-on-topic-id-regression]
- do [#route-busy-vs-starting-wording]
- do [#codex-queue-head-exact-heading-guidance]
- do [#backlog-auto-marker-misfire]
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ipcfullprompt-recur2] CAPTURE IMPLEMENTED + EXTENDED. (1) Default-on forensic detector (submodule 5ba20b7c): detect_response_block_corruption + write.rs IPC-adoption hooks → .agent-doc/logs/ops.log + .agent-doc/logs/ipcfullprompt/. (2) Extended for the full-tail scaffold-duplication shape (submodule dcf20f33, pointer 597ec5b19): detect_duplicated_scaffold flags any canonical structural marker (>1 on its own line) — reproduced live at brandon-cinquegrana.md (two agent:exchange close markers around an in-progress prompt 'Can it be re' / 'Can it be rep11ro'; the response-block detector missed it; preserved at .agent-doc/logs/ipcfullprompt/manual-20260529-brandon-{baseline,candidate}.md; working tree restored to HEAD). Binary IPC guards verified fail-closed — corruption stayed uncommitted. STILL OPEN: editor-side PatchWatcher.setText PREVENTION needs the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging); detection/capture is now comprehensive (response-block + scaffold). Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [ ] [#queue-head-consume-on-topic-id-regression] CI + make check RED on main (run 26662056181; reproduces at clean HEAD 2a6f60e5 with zero working-tree changes; CI was green on run 26653786056 so it went red recently): tests/finalize_integration.rs::finalize_consumes_synthetic_queue_prompt_when_response_topic_targets_head_id FAILS. A finalize response heading '### Re: #spec-test-build-install-commit-push' that targets queue head preset id #spec-test-build-install-commit-push logs '[queue] skipped consumption because the active prompt did not target the queue head' instead of '[queue] consumed'; head not drained, queue_active stays true. Pre-existing, separate from #prompt-duplicated-while-typing. Bisect the commit that turned it red and restore response-topic-targets-head-id consumption.
- [ ] [#route-busy-vs-starting-wording] [recommended] route.rs starting-actor FailClosed reports 'still starting (waited Ns for claude startup)' even when the pane is busy on an active turn (live capture: pane %33 showed '· Roosting… (Ns · …)' spinner + '⏵⏵ … · N shell'). Distinguish 'busy on an active turn' from 'cold starting' in the error via a Claude busy-cue on the active-turn spinner. Fragile (random gerund; 'esc to interrupt' vs 'thinking with high effort') — UX-only, low priority. Ground truth + analysis: tasks/agent-doc/plan-jb-stale-busy-idle-footer.md.
- [ ] [#codex-queue-head-exact-heading-guidance] [recommended] After #queue-strike-on-halt narrowed the Codex Stop-hook heading-match to exact-topic only, a Codex auto-loop completion with a descriptive heading (### Re: <descriptive topic>) no longer auto-consumes the queue head — only an exact ### Re: do [#id] heading does. Add SKILL.md / harness-invocation guidance telling Codex auto-loop completions to title the response heading exactly with the queue head prompt (or emit an explicit done signal), and live-verify a Codex auto-queue still advances across items. Plan: tasks/agent-doc/plan-queue-strike-on-halt-response.md.
- [ ] [#backlog-auto-marker-misfire] The 'auto' attribute on the agent:backlog marker (queue-only attribute) misfired an auto-run that destructively rewrote the backlog — minted new IDs wrapping 'do [#...]' stubs of already-done items; user reported original task text lost and recovery needed git/sidecar archaeology. Fix: reject 'auto'/unknown attrs on agent:backlog (warn, no mutation); auto-loop must trigger only from the queue 'auto' attribute; never rewrite committed backlog item text in an auto-run. Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md. DO NOT IMPLEMENT.
- [ ] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [ ] [#pending-add-append-not-prepend] `--pending-add` appends new items below the previous CLI arg within the same finalize cycle, contradicting SKILL.md "Pending capture rule" ("Put new items at the beginning of agent:backlog"). Repro: tasks/agent-doc/open-code.md commit f94e7851c — agent added wave-4, wave-3, wave-2, wave-1 expecting wave-1 on top per SKILL.md; actual backlog was wave-4 on top (pure CLI-arg order). User flagged it as bug; agent had to issue --pending-reorder. Plan: tasks/agent-doc/plan-pending-add-prepend-semantics.md (recommend Option A — strict prepend, drop the implicit adjacency heuristic).
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
- [ ] [#jb-busy-queue-ux-feedback] [recommended] When the authoritative actor is genuinely busy, JB Run Agent Doc correctly queues the dispatch in agent:queue auto instead of injecting a duplicate trigger (monsterrodholders.md, equityfundingsource.md reports), but the plugin surfaces the route/resync log dump as an error. Present queued-behind-a-busy-turn as an informational notification distinct from real failures so expected busy-queuing does not read as a plugin error.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
