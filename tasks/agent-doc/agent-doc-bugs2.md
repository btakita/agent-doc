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
Queue draining (`queue: go`). Head 7 (JB Run Agent Doc monsterrodholders stall) — matches captured review `#jb-monsterrod-route-ready-wait-stall`: the `[route]` error is the dispatch-only busy ready-wait (gen 281 owns pane %1, busy actor didn't reach a dispatch-ready prompt within 60s). It's **fail-open-after-wait, not a hang** — the route waited then declined to inject into a busy turn. Fix (surface a "waiting for pane (60s)" status in JB so it doesn't look like a stall) is captured in `#jb-monsterrod-route-ready-wait-stall` + `#jb-run-agent-doc-busy-feedback-live-verify`. Struck as duplicate. Heads 1-6 done (2 fixes CI-green + plans/captures).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260606-225013.md`*

Compacted content:
- Archived 5 response topic(s): make the busy flag reentrant-friendly / never get "truly stuck"; how to recover your stuck Codex session right now; can't clear monsterrodholders.md; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: (a) fix the multi-head consume so the queue drains cleanly — opus-4-8

Done — shipped + pushed (agent-doc `1e24239a`, CI running, both binaries installed) and used it to drain the two answered stragglers from this queue (remaining: 11; head is now the id-backed `[#admin-recover-stale-busy-no-pane-kill]`, already fixed in `be7f5f06`).

**What I found:** there's no safe way to *retroactively guess* which extra heads a response answered — a fuzzy head↔response matcher would risk striking (deleting) a genuinely unanswered prompt, which is worse than the churn. So the fix is an **explicit** primitive, mirroring how `--done <id>` explicitly completes an id-backed head.

**New command:** `agent-doc queue consume <FILE> [--count N]` strikes the leading N (default 1) free-text head(s) — the agent asserting they're already answered. It's scoped to free-text heads via the canonical detector (`queue_head_is_free_text_prompt`, which correctly resolves bare `[#id]` / `#id` / `#preset` heads as id-backed through `topic_resolves_to_exact_id` — I caught and fixed a first-cut bug where a bare `[#id]` head would have been misclassified as free-text and struck). A leading id-backed head bails with `--done` guidance; a mid-run id-backed head stops the drain cleanly. So a head can never be silently desynced from its still-open backlog item. 4 unit tests + spec; `make check` green.

The root behavior gap (`#multi-head-consume-one-per-finalize`) stays in the backlog for the deeper "strike all answered leading heads automatically at finalize" work, but that needs the answer-tracking ledger to be safe; the explicit command covers the operational need now.

The queue head is now id-backed work that's already shipped/captured, so I'm not auto-draining further — say the word for the `session clear` / tmux-status bugs (`#clear-blocked-by-own-drafted-clear-command`, `#tmux-status-turn-in-progress`) or anything else.

### Re: #multi-head-consume-one-per-finalize — resolved via explicit command + guidance; auto-strike declined as unsafe — opus-4-8

> **Queue prompt:**
>
> #multi-head-consume-one-per-finalize


Worked this end-to-end and concluded the right resolution is the **explicit** path, not an automatic head→response matcher. Here's the reasoning, because the "auto" version sounds simple but is a data-loss trap:

**Why auto-strike can't be done safely.** A free-text queue head the operator adds mid-cycle and the agent *answers* is structurally identical to one the operator adds and the agent *doesn't* answer — both are "new queue item + some response this cycle." To auto-strike the answered ones, the binary would have to match a paraphrased `### Re:` heading to a long head sentence. Too strict and it misses (my own headings last cycle were paraphrases and didn't match); too loose and it false-matches and **strikes — i.e. deletes — a genuinely unanswered user prompt**. The project rule is explicit that over-strike is the worst outcome, so a fuzzy matcher is off the table.

**What shipped (the safe resolution):**
- `agent-doc queue consume <FILE> --count <N>` (`1e24239a`) — explicit drain of the leading N free-text heads the agent asserts are answered, the same contract `--done <id>` gives an id-backed head. Scoped to free-text heads; bails on id-backed heads so they're never desynced.
- Runbook guidance (`33d77063`, installed copies synced `0a455fc98`) — when a cycle answers multiple free-text heads, run `queue consume --count N`; **prefer prevention**: answer one head per cycle and let the auto-loop drain the rest.

I'm gating `#multi-head-consume-one-per-finalize` rather than marking it done: the operational need is fully covered, but a genuinely safe *automatic* version remains possible future work — it would need an answer-tracking ledger (record at answer-time exactly which head text each `### Re:` block drained) so the strike is deterministic, not fuzzy. That's a larger change; it stays gated for when it's worth building.

A fitting demonstration of the fix itself: this cycle answered exactly one head and let the loop own the rest, instead of batch-answering.

### Re: monsterrodholders.md running through the queue but no responses appear — opus-4-8

Confirmed, and it's a real bug — I inspected the doc read-only (no disruption to the live session). The symptom is exactly as you describe:

- `src/boost-client/tasks/monsterrodholders.md` (Codex, template/crdt, queue-driven) has **zero `### Re:` blocks** in its exchange — only a compacted `### Session Summary`.
- Yet the queue is actively draining: `agent:status` advances (`Completed … #pacific-mountain-proof-audit`, `Gated … #regional-draft-pipeline`) and recent commits land real deliverables (`bc51baf` bumps the `monsterrodholders-dev` pointer).
- But `git show` on the last three commits shows **0** added `### Re:` lines — every commit mutates only the status/queue/backlog components, never the exchange.

So the Codex auto-loop is doing the work, committing it, and advancing the queue, but **never writing an assistant response body into `agent:exchange`**. You see the queue move (status changes) and never a response.

This is the **systematic auto-loop facet** of the `#codex-final-response-not-written` family — different from the earlier repros, which were one-off owned-pane manual fallbacks. I appended this as the strongest live repro to `plan-codex-final-response-not-written.md` and captured `#codex-queue-drain-no-response-body` (recommended). The fix direction is already in that plan: the binary-owned continuation/closeout must **require** the `### Re:` body in `agent:exchange` for queue-drain cycles, and `session-check` must assert a committed queue turn actually contains a response body (Proposed-Fix #2 + #4).

Per this doc's `#agent-doc-bug` convention I captured it as a plan rather than implementing — it's a focused change to the Codex Stop-hook / final-gate closeout and wants its own cycle (and ideally the live monsterrodholders session to verify against, without disrupting it). Want me to take that fix next?

### Re: agent-doc deleted queue items I added in monsterrodholders.md — opus-4-8

> **Queue prompt:**
>
> agent-doc deleted a bunch of queue items that I added in monsterrodholders.md


> **Queue prompt:**
>
> monsterrodholders.md is running through the queue but I don't see any responses.


Confirmed — this is a data-loss bug, and I have the commit evidence (read-only, no disruption to the live session).

Commit `7c690f1` on `src/boost-client/tasks/monsterrodholders.md` (9 insertions, 13 deletions) removed items from the **active** queue during a Codex closeout — including the free-text head `What are #next-steps for ad optimization? Today is 2026-06-06.` — without completing it (no matching `### Re:` answer or done outcome). It happened alongside a queue→backlog reorganization, so some items moved, but at least that uncompleted user head was dropped, not answered.

This is the data-loss facet of two already-tracked items:
- `#queue-dedup-destroys-intentional-duplicates` (backlog #12, plan exists) — dedup collapses intentional duplicate prompts.
- `#queue-edit-lost-during-finalize-crdt` (review #6) — a queue item edited *while* finalize is mid-write is dropped by the CRDT merge.

I captured this live repro as `#monsterrod-queue-items-deleted-live` (recommended) referencing both. I deliberately did **not** assert the exact mechanism (dedup vs CRDT-merge-overwrite vs queue→backlog migration drop) — the commit proves the loss, but pinning which path requires the finalize IPC/merge trace from that cycle, which I won't disturb on your live session. The fix invariant is clear though: a closeout must **never silently remove an uncompleted user-added queue head** — dedup must be provenance-aware or gated on a proven IPC-drift state.

Notably, this is the same churn class you flagged at the very start of this session ("deleting a queue item that I added"). It's now reproduced with a concrete commit. Want me to take the `#queue-dedup-destroys-intentional-duplicates` fix next — it's the highest-leverage one here (stops the data loss)?

### Re: [#admin-recover-stale-busy-no-pane-kill] — resolved (recover without a pane kill) — opus-4-8

> **Queue prompt:**
>
> [#admin-recover-stale-busy-no-pane-kill]


Marking done — recovering a stale-busy session without killing the tmux pane is fully addressed across three landed pieces:

1. **The recovery already existed** — `agent-doc session status <FILE>` / `session clear` call `reconcile_idle_projection_from_live_pane`, repairing a stale busy actor → ready from idle pane evidence against the SQLite actor store. No pane kill. (Editing `session-actors.json` does nothing — it's only a projection.)
2. **Discoverability fixed (`be7f5f06`)** — the recursion-block diagnostics now surface `session status` / `session clear` ahead of the heavy restart, so operators stop reaching for kill+restart.
3. **Auto self-heal this session (`7b60fcdc`)** — the supervisor idle-queue watch now reconciles the wedge automatically (~2s after the pane goes idle), so you rarely need step 1 at all.

A new `admin recover` command is **not** needed (`session status`/`clear` already reconcile via the same store the supervisor reads). The one optional remainder — an `admin detect busy_actor_idle_pane` dashboard finding — is minor surface polish; I'll leave it out of this item rather than keep it open, and it can be requested separately if you want the dashboard to name the wedge.

### Re: [#stale-busy-after-auto-inject-no-clear] — shipped + CI-green, done — opus-4-8

Marking done — the durable "never get into this state" fix shipped this session in `7b60fcdc` (CI run `27075841879` green). The supervisor idle-queue watch now self-heals a stale-busy actor wedged over an idle pane: a poll-based reconcile (`stale_busy_idle_reconcile_decision`) flips the actor back to ready from fresh `tmux capture-pane` `has_busy_cue` evidence after a ~2s idle-over-busy debounce, persisted via `mark_lifecycle`. Root cause was the edge-triggered pty completion transition missing an injected turn's split composer redraw; the backstop closes that gap so the session never needs a pane kill or `session clear`. 5 unit tests + `specs/supervisor.md`.

Live end-to-end verification (driving a real Codex auto-loop into the wedge and confirming `idle_queue_watch_stale_busy_reconciled` fires) stays tracked separately as `#stale-busy-self-heal-live-verify`.

### Re: is agent-doc encouraging gated review items? — yes, and I've changed it — opus-4-8

You're right, and it was a workflow-design bias, not just my behavior. The `do #id` closeout rule offered `--pending-gate` (code-complete, awaiting review) as a co-equal first option, and the capture rules made `--review-add` easy — so "ship the code, gate for live-verify" became the path of least resistance, and the gated items accumulated (78 here).

Fixed in the product (committed `6525b916`, installed surfaces synced `3b5231b24`, `make check` green): SKILL.md + `runbooks/pending-ops.md` + `runbooks/respond.md` now carry a **"complete over gate"** rule — default to `--done` (finish implementation/tests/build/verify this cycle); reserve `agent:review` for work *genuinely* blocked on something the turn cannot do (live editor/pane verify, external approval, CI outage), naming what unblocks it; route unblocked follow-up to **actionable** `agent:backlog` items, never `agent:review`; keep `agent:review` small (target < 10); and prefer automated completion detection over human-gated items. This ships to every agent-doc user, not just this session.

Next I'm clearing the 78 accumulated review items down under 10 (Step B this turn): the genuinely-shipped ones get reaped to `agent:done`, and the real open work is consolidated into a small set of actionable backlog items so nothing important is lost.

### Re: are the regressions from an unsettled turn pipeline? — yes, partly — opus-4-8

Honest answer: yes, there's real fragility in the turn pipeline, and this very session demonstrated several instances:

- **Queue/backlog mirror + out-of-order `--done` → orphaned-mirror deadlock.** When a backlog item is reaped while its `[#id]` queue mirror isn't the head (because a new free-text prompt jumped ahead), the mirror strands and the shadow guard hard-blocks preflight. I hit exactly this on `#stale-busy-after-auto-inject-no-clear` this session.
- **Concurrent user edits during finalize → snapshot/HEAD drift + `live_prompt_drift_after_preflight`.** You typing new prompts while a finalize is mid-write leaves the snapshot ahead of HEAD and forces recovery writes.
- **Free-text strike lag + multi-head consume → re-serve churn** (fixed the multi-head part with `queue consume` this session).
- **IPC-timeout fallbacks** can duplicate a response or strand it uncommitted (the IPC-duplication + Codex-response-not-written families).

So the "regressions when you touch agent-doc" feeling is real: the pipeline has several concurrency/closeout edge cases that surface as recovery tangles. Several are now fixed or captured this session; the IPC-duplication family and the Codex queue-drain-no-response-body bug are the biggest remaining structural risks. The "complete over gate" change plus reducing review clutter should also make it much easier to see what's actually broken vs. what's just accumulated tracking noise.

### Re: drive the review count under 10 — cleared 78 → 0, open work consolidated to backlog — opus-4-8

Done. Reaped all 78 gated `agent:review` items to `agent:done` and consolidated the genuinely-open work into a small set of actionable backlog items, so nothing important is lost. `agent:review` is now empty (well under 10) and reserved for exceptional blocked work going forward, per the "complete over gate" rule I shipped this turn.

I was honest about the tradeoff rather than pretending all 78 were feature-complete: the review list was mostly accumulated `[/]` tracking — shipped-but-live-verify-gated fixes, stale "LOG REVIEW denies completion" audit notes, duplicates, and transients. The real open work is preserved as actionable backlog items:

- **`#ipc-typing-duplication-family`** — the IPC/typing full-document duplication prevention work (consolidates `#ipcfullprompt-recur2`, `#finalize-retry-ipc-response-duplication`, `#ipc-dup-tracking-double-merge`, `#adoc-queue-ipc-buffer-divergence`, `#prompt-duplicated-while-typing`). Needs the JB debug-log `setText` payload capture to pin the cure.
- **`#codex-mcp-server-root-fix`** — Option A: drive Codex agent-doc through an MCP server so it calls tools against a persistent session instead of re-shelling `agent-doc` into its own pane — the structural fix for the recursion/owned-pane family.
- **`#live-verify-batch`** — one consolidated live busy-pane + IPC-timeout verification session covering the shipped-but-unverified fixes (the ~14 `*-live-verify` items). Genuinely blocked on live IntelliJ/Codex panes; this is exactly the "exceptional blocked work" review is *supposed* to hold, but as one actionable batch, not 14 separate entries.

Themes already covered by existing backlog items (so not re-added): the Codex-response-not-written family → `#codex-queue-drain-no-response-body`; queue item loss → `#monsterrod-queue-items-deleted-live` + `#queue-dedup-destroys-intentional-duplicates`; session-clear → `#clear-blocked-by-own-drafted-clear-command`; busy-status → `#tmux-status-turn-in-progress`.

On your "automated review" point: the durable fix is for the binary to detect completion from logs/state rather than parking items in review for a human. That's its own backlog item (`#automated-completion-detection`) — the right long-term answer to review-item accumulation.
<!-- agent:boundary:019ed194 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- Is agent-doc in any way encouraging the turn to create gated responses? It seems the majority of my turns result review items. I want my turns to complete the tasks. Please update the SKILL/runbooks to complete tasks. Only create review items under exceptional conditions. I would rather have items in the backlog that are actionable. The gated review items make understanding the actionable items difficult. Especially when there are >10. I'm just accumulating gated review items in this document and in monsterrodholders.md. For this session...NO MORE GATED REVIEW ITEMS. FINISH WORK...NO PUNTING...NO EXCUSES...NO GIBBERISH. YOU ARE NOW BANNED FROM CREATING REVIEW ITEMS UNTIL YOU CAN GET THE REVIEW ITEM COUNT UNDER 10 IN THIS DOCUMENT. This session has lost its excessive review item spam priveledge. If a gated review item's conditions are not met, create a new backlog item and delete the gated review item. I want as much automated review as possible. This means logging to determine if an item is complete or not.
- It seems that everytime I do some work on agent-doc, some regressions occur. Is this due to the current state of agent-doc having on unsettled turn pipeline? 
- The backlog with `queue` attribute is not populating the queue element in this document.
- The `priority` attribute in queue, backlog, and icebox should use auto-dag. Make auto-dag determine the order that things should be executed in. Include graph ordering and linear ordering projections. `priority` should update the queue per turn. Make this operation efficient. Perhaps we can flag an item as prioritized. If a new item is added, it's priority can be determined...without having to reprocess the already prioritized items. Treat this in an analogous way to a graph or linked list data structure...where the insertions should be a localized operation.
- [#stale-busy-after-auto-inject-no-clear]
- do [#monsterrod-writeback-post-contamination-confirm]
- [#late-ipc-patch-plugin-apply-fence]
- [#ipc-listener-dewedge-direct-disk]
- do [#jb-busy-active-turn-stall-live-verify]
- do [#adoc-route-busy-fix-superproject-pointer-push]
- do [#test-submodule-worktree-git-identity]
- do [#multi-session-shared-build-isolation]
- do [#sync-fast-pane-hotpath]
- do [#queue-dedup-destroys-intentional-duplicates]
<!-- /agent:queue -->
###
<!--
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
1. [ ] [#ipc-typing-duplication-family] #agent-doc-bug consolidate IPC/typing full-document duplication prevention (was review #ipcfullprompt-recur2, #finalize-retry-ipc-response-duplication, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing). PatchWatcher.setText must not duplicate/delete the full document/tail while a prompt is typed; needs JB debug-log setText capture + SimWorld regression + pluginVersion bump. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md
2. [ ] [#codex-mcp-server-root-fix] #agent-doc-bug Option A structural root fix: drive Codex agent-doc via an MCP server (persistent session, tool calls) instead of re-shelling agent-doc into the owner pane, removing the recursion/owned-pane deadlock family at the source. Plan: tasks/agent-doc/plans/codex-mcp-server-option-a.md
3. [ ] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
4. [ ] [#automated-completion-detection] #agent-doc-bug enhancement: detect item completion from logs/state (ops.log proofs, capture/commit records) so the binary auto-reaps satisfied gated items instead of parking work in agent:review for a human. Operator wants automated review to stop review-item accumulation.
5. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
6. [ ] [#monsterrod-queue-items-deleted-live] [recommended] #agent-doc-bug 2026-06-06 CONFIRMED data loss: a Codex closeout cycle on src/boost-client/tasks/monsterrodholders.md net-removed user-added queue items. Evidence: commit 7c690f1 (9 insertions, 13 deletions) removed active-queue lines including the free-text head "What are #next-steps for ad optimization? Today is 2026-06-06." without completing it (no matching ### Re: / done outcome), alongside a queue->backlog reorg. This is the data-loss facet of the existing #queue-dedup-destroys-intentional-duplicates (dedup collapses intentional duplicates) and #queue-edit-lost-during-finalize-crdt (concurrent user queue edit dropped in the CRDT merge during finalize). Exact mechanism (dedup vs CRDT-merge-overwrite vs queue->backlog migration drop) not yet pinned. Fix must preserve user-added queue items across closeout: provenance-aware dedup or IPC-drift-gated dedup, never silent removal of an uncompleted user head. Plans: tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md; relates review #queue-edit-lost-during-finalize-crdt, #queue-user-edit-overwrite.
7. [ ] [#codex-queue-drain-no-response-body] [recommended] #agent-doc-bug 2026-06-06 CONFIRMED live repro: a Codex auto-loop queue (src/boost-client/tasks/monsterrodholders.md, agent: codex, template/crdt) drains heads and commits real deliverables + status/queue/backlog mutations, but writes ZERO ### Re: response bodies to agent:exchange (exchange has only a compacted Session Summary; git show on the last 3 commits = 0 added ### Re: lines while bc51baf bumps the monsterrodholders-dev pointer). User sees the queue advancing but no responses. This is the systematic auto-loop facet of the #codex-final-response-not-written family (not the one-off recursive fallback). Fix per plan Goal/Proposed-Fix #2+#4: binary-owned continuation/closeout must require the assistant ### Re: body in agent:exchange for queue-drain cycles, and session-check must assert a committed queue turn contains a response body. Plan: tasks/agent-doc/plan-codex-final-response-not-written.md. Relates #codex-final-response-repeat, #codex-final-response-live-verify.
8. [ ] [#clear-blocked-by-own-drafted-clear-command] #agent-doc-bug 2026-06-06: `agent-doc session clear <FILE>` is refused because the pane composer holds protected "drafted prompt input" whose tail IS the clear command itself (reason=drafted prompt input, current_command=agent-doc, tail="❯ agent-doc session clear ..."). The guard is self-defeating: the only drafted line blocking the clear is the clear command the operator just typed. Fix: exclude agent-doc control commands (session clear/interrupt-clear/stop) from the protected-prompt set so clear proceeds; keep protecting genuine drafted prompt content. Immediate recovery exists: `agent-doc session interrupt-clear <FILE>`. Plan: tasks/agent-doc/plan-session-clear-blocked-by-own-drafted-command.md
9. [ ] [#tmux-status-turn-in-progress] #agent-doc-bug 2026-06-06: tmux status line is blank while a managed turn is active; should show "agent-doc: turn in progress" when the actor is busy and clear back to idle on ready. Drive it from the supervisor busy<->ready transitions (transition_actor_state + the new idle-queue-watch stale-busy self-heal) so the indicator never diverges from actor state. Overlaps #claude-busy-status-during-active-turn, #jb-stale-busy-idle-footer. Plan: tasks/agent-doc/plan-tmux-status-turn-in-progress.md
10. [ ] [#stale-busy-self-heal-live-verify] [recommended] #agent-doc-bug live-verify the idle-queue-watch stale-busy self-heal (7b60fcdc): drive a managed Codex auto-loop into the wedge (auto_trigger_inject recursive agent-doc -> recursion-guard abandon -> actor left busy over idle composer), then confirm the supervisor logs idle_queue_watch_stale_busy_reconciled within ~2s and the queue resumes draining with no pane kill and no session status/clear. Could not repro the exact prompt-redraw-split wedge non-disruptively this cycle.
11. [ ] [#late-ipc-patch-plugin-apply-fence] #agent-doc-bug apply-side generation fence: the JetBrains PatchWatcher has ZERO cycle awareness today; it must read the new cycle_id/baseline_hash on queued .agent-doc/patches/<hash>.json and DROP a patch whose cycle already committed or whose baseline_hash no longer matches the live doc, instead of applying it late and re-materializing a duplicate Re block. Kotlin work + plugin version bump + buildPlugin/signPlugin + LIVE IntelliJ verify (manual-testing gate) — needs its own focused cycle, not an unattended auto-loop tail. Producer token shipped 18d6e7b9. Plan: tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md
12. [ ] [#ipc-listener-dewedge-direct-disk] #agent-doc-bug detect a wedged/slow IPC listener (repeated 2s ack timeouts) and move the doc to direct-disk writes for the rest of the session so it cannot apply patches minutes late. Relates to the hung-write stale-live-buffer family. Plan: tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md
13. [/] [#jb-run-preempt-followups] #agent-doc-bug follow-ups from plan-jb-run-preempt-autoloop-priority.md (sub-parts 2-4, head-insert shipped 79e0406d): (2) empty-queue autoloop suppression — overlaps #post-clear-auto-queue-suppress; (3) 60s busy ready-wait stall audit when a queue fallback exists; (4) recursion-guard escape hatch for a wedged loop ('recursive direct invocation would deadlock' blocking the one Run Agent Doc that could break the stall) — overlaps #autoloop-command-preemption. Plan: tasks/agent-doc/plan-jb-run-preempt-autoloop-priority.md
14. [ ] [#monsterrod-writeback-post-contamination-confirm] [recommended] Confirm monsterrodholders.md responses now write to the document reliably after the 1-pane-1-document eviction fix (25ff1116). Evidence it resolved: post-fix commits f1ae73a (00:35) + a53ecee (00:25) landed real response content; only out_of_band_write in ops.log is ~51 days old. If responses still go missing AFTER the contamination fix, capture the specific signature (Codex final-gate vs IPC merge drop vs closeout) — that is a DIFFERENT cause than contamination and routes to existing gated items #codex-final-response-repeat / #codex-final-response-live-verify (plan-codex-final-response-not-written.md).
15. [ ] [#jb-busy-active-turn-stall-live-verify] [recommended] live-verify #jb-run-agent-doc-busy-active-turn-stall end-to-end: with prompt-bearing drift present in a doc whose Codex pane is mid-turn, click JB Run Agent Doc and confirm the session-still-running notification fires IMMEDIATELY (logs route_dispatch_only_busy_active_turn_skip_wait) instead of after a 60s stall. Deterministic core shipped b93b3c3c (skip busy ready-wait on proven active-turn busy_proof_line); could not repro the exact prompt-drift-while-busy path non-disruptively on the live monsterrodholders pane this session.
16. [ ] [#adoc-route-busy-fix-superproject-pointer-push] superproject main pointer push for the #jb-run-agent-doc-busy-active-turn-stall fix is DEFERRED (local commit 13600139e): pull --rebase conflicts on src/boost-client submodule pointer (concurrent-session shared-checkout divergence). Submodule fix b93b3c3c is pushed to origin/20260422/codex-agent-backend; land the superproject pointer on a clean cycle once boost-client divergence resolves.
17. [ ] [#test-submodule-worktree-git-identity] #agent-doc-bug: agent-doc tests that git-commit inside a checked-out submodule worktree (super/.git/modules/<sub>) don't set user.email/user.name there, so they pass locally (dev has global identity) but fail CI exit-128 — a green-local/red-CI flake. Fix at the test level: set the two config lines in the submodule worktree before the commit. Live example: flow::closeout::tests::classify_recovery_nested_parent_pointer_stale_when_submodule_ahead_of_parent (closeout.rs ~1383). Same class as the codex_final_gate test-bin-PATH flake fixed this session (0f7607a2). Replaces the deleted feedback_submodule_worktree_test_git_identity memory workaround.
18. [ ] [#multi-session-shared-build-isolation] #agent-doc-bug: the multi-session model lets a concurrent session's untracked/uncommitted work break the shared src/agent-doc crate build, forcing an isolated git-worktree-inside-src verification dance to ship a fix (committed+CI-green but not live until the other session compiles). Consider session-isolated build/worktree isolation so concurrent agent-doc sessions don't block each other's build/install. Replaces the deleted feedback_verify_via_worktree_inside_src memory workaround.
19. [ ] [#sync-fast-pane-hotpath] #agent-doc-bug: editor file-switch sync blocks because the fast pane-focus (safe_passive_focus_actor_after_sync_lock, sync.rs:467) runs AFTER the sync lock (SYNC_LOCK_WAIT_BUDGET 3s) + stacked reconcile budgets (router 1s, ownership proof 750ms, prune 1s). The claiming pane is usually already known (cheap cached read) but showing it is serialized behind lock contention + heavy reconcile. Fix: pre-lock fast pane resolution from the lock-free local projection + select_pane immediately (≤300ms target), create-on-miss without blocking, defer prune/router/ownership-proof/repair to a background reconcile, and make the fast select idempotent with the reconciler's final placement (race-safe; the lock guards concurrent stash/swap). Plan: tasks/agent-doc/plan-sync-pipeline-fast-pane-hotpath.md
20. [ ] [#queue-dedup-destroys-intentional-duplicates] #agent-doc-bug: queue dedup destroys intentional duplicate prompts. dedup_live_prompts (queue.rs:631, run every preflight at preflight.rs:3562) collapses all same-text live prompts, assuming duplicates are always IPC-drift merge artifacts — but users legitimately queue a command twice (user's two `- deploy` items; the 2nd was dropped mid-run). At preflight, user-added vs merge-created duplicates are structurally identical (provenance lost); snapshot-count / adjacency / 3+-copy heuristics all fail the uncommitted-2nd-deploy case. Fix at the merge source (provenance-aware) or gate dedup on a proven IPC-drift state — do NOT delete dedup (reintroduces #adoc-queue-ipc-drift growth). Plan: tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
