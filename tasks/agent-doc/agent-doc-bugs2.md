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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-022559.md`*

Compacted content:
- Archived 4 response topic(s): `~~`-not-struck; verify [#late-ipc-patch-plugin-apply-fence] in logs; can't run monsterrodholders.md (recursion wedge); 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: drain queue — #recursion-guard-wedge-escape part 1 (start recursion guard) — opus-4-8

> **Queue prompt:**
>
> :pushpin: Every JB `File Cache Conflict` that is applied causes the md file to lose its plugin syntax highlighting...falling back to the default markdown syntax highlighting. 


You chose **drain queue now**, so I took the top backlog item (`#recursion-guard-wedge-escape`, backlog priority #1) and shipped **part 1**.

**The gap.** `run` (`agent-doc <FILE>`) already fails closed on a recursive self-owned-pane invocation, but the `start` entry did not. `existing_session_pane_action_from_entry` returns `None` when the current pane is the owner (intentional, for legitimate same-pane restart), so `agent-doc start <FILE>` run *inside the Codex pane that already owns the doc* fell through and spawned a replacement owner — looping re-injection of `agent-doc <FILE>` into the owner pane. That is the wedge with no clean operator escape.

**The fix (f24889fc, origin/main).**
- New `run::recursive_codex_start_invocation_diagnostic` — a Codex-only owner-pane self-invocation detector (gated on `detect_harness() == "codex"`, so a legitimate restart from a bare shell after the supervisor exits is **not** blocked).
- `start::run_with_reap_policy` now calls it right after resolving the pane, *before* any pane relocation or owner spawn, and bails. It fires **unconditionally** (even under `--force`) because the deadlock is inherent to same-pane nesting.
- Bail logs `start_recursive_self_owned_pane_refused` and prints a one-line out-of-pane recovery: reconcile a possibly stale-busy actor with `agent-doc session status <FILE>`, then if genuinely wedged run `agent-doc session interrupt-clear <FILE>` from a *different* pane.
- Unit test on the message; spec note in `specs/07-core-commands.md`. `make check` green, `cargo build --release` + `cargo install --path .` both done so the fix is live.

**Why the item stays open (part 2 remaining).** The bug also asks for a `session clear`/`interrupt-clear` **force path** that kills the wedged owner pane + supervisor PID and clears the registry in one command — a destructive change warranting its own turn + live verification. Re-scoped the backlog item to that remaining work.

**Closeout churn — this turn reproduced your bug live.** While I was working, you edited the queue (finished the line-43 note, removed the duplicate, added two new bug reports, removed `#recursion-guard-wedge-escape` from the queue). The `finalize` tripped `live_prompt_drift_after_preflight`, the cycle got stuck in `response_captured` with a stale 19841-byte snapshot vs your 17146-byte cleaned file, and I recovered via `reset --from-current --preserve-session` + this `write --commit` repair (deliberately avoiding queue-consume so your current queue head — the `File Cache Conflict` bug report — was **not** struck). Your queue edits are all preserved. This is direct evidence for `#queue-dedup-destroys-intentional-duplicates` / `#monsterrod-queue-items-deleted-live`: a live queue edit during an open cycle still forces a stale-snapshot recovery dance.

### Re: Would an AST model of the md file (like tsift) be more reliable / IPC-sharable? — opus-4-8

Yes — and I think it is the **root-cause fix** for the whole data-loss family you keep hitting (queue edits dropped, dedup destroying intentional duplicates, the strike that hit your `File Cache Conflict` item last turn, the `live_prompt_drift_after_preflight` stuck cycles).

The common cause of all of those: agent-doc mutates the document as **text**. Queue-consume strikes "the leading free-text *line*", dedup collapses same-*text* *lines*, and IPC ships *text* patches that fail when the live buffer drifted. None of those operations know *which item* they are acting on — only what the text looks like — so a concurrent edit makes them act on the wrong thing.

