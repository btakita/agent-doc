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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260530-013546.md`*

Compacted content:
- Archived 6 response topic(s): queue auto enabled; do [#jb-load-tmux-window-verify]; do [#queue-head-consume-on-topic-id-regression]; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#backlog-auto-marker-misfire] — opus-4-8

**Skipping — not implemented.** This item and its plan (`tasks/agent-doc/plan-backlog-auto-marker-misfire.md`) both carry an explicit **DO NOT IMPLEMENT** marker, so I'm respecting the committed instruction rather than the queue directive. The queue head is consumed/struck without any code change.

Notably, this surfaced via the `auto` queue-loop (`queue_trigger: auto`) — i.e. an auto-run pulled in a DO-NOT-IMPLEMENT item, which is exactly the failure *class* `#backlog-auto-marker-misfire` exists to prevent (auto attributes triggering work that shouldn't run). I'm **halting the auto-loop** here and handing control back; the four remaining queue items (`#pending-add-append-not-prepend`, `#next-steps-backlog-capture`, `#run-agent-doc-stale-busy-replay`, `#jb-busy-queue-ux-feedback`) stay queued for a deliberate re-trigger.

Two side notes from this cycle:

- The `stuck_captured_cycle` warning preflight raised is a **false positive**: capture `cycle-1780100032048` ("gate→done for codex guidance") was committed and then archived by compaction (`.agent-doc/archives/483039e7…-20260530-013546.md`), so it's correctly absent from HEAD. No recovery run — that would re-inject an already-archived response. Logged as a recommended backlog item (`#stuck-capture-compact-false-positive`).
- The post-`/agent:exchange` HTML comment referencing `#mermaid` / `#spec-test-build-install-commit-push` / `#next-steps` is parked outside `agent:exchange`/`agent:queue`, so it's a non-executable note and was not run.
❯ Why is [#backlog-auto-marker-misfire] marked as "DO NOT IMPLEMENT"?

### Re: Why is #backlog-auto-marker-misfire marked "DO NOT IMPLEMENT"? — opus-4-8

Because it was filed through the **`#agent-doc-bug` preset**, which is plan-only by definition. That preset (frontmatter) reads: *"Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md."* It never authorizes writing code — it produces a diagnosis, a plan file, and a backlog entry, and then stops.

Concretely, three things put the marker there:

1. **Provenance.** The item entered the backlog via `#agent-doc-bug` on 2026-05-29 13:25 (commit `69ccbc2ff`), originally filed from `boost-client/tasks/monsterrodholders/mrh-project.md`. The plan file header states it directly: *"Status: PLAN ONLY — do not implement. Filed via `#agent-doc-bug`."*

2. **The fix isn't validated yet.** `plan-backlog-auto-marker-misfire.md` lists its root cause as *hypotheses* ("to confirm during implementation") and its remedy as *"decide during implementation."* Nobody has confirmed which path actually rewrote the backlog (marker-attr parser tolerating `auto`/`auot`, the auto-loop keying off any `auto` attribute, or the destructive backlog-rewrite path).

3. **It's high-risk surface.** The fix touches marker-attribute parsing **and** backlog mutation — the same destructive-rewrite territory that caused the original data loss (minted IDs wrapping `do #…` stubs, original task text gone, recovery via git/sidecar archaeology). That's deliberately gated behind explicit go-ahead.

So the marker means: *diagnosed and planned, but do not write code until you explicitly authorize it.* Standard lifecycle for any `#agent-doc-bug`-filed item — it stays `DO NOT IMPLEMENT` until you say "implement #backlog-auto-marker-misfire" (or similar) in the document or chat. You removing the queue's `auto` attribute this turn is consistent with keeping it parked.

(Side note: this turn's `stuck_captured_cycle` warning is again a false positive — the cited body *is* in HEAD; the capture's recorded snapshot hash just went stale after the follow-up pending-only commit advanced HEAD. Same family as `#stuck-capture-compact-false-positive`; no recovery run.)
❯ Please validate the fix for [#backlog-auto-marker-misfire].

### Re: Validate the fix for #backlog-auto-marker-misfire — opus-4-8

I read "validate the fix" as **validate the *proposed* fix in the plan** (no fix code exists — the item is `DO NOT IMPLEMENT`), so this is a static, read-only validation of the plan's three root-cause hypotheses against current v0.34.0 source. **I did not implement anything.** Findings are recorded in `plan-backlog-auto-marker-misfire.md` under a new "Validation against current source" section (committed `2fe0ca366`).

**Verdict: the data-loss path is already prevented by queue-scoping; only the defensive warn remains genuinely open.**

| Plan hypothesis | Holds in v0.34.0? | Evidence |
|---|---|---|
| #2 auto-loop keys off `auto` *wherever* it appears | **No** | All 14 production `has_auto_attr()` call sites pass attrs of a component resolved by `find(\|c\| c.name == "queue")` (run.rs, preflight.rs, route.rs, write.rs, codex_hook.rs, plan.rs, orchestrate.rs). No path reads the `backlog` component's attrs. |
| #3 auto-run destructively rewrites the backlog | **No (for backlog)** | The ID-minting expansion (`orchestrate.rs::queue_task_batch`) reads only the `queue` component and bails unless the queue is active. Nothing lets a backlog `auto` attr drive a backlog rewrite. |
| #1 marker parser tolerates unknown attrs on `agent:backlog` | **Yes — unaddressed** | `component::parse_attrs` (component.rs:577) accepts any token into the attrs map with no per-component validation/warning. `&lt;!-- agent:backlog auto --&gt;` (and the `auot` typo) still parse silently — but the result is now **inert** because no consumer reads backlog attrs. |

**What this means:** Against current code the destructive misfire does **not** reproduce statically. The auto-loop and the `do [#…]`-stub ID-minting are both strictly queue-sourced, so an `auto`/`auot` on the backlog marker is silently accepted but does nothing. The only remaining value in the planned fix is **defensive UX** — warn/reject a misplaced `auto`/unknown attr on `agent:backlog` ("did you mean `agent:queue auto`?"), proposed fix #1, plus optionally the recovery snapshot (fix #4).

**Honest uncertainty:** this is a static read of current source only. I could **not** pin the original corruption to a specific prior version/path — its shape (`preset #…` line + `- [ ] [#minted] do [#oldid]`) matches the orchestration task-batch *backlog write-back* output, so the historical trigger was an orchestration write-back that, in today's code, only sources from `queue`. Whether the queue-scoping that closes #2/#3 was pre-existing or landed with the recent queue-auto convergence work isn't proven without git archaeology of the orchestration write-back at report time.

`#backlog-auto-marker-misfire` stays `DO NOT IMPLEMENT`. If you want the defensive warn (fix #1) actually built, say "implement #backlog-auto-marker-misfire" and I'll scope it to a parser-level warning with the regression tests the plan lists.
❯ implement #backlog-auto-marker-misfire

### Re: implement #backlog-auto-marker-misfire — opus-4-8

Implemented the defensive fix, the only part the validation found genuinely open (#2 auto-loop scope and #3 backlog rewrite were already prevented by queue-scoping). **Marked done.**

**Change** — a non-blocking `misplaced_component_attr` preflight warning (`agent-doc-orchestration/src/preflight.rs`):
- Queue-only attr (`auto`) on a non-`queue` component → warns with `did you mean &lt;!-- agent:queue auto --&gt;?`
- Unrecognized attribute key (the reported `auot` typo) on any component → warns `not a recognized component attribute`
- The attribute is **never mutated** and never activates the auto-loop — the warning just makes the previously-silent misfire visible. Known keys allow-listed: `patch`, `mode`, `max_lines`, `archive`, `transfer-source`, `timestamp`, `broken`, plus queue-only `auto`.

**Verification:**
- 4 new tests: `auto`-on-backlog warns, `auot` typo warns, queue `auto` + known attrs do **not** warn, and a regression asserting backlog `auto` never activates the queue.
- `make check` green (clippy `-D warnings` + full suite).
- `cargo build --release` + `cargo install --path .` (both binaries refreshed).
- **Live-verified**: ran the installed binary's `preflight` on a throwaway doc with `&lt;!-- agent:backlog auto --&gt;` → warning fired as expected.
- Spec §2.4 (attribute validation) + §2.5 (queue-only `auto`) updated.

**Commits:** submodule `fdc184f2` (code+spec+tests, pushed), superproject pointer `f88ca1951`, plan status → IMPLEMENTED `881d3656a` (all pushed to `main`).

**Not implemented:** proposed fix #4 (pre-auto-run recovery snapshot) — deferred as unnecessary now that the misfire is both inert (queue-scoped) and surfaced (warning). Fixes #2/#3 needed no code change. If you want #4 anyway, say so and I'll file/scope it.
<!-- agent:boundary:ac5ef746 -->
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
- ~do [#jb-load-tmux-window-verify]~
- ~do [#queue-head-consume-on-topic-id-regression]~
- ~do [#route-busy-vs-starting-wording]~
- ~do [#codex-queue-head-exact-heading-guidance]~
- ~do [#backlog-auto-marker-misfire]~
- do [#pending-gate-snapshot-desync]
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ah0s] [recommended] Add explicit-position --pending-add primitives so ordered backlog batches never depend on implicit prepend-vs-argorder semantics: --pending-add-after <id>, --pending-add-before <id>, --pending-add-back/--pending-append, keeping --pending-add as the front-insert default. Backlog is a priority pool with ID-based consumption (not a stack/queue; FIFO lives in agent:queue), so position should be author-set explicitly when it matters. Two agents (open-code.md #adcr waves f94e7851c; corky.md crate-split phases fc8d9b3bc) both reversed args expecting prepend-flip and got reverse-of-intended order. Pairs with #pending-add-append-not-prepend (that item finalizes the default-order decision; this adds the explicit escape hatches). Plan: tasks/agent-doc/plan-pending-add-explicit-position.md.
- [ ] [#pending-add-suppresses-queue-consume] [recommended] Bundling --pending-add in the same finalize as a queue-head-targeting response suppresses queue-head consumption: finalize logs 'skipped consumption because the active prompt did not target the queue head' and treats the pending-add as the active prompt, even when the response heading exactly matches the head (Re: do #ID form, no brackets). Repro 2026-05-29 agent-doc-bugs2.md declining backlog-auto-marker-misfire with a bundled --pending-add — head stayed unstruck. Fix: heading/queue-head match should drive consumption independently of bundled pending mutations, or consume the head when the response heading matches regardless of detected active prompt.
- [ ] [#stuck-capture-compact-false-positive] [recommended] stuck_captured_cycle false-positive after compaction: a capture recorded as committed (cycle-1780100032048, response 'gate→done for codex guidance') is later archived by compact, so preflight reports committed-but-not-in-HEAD and suggests `write --commit` recovery — which would re-inject an already-archived response (it lives in .agent-doc/archives/<hash>-*.md). Fix: when a captured-committed body is absent from HEAD because it was compacted (present in the matching archive), reconcile the capture instead of flagging it stuck. Repro 2026-05-29 agent-doc-bugs2.md.
- [ ] [#pending-gate-snapshot-desync] `--pending-gate <id>` lands the gate ([/], moved to review) on the working-tree disk but NOT the committed snapshot when the IPC listener is dead and the write goes through the CRDT-merge / --force-disk path: the snapshot is saved from content_ours (baseline+response, item still [ ]) before the gate mutation, so commit sees snapshot==HEAD and leaves the gate uncommitted as post-commit local drift. Repro 2026-05-29 on agent-doc-bugs2.md finalizing the codex heading-guidance item; --done does NOT hit it (reap is captured pre-snapshot). Fix: apply pending gate/edit mutations before saving content_ours / the commit snapshot.
- [ ] [#codex-auto-queue-live-verify] [recommended] Live-verify a Codex auto-queue advances across items with the new harness-invocation heading-discipline guidance: run a real Codex auto-queue and confirm a descriptive-heading completion advances once the agent titles the heading exactly with the head (### Re: do [#id] / ### Re: #id) or passes --done. Needs a live Codex session.
- [ ] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
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
