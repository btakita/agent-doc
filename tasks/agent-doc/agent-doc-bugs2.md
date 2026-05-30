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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260530-041336.md`*

Compacted content:
- Archived 2 response topic(s): do #codex-auto-queue-live-verify; do #drained-done-queue-clear
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: #finalize-retry-ipc-response-duplication — opus-4-8

Implemented the CLI-side fix + recovery for the multi-retry/late-IPC response duplication that `❯`-prefixed agent body lines. Three changes in `src/agent-doc/agent-doc-orchestration/src/`:

- **`git.rs` — never `❯`-prefix response body.** `canonicalize_answered_prompt_prefixes` now tracks *why* the prelude walk stops. A prose block that butts directly against a preceding `### Re:` heading (no blank-line/comment separator) is the trailing body of that response — e.g. a duplicated block from a retry — not a fresh user prelude, so it's skipped. This is the root cause of `❯ **Scope:** …` / `❯ **Commits:** …` mis-attribution (hypothesis #2).
- **`dedupe.rs` — `❯`-insensitive collapse, keep the clean twin.** `normalize_response_block_line` strips a leading `❯` from response-body lines so a corrupted copy still compares equal to its clean committed twin (previously the prefix made them look "new" and slipped past dedupe + over-application guards). `dedupe_responses_with_report` now drops the `❯`-corrupted copy of a duplicate pair (new `block_has_prompt_prefixed_body`), keeping the clean one instead of always-second.
- **Recovery is now automatic.** Because `is_committed_response_overapplication` (and the consecutive dedupe replay) are `❯`-insensitive, `detect_late_ipc_response_overapplication` / `detect_jb_cache_conflict_accept_duplicate_replay` recognize the corrupted-duplicate shape and `preflight` restores clean HEAD over the working tree — eliminating the manual `git checkout HEAD -- <doc>` the repro required (hypothesis #3).

**New deterministic regression coverage:**
- `dedupe::dedupe_collapses_prompt_prefixed_corrupted_duplicate` — corrupted duplicate → one clean block, no surviving `❯`, idempotent (stands in for the two-retry `already_applied` convergence invariant).
- `dedupe::overapplication_detects_prompt_prefixed_corrupted_duplicate`.
- `git::canonicalize_answered_prompt_prefixes_never_prefixes_duplicate_response_body`.
- `session_check::detects_prompt_prefixed_corrupted_duplicate_as_overapplication` — remediation restores committed HEAD, no manual checkout.

**Verification:** `make check` green (clippy clean + full suite). Both binaries rebuilt (`cargo build --release` + `cargo install --path .`, exit 0) so the fix is live in `.bin/agent-doc` and `~/.cargo/bin/agent-doc`.

**Scope/honesty:** This is the CLI defensive + auto-recovery layer. Root-cause hypothesis #1 (live-buffer non-idempotence across IPC `already_applied` retries) is plausibly plugin-side — the plan flags confirming whether plugin 0.2.138 eliminates the compounding before adding more CLI machinery. Gating (not closing) this item: code-complete and unit-verified, awaiting live IDE-IPC validation that the corruption no longer recurs and the preflight auto-repair fires in a real session.

### Re: #pending-add-append-not-prepend — opus-4-8

Resolved as **already-correct — no binary change** (your call: close as spec-conformant).

Verified against source + test:
- `src/notify.rs:187` iterates `pending_add.iter().rev()` inserting each at position 0, so CLI args `[A, B]` land as backlog `[A, B, …existing]` — reading order at the **front**, which is exactly SKILL.md's "Put new items at the beginning of `agent:backlog`."
- Test `pending_add_multiple_items_keep_sequence_order_at_front` (`src/notify.rs:603`) pins this: `--pending-add "first new" --pending-add "second new"` over `[#abcd]` → `[first new, second new, #abcd]`. **Passes.**

The plan's repro (`open-code.md` f94e7851c: waves added 4→3→2→1, wave-4 landed on top) was a **usage error**, not a binary defect — items must be passed top-to-bottom (wave-1 first), not tail-first. This matches the standing guidance in `feedback-agent-doc-pending-add-order` ("the ordering behavior is correct, so don't file a fabricated bug").

Plan Option A (newest-arg-on-top stack) was **not** implemented: it would invert the documented convention, break the passing test, and contradict that feedback. `tasks/agent-doc/plan-pending-add-prepend-semantics.md` is effectively superseded — current behavior already satisfies the "beginning of backlog" spec; the doc fix it recommends (Option A) is declined. If a true newest-on-top stack is ever wanted, it should be a new explicit opt-in flag, not a change to `--pending-add`'s default.
<!-- agent:boundary:7efc6207 -->
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
- ~do [#pending-gate-snapshot-desync]~
- ~do [#codex-auto-queue-live-verify]. auto-queue in monsterrodholders.md stalled~
- ~do [#drained-done-queue-clear]~
- ~do [#finalize-retry-ipc-response-duplication]~
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#recguard-abandon] [recommended] Recursive same-pane direct `agent-doc <FILE>` correctly refuses to dispatch back into the owning Codex pane, but the guard path can leave an empty `preflight_started` cycle that requires manual recovery. Live repro 2026-05-30: repeated `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` from owner pane `%28` failed with `recursive direct invocation would deadlock`; fix should fail fast with the same diagnostic while closing/abandoning the empty preflight cycle so `session-check` accepts the terminal state. Plan: tasks/agent-doc/plan-recursive-direct-invocation-abandon.md.
- [ ] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [ ] [#codex-auto-queue-stalled-final-gate] Codex auto-queue final gate failed live: after `src/boost-client/tasks/monsterrodholders.md` completed/deployed `#seocat`, the document still had `queue_active: true`, `agent:queue auto`, and next head `do [#seopdp]`, but Codex sent a normal final answer instead of continuing/blocking. This is the failed live proof for `#codex-auto-queue-live-verify`. Fix by adding a binary-owned durable "queue continuation required" marker/final gate that successful closeout writes when a ready auto-queue head remains, and teach Codex Stop/session-check strict mode to block final answers even when transient hook session state is missing or the turn used a manual/recovery closeout path. Plan: tasks/agent-doc/plan-codex-auto-queue-stalled-final-gate.md.
- [ ] [#active-queue-persisted-no-continue] Active persisted queues can stall after a successful closeout when `queue_active: true` remains but the queue tag lacks `auto`: the next head is ready, yet continuation paths only consider `agent:queue auto` (`codex_hook.rs::active_auto_queue_prompt` requires the attr; direct continuation gates on an auto outcome; Claude guidance checks `queue_trigger == "auto"`). Repro 2026-05-30 `tasks/agent-doc/agent-doc-bugs2.md`: plain `agent:queue` (no auto attr), `queue_active: true`, five remaining heads beginning `#pending-gate-snapshot-desync`, no next cycle launched after closeout. Treat `auto` as a start trigger and `queue_active` as the continuation signal, or halt with an explicit reason. Plan: tasks/agent-doc/plan-active-queue-persisted-continuation.md.
- [ ] [#bare-write-captured-uncommitted] `agent-doc write` can preserve/synthesize a template response body, receive IPC `already_applied`, then fail `session-check` with the cycle still `response_captured`, leaving the visible response outside the binary-owned commit boundary. Repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` #seocrawl closeout: `agent-doc write --done seocrawl ...` printed "bare `agent-doc write` preserved the response body, but the session closeout is still outside the commit boundary"; operator had to manually commit `tasks/monsterrodholders.md` as `c13c258`. Fix: if write places/preserves response content, continue through snapshot+commit+session-check or fail before mutating; add idempotent repair for matching `response_captured` body-present state. Plan: tasks/agent-doc/plan-bare-write-captured-response-outside-commit.md.
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