An AST / structured model fixes that at the source:
- Model queue items, backlog items, and exchange turns as **typed nodes with stable ids**, not lines. Then "consume the dispatched head" strikes *that node by id*, dedup compares *node identity/provenance* (not text), and reorder/enqueue are node ops. The bug where I struck a head I never answered becomes structurally impossible.
- **IPC sharability:** a patch becomes `{node-id, operation}` instead of a text match against the buffer. That is exactly what survives live-buffer drift — the failure mode that wedged us twice this session.
- You already have most of the substrate: `crdt.rs` is yrs-based and `component.rs` parses the `&lt;!-- agent:name --&gt;` overlay. The gap is that the CRDT operates on **text**, not a structured schema. The work is a markdown block AST (comrak / pulldown-cmark, or reuse tsift's markdown-ast) + the agent-component overlay as typed nodes, backed by a structured (map/array) CRDT.

tsift uses tree-sitter for *code*; for markdown the analog is a markdown block AST plus your component semantics. Captured as `#md-ast-document-model` ([recommended], top of backlog) — I framed it explicitly as the root cause so it is weighed against patching each text bug individually.

### Re: :inbox_tray:/enqueue without the `queue` attr, via a realtime watcher — opus-4-8

Captured as `#queue-enqueue-action`. The feature itself is small (an enqueue action that moves a `[#task-id]` from backlog/icebox into `agent:queue` directly, applying `:pushpin:`), but the **realtime/watcher** half you want is the hard part and it depends on the AST model above: a watcher reacting to "the user typed/pasted a backlog item" is only reliable if changes are **node-keyed CRDT events**, not text-diff guesses — otherwise the watcher inherits the same drift/dedup races. So I sequenced it as: `#md-ast-document-model` first (makes realtime reliable), then `#queue-enqueue-action` on top of it. The non-realtime version (explicit `agent-doc enqueue [#id]`) could ship independently sooner if you want a quick win.

### Re: the queued `do` items (#ipc-listener-dewedge-direct-disk, #jb-file-cache-conflict-syntax-loss, #recursion-guard-wedge-escape part 2) — opus-4-8

I deliberately did **not** batch those three code changes into this turn. Two reasons: (1) your active input this turn was these design questions, and the AST decision plausibly changes *how* several of them get fixed (e.g. the dewedge IPC work overlaps `#md-ast-document-model`); (2) every response-bearing commit on this active go-mode queue strikes a free-text head — batching multiple risky changes while your free-text bug reports sit at the queue head is how items get lost. They remain queued. Tell me which to take first (or just let the queue drain once you are comfortable), and I will do them one focused turn each. My recommendation: decide `#md-ast-document-model` first, since it may absorb several of the others.
<!-- agent:boundary:97794a54 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- :pushpin: Support :inbox_tray:/**enqueue** to add a backlog or icebox item ([#task-id]) into the queue without the need for `queue` to be set on `agent:backlog`. Ideally this action occurs from a watcher that can pick up this change when it happens and/or document events (CRDT of the document model) as the user types (or pastes) on the backlog item. I want to move agent-doc to be realtime, so a watcher can be valuable if it is reliable.
- :pushpin: A backlog with `queue` attribute should immediately add to the queue. Prioritize both using auto-dag if `priority` attribute is present. A free-text addition to a queue should be have :pushpin: applied. Also, the queue items, if they were prioritized, are missing the :round_pushpin: annotation.
- :pushpin: The previous and next items in the queue I was editing were uncommitted deleted by agent-doc.
- :pushpin: Completed items were not marked as completed in the queue...such as "The JB + VSCode plugin should bold any markdown that is **bold**, italicize any markdown that is *italicized*, and bold + italicize any markdown that is **_bold italicized_**."...which was opportunistically (good behavior that I want to encourage...with the help of auto-dag perhaps?) picked up in a previous turn.
- do [#ipc-listener-dewedge-direct-disk]
- do [#jb-busy-active-turn-stall-live-verify]
- do [#adoc-route-busy-fix-superproject-pointer-push]
- do [#test-submodule-worktree-git-identity]
- do [#multi-session-shared-build-isolation]
- do [#sync-fast-pane-hotpath]
- do [#queue-dedup-destroys-intentional-duplicates]
- do [#ipc-typing-duplication-family]
- do [#codex-mcp-server-root-fix]
- do [#live-verify-batch]
- do [#automated-completion-detection]
- do [#monsterrod-queue-items-deleted-live]
- do [#clear-blocked-by-own-drafted-clear-command]
- do [#tmux-status-turn-in-progress]
- do [#stale-busy-self-heal-live-verify]
- do [#partial-staging-closeout-guard]
- do [#jb-file-cache-conflict-syntax-loss]
- do [#recursion-guard-wedge-escape]
- Would it also make sense for agent-doc to have an AST model of the md file...like what tsift does? Would an AST model be more reliable and sharable with IPC?
- Support :inbox_tray:/**enqueue** to add a backlog or icebox item into the queue. Ideally this action occurs from a watcher that can pick up
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
1. [ ] [#queue-enqueue-action] #agent-doc-bug feature: support :inbox_tray:/enqueue to add a backlog or icebox item ([#task-id]) into agent:queue WITHOUT requiring the queue attribute on agent:backlog. Ideally a realtime watcher picks up the document change as the user types or pastes (CRDT document events on the backlog item), moving agent-doc toward realtime. Depends on a reliable watcher; relates #md-ast-document-model (a node-keyed model is what makes the watcher reliable). Today enqueue requires the queue attr plus a preflight backlog->queue sync.
2. [ ] [#md-ast-document-model] [recommended] #agent-doc-bug ROOT-CAUSE candidate for the queue/dedup/live-buffer data-loss family: model the session document as a structured AST plus a structured CRDT (queue items, backlog items, exchange turns as typed nodes with stable ids) instead of text lines. Today queue-consume strikes the leading free-text LINE, dedup collapses same-text LINES, and IPC sends text patches that fail on live-buffer drift (live_prompt_drift_after_preflight) — all because mutations operate on TEXT not identified nodes. A node-keyed model makes consume/dedup/reorder/enqueue operate on the dispatched node by id, and makes IPC patches drift-resilient (operation = node-id plus op rather than text match). yrs CRDT already exists in crdt.rs but operates on text; the gap is a structured schema. Use a markdown block AST (comrak or pulldown-cmark, or the tsift markdown-ast) plus the agent-component overlay. Enables the realtime watcher in #queue-enqueue-action. Plan TBD.
3. [ ] [#jb-file-cache-conflict-syntax-loss] #agent-doc-bug Every JB File Cache Conflict that is applied causes the .md file to lose its plugin syntax highlighting, falling back to default markdown highlighting. RESTORED: this user free-text queue head was wrongly struck by queue-consume during the #recursion-guard-wedge-escape turn (queue-consume strikes the leading free-text head on ANY active-queue commit, even when the committed response addressed a different item). Evidence for #queue-dedup-destroys-intentional-duplicates and the queue-edit-loss family. Fix: queue-consume must only strike a head the turn actually answered, not any free-text head.
4. [ ] [#recursion-guard-wedge-escape] [recommended] #agent-doc-bug PART 1 SHIPPED (f24889fc origin/main): the `agent-doc start` entry now hard-refuses a recursive self-owned-pane start (run::recursive_codex_start_invocation_diagnostic, Codex-only via detect_harness, fires even under --force) instead of spawning a looping replacement owner; bail logs start_recursive_self_owned_pane_refused and prints an out-of-pane recovery path. REMAINING part 2: `agent-doc session clear`/`interrupt-clear` must offer a force path that kills the wedged owner pane + supervisor PID and clears the sessions registry in one command (today recovery is the multi-step session status then interrupt-clear from another pane). Overlaps #autoloop-command-preemption, plan-jb-run-preempt-autoloop-priority.md sub-part 4; relates #clear-blocked-by-own-drafted-clear-command.
5. [ ] [#partial-staging-closeout-guard] [recommended] #agent-doc-bug closeout guard for partial change-set staging: a turn that edits source + tests can commit a partial set (e.g. queue.rs ~~ render change committed but its test-assertion fixes left unstaged) so local make check passes against the working tree while CI fails against the committed tree only. Observed live this session: 1645112d CI red, fixed dcb734eb. Add a heuristic/guard that flags when committed output-format changes leave companion test files (matching changed string literals) unstaged, or warns the staged diff is not self-consistent. Same "regressions when I work on agent-doc" class.
6. [ ] [#ipc-typing-duplication-family] #agent-doc-bug consolidate IPC/typing full-document duplication prevention (was review #ipcfullprompt-recur2, #finalize-retry-ipc-response-duplication, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing). PatchWatcher.setText must not duplicate/delete the full document/tail while a prompt is typed; needs JB debug-log setText capture + SimWorld regression + pluginVersion bump. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md
7. [ ] [#codex-mcp-server-root-fix] #agent-doc-bug Option A structural root fix: drive Codex agent-doc via an MCP server (persistent session, tool calls) instead of re-shelling agent-doc into the owner pane, removing the recursion/owned-pane deadlock family at the source. Plan: tasks/agent-doc/plans/codex-mcp-server-option-a.md
8. [ ] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
9. [ ] [#automated-completion-detection] #agent-doc-bug enhancement: detect item completion from logs/state (ops.log proofs, capture/commit records) so the binary auto-reaps satisfied gated items instead of parking work in agent:review for a human. Operator wants automated review to stop review-item accumulation.
10. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
11. [ ] [#monsterrod-queue-items-deleted-live] [recommended] #agent-doc-bug 2026-06-06 CONFIRMED data loss: a Codex closeout cycle on src/boost-client/tasks/monsterrodholders.md net-removed user-added queue items. Evidence: commit 7c690f1 (9 insertions, 13 deletions) removed active-queue lines including the free-text head "What are #next-steps for ad optimization? Today is 2026-06-06." without completing it (no matching ### Re: / done outcome), alongside a queue->backlog reorg. This is the data-loss facet of the existing #queue-dedup-destroys-intentional-duplicates (dedup collapses intentional duplicates) and #queue-edit-lost-during-finalize-crdt (concurrent user queue edit dropped in the CRDT merge during finalize). Exact mechanism (dedup vs CRDT-merge-overwrite vs queue->backlog migration drop) not yet pinned. Fix must preserve user-added queue items across closeout: provenance-aware dedup or IPC-drift-gated dedup, never silent removal of an uncompleted user head. Plans: tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md; relates review #queue-edit-lost-during-finalize-crdt, #queue-user-edit-overwrite.
12. [ ] [#clear-blocked-by-own-drafted-clear-command] #agent-doc-bug 2026-06-06: `agent-doc session clear <FILE>` is refused because the pane composer holds protected "drafted prompt input" whose tail IS the clear command itself (reason=drafted prompt input, current_command=agent-doc, tail="❯ agent-doc session clear ..."). The guard is self-defeating: the only drafted line blocking the clear is the clear command the operator just typed. Fix: exclude agent-doc control commands (session clear/interrupt-clear/stop) from the protected-prompt set so clear proceeds; keep protecting genuine drafted prompt content. Immediate recovery exists: `agent-doc session interrupt-clear <FILE>`. Plan: tasks/agent-doc/plan-session-clear-blocked-by-own-drafted-command.md
13. [ ] [#tmux-status-turn-in-progress] #agent-doc-bug 2026-06-06: tmux status line is blank while a managed turn is active; should show "agent-doc: turn in progress" when the actor is busy and clear back to idle on ready. Drive it from the supervisor busy<->ready transitions (transition_actor_state + the new idle-queue-watch stale-busy self-heal) so the indicator never diverges from actor state. Overlaps #claude-busy-status-during-active-turn, #jb-stale-busy-idle-footer. Plan: tasks/agent-doc/plan-tmux-status-turn-in-progress.md
14. [ ] [#stale-busy-self-heal-live-verify] [recommended] #agent-doc-bug live-verify the idle-queue-watch stale-busy self-heal (7b60fcdc): drive a managed Codex auto-loop into the wedge (auto_trigger_inject recursive agent-doc -> recursion-guard abandon -> actor left busy over idle composer), then confirm the supervisor logs idle_queue_watch_stale_busy_reconciled within ~2s and the queue resumes draining with no pane kill and no session status/clear. Could not repro the exact prompt-redraw-split wedge non-disruptively this cycle.
15. [ ] [#ipc-listener-dewedge-direct-disk] #agent-doc-bug detect a wedged/slow IPC listener (repeated 2s ack timeouts) and move the doc to direct-disk writes for the rest of the session so it cannot apply patches minutes late. Relates to the hung-write stale-live-buffer family. Plan: tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md
16. [/] [#jb-run-preempt-followups] #agent-doc-bug follow-ups from plan-jb-run-preempt-autoloop-priority.md (sub-parts 2-4, head-insert shipped 79e0406d): (2) empty-queue autoloop suppression — overlaps #post-clear-auto-queue-suppress; (3) 60s busy ready-wait stall audit when a queue fallback exists; (4) recursion-guard escape hatch for a wedged loop ('recursive direct invocation would deadlock' blocking the one Run Agent Doc that could break the stall) — overlaps #autoloop-command-preemption. Plan: tasks/agent-doc/plan-jb-run-preempt-autoloop-priority.md
17. [ ] [#jb-busy-active-turn-stall-live-verify] [recommended] live-verify #jb-run-agent-doc-busy-active-turn-stall end-to-end: with prompt-bearing drift present in a doc whose Codex pane is mid-turn, click JB Run Agent Doc and confirm the session-still-running notification fires IMMEDIATELY (logs route_dispatch_only_busy_active_turn_skip_wait) instead of after a 60s stall. Deterministic core shipped b93b3c3c (skip busy ready-wait on proven active-turn busy_proof_line); could not repro the exact prompt-drift-while-busy path non-disruptively on the live monsterrodholders pane this session.
18. [ ] [#adoc-route-busy-fix-superproject-pointer-push] superproject main pointer push for the #jb-run-agent-doc-busy-active-turn-stall fix is DEFERRED (local commit 13600139e): pull --rebase conflicts on src/boost-client submodule pointer (concurrent-session shared-checkout divergence). Submodule fix b93b3c3c is pushed to origin/20260422/codex-agent-backend; land the superproject pointer on a clean cycle once boost-client divergence resolves.
19. [ ] [#test-submodule-worktree-git-identity] #agent-doc-bug: agent-doc tests that git-commit inside a checked-out submodule worktree (super/.git/modules/<sub>) don't set user.email/user.name there, so they pass locally (dev has global identity) but fail CI exit-128 — a green-local/red-CI flake. Fix at the test level: set the two config lines in the submodule worktree before the commit. Live example: flow::closeout::tests::classify_recovery_nested_parent_pointer_stale_when_submodule_ahead_of_parent (closeout.rs ~1383). Same class as the codex_final_gate test-bin-PATH flake fixed this session (0f7607a2). Replaces the deleted feedback_submodule_worktree_test_git_identity memory workaround.
20. [ ] [#multi-session-shared-build-isolation] #agent-doc-bug: the multi-session model lets a concurrent session's untracked/uncommitted work break the shared src/agent-doc crate build, forcing an isolated git-worktree-inside-src verification dance to ship a fix (committed+CI-green but not live until the other session compiles). Consider session-isolated build/worktree isolation so concurrent agent-doc sessions don't block each other's build/install. Replaces the deleted feedback_verify_via_worktree_inside_src memory workaround.
21. [ ] [#sync-fast-pane-hotpath] #agent-doc-bug: editor file-switch sync blocks because the fast pane-focus (safe_passive_focus_actor_after_sync_lock, sync.rs:467) runs AFTER the sync lock (SYNC_LOCK_WAIT_BUDGET 3s) + stacked reconcile budgets (router 1s, ownership proof 750ms, prune 1s). The claiming pane is usually already known (cheap cached read) but showing it is serialized behind lock contention + heavy reconcile. Fix: pre-lock fast pane resolution from the lock-free local projection + select_pane immediately (≤300ms target), create-on-miss without blocking, defer prune/router/ownership-proof/repair to a background reconcile, and make the fast select idempotent with the reconciler's final placement (race-safe; the lock guards concurrent stash/swap). Plan: tasks/agent-doc/plan-sync-pipeline-fast-pane-hotpath.md
22. [ ] [#queue-dedup-destroys-intentional-duplicates] #agent-doc-bug: queue dedup destroys intentional duplicate prompts. dedup_live_prompts (queue.rs:631, run every preflight at preflight.rs:3562) collapses all same-text live prompts, assuming duplicates are always IPC-drift merge artifacts — but users legitimately queue a command twice (user's two `- deploy` items; the 2nd was dropped mid-run). At preflight, user-added vs merge-created duplicates are structurally identical (provenance lost); snapshot-count / adjacency / 3+-copy heuristics all fail the uncommitted-2nd-deploy case. Fix at the merge source (provenance-aware) or gate dedup on a proven IPC-drift state — do NOT delete dedup (reintroduces #adoc-queue-ipc-drift growth). Plan: tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
1. [ ] [#late-ipc-patch-plugin-apply-fence] #agent-doc-bug SHIPPED (9eee5255 origin/main; plugin 0.2.149 signed): PatchWatcher apply-side generation fence drops a superseded queued patch (baseline drift OR same-cycle already committed), fail-open. Remaining USER live gate (folded into #live-verify-batch): install agent-doc-jetbrains-0.2.149-signed.zip + confirm idea.log "generation fence: dropping superseded patch". Plan: tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
