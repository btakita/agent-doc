---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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

Completed #md-ast-mutations-node-keyed: node-keyed markdown AST mutations shipped in src/agent-doc 316681b0; current agent-doc main is 6a017bea and superproject pointer pushed as 1b933127b. Superproject Agent Doc workflow could not start because GitHub reported a billing/spending-limit runner allocation blocker.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-091343.md`*

Compacted content:
- Archived 3 response topic(s): do [#md-ast-document-model]; closeout repair - gpt-5; do [#recursion-guard-wedge-escape] -- gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#recursion-guard-wedge-escape]. #spec-test-build-install-commit-push
### Re: do [#recursion-guard-wedge-escape] -- gpt-5

Closed as shipped. Part 2 is present in `src/agent-doc` commit `91f06782` (`Add force interrupt-clear wedge escape`), which is included in current `origin/main` at `9df1244f`.

Proof checked in source:

- `agent-doc session interrupt-clear <FILE> --force` is exposed in `src/main.rs`.
- `force_interrupt_clear` closes actor state, removes matching session registry projections, signals child/supervisor PIDs, kills the selected owner pane, removes the supervisor socket, writes clear cooldown, and reclaims orphaned empty preflight cycles.
- `specs/07-core-commands.md` pins the recursive-start guard and the one-command destructive hatch.

Verification run now:

```
cargo test -p agent-doc force_interrupt_clear_summary_reports_destructive_cleanup -- --nocapture
cargo test -p agent-doc-orchestration recursive_start_diagnostic_refuses_and_names_out_of_pane_recovery -- --nocapture
cargo test -p agent-doc-orchestration owned_pane_queue_wedge_halt_diagnostic_names_halt_and_both_recoveries -- --nocapture
```

All three passed. Source CI for `src/agent-doc` `9df1244f` was already green in run `27088129850`, so this was a stale backlog/queue mirror and is now done.

### Re: do [#partial-staging-closeout-guard] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#partial-staging-closeout-guard]


Shipped and closed.

Implemented `session-check` partial-staging detection in `src/agent-doc` commit `c87f64b6` (`Warn on partial staging closeouts`). The guard inspects the active repo plus dirty submodule repos surfaced by the owning worktree, compares the latest commit against tracked dirty/staged companion changes, and warns only when related source/test paths share changed string literals. That targets the failure mode where local verification passed against a dirty worktree while CI saw only a partial commit.

Spec and regression coverage were updated in `SPEC.md`, `specs/09-git-integration.md`, `agent-doc-orchestration/src/session_check.rs`, and `tests/test_cli.rs`. The superproject pointer is committed in `47bc0ccb7`.

Verification:

```text
cargo test -p agent-doc-orchestration partial_staging_closeout_guard_warns_on_dirty_companion_test_literal -- --nocapture
cargo test flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture
make check
make install
```

The first `make check` rerun exposed one sync test failure plus one IPC timeout during nextest cancellation; both passed in isolation. A clean full `make check` rerun then passed: 3892 tests passed, 233 skipped, doctests passed, and the ignored medium SimWorld check passed. `make install` replaced `/home/brian/.cargo/bin/agent-doc` and installed `libagent_doc-0.34.0.so`.
<!-- agent:boundary:23fcebe5 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#md-ast-document-model]. #spec-test-build-install-commit-push~~
- ~~:round_pushpin: do [#partial-staging-closeout-guard]~~
- :round_pushpin: do [#codex-mcp-server-root-fix]
- :round_pushpin: do [#live-verify-batch]
- :round_pushpin: do [#automated-completion-detection]
- :round_pushpin: do [#monsterrod-queue-items-deleted-live]
- :round_pushpin: do [#clear-blocked-by-own-drafted-clear-command]
- :round_pushpin: do [#tmux-status-turn-in-progress]
- :round_pushpin: do [#stale-busy-self-heal-live-verify]
- do [#jb-file-cache-conflict-syntax-loss]
- do [#jb-busy-active-turn-stall-live-verify]
- do [#adoc-route-busy-fix-superproject-pointer-push]
- do [#multi-session-shared-build-isolation]
- do [#sync-fast-pane-hotpath]
- ~~do [#recursion-guard-wedge-escape]~~
- ~~do [#queue-dedup-destroys-intentional-duplicates]~~
- do [#jb-run-preempt-followups]
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
1. [ ] [#md-ast-document-model] #agent-doc-bug PARTIAL SHIPPED 9df1244f: runtime writes now persist structured .overlay.yrs sidecars beside legacy .yrs from the same markdown snapshot, and preflight queue dedup is markdown-AST node-keyed so intentional duplicate do/free-text prompts survive. REMAINING: legacy text CRDT is still the merge base; editor/IPC plugins still need the authoritative node_patches apply path and realtime watcher convergence. Plan: tasks/agent-doc/plan-md-ast-document-model.md
2. [ ] [#jb-file-cache-conflict-syntax-loss] #agent-doc-bug REOPENED false closeout: previous closeout verified queue-consume behavior, not JetBrains File Cache Conflict syntax-highlighting loss; define and verify the actual cache-conflict syntax preservation path before closing.
3. [ ] [#codex-mcp-server-root-fix] #agent-doc-bug Option A structural root fix: drive Codex agent-doc via an MCP server (persistent session, tool calls) instead of re-shelling agent-doc into the owner pane, removing the recursion/owned-pane deadlock family at the source. Plan: tasks/agent-doc/plans/codex-mcp-server-option-a.md
4. [ ] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
5. [ ] [#automated-completion-detection] #agent-doc-bug enhancement: detect item completion from logs/state and retrieval (ops.log proofs, capture/commit records, agent:done archives, semantic match to queued free-text) so the binary auto-reaps satisfied gated/review/queue mirrors and strikes completed opportunistic queue items instead of parking work in agent:review or leaving stale queue heads. Operator wants automated review/queue completion to stop accumulation; example stale queue mirror: completed #editor-nested-bold-italic-rendering / JB+VSCode bold italic rendering work.
6. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
7. [ ] [#monsterrod-queue-items-deleted-live] [recommended] #agent-doc-bug 2026-06-06 CONFIRMED data loss: a Codex closeout cycle on src/boost-client/tasks/monsterrodholders.md net-removed user-added queue items. Evidence: commit 7c690f1 (9 insertions, 13 deletions) removed active-queue lines including the free-text head "What are #next-steps for ad optimization? Today is 2026-06-06." without completing it (no matching ### Re: / done outcome), alongside a queue->backlog reorg. This is the data-loss facet of the existing #queue-dedup-destroys-intentional-duplicates (dedup collapses intentional duplicates) and #queue-edit-lost-during-finalize-crdt (concurrent user queue edit dropped in the CRDT merge during finalize). Exact mechanism (dedup vs CRDT-merge-overwrite vs queue->backlog migration drop) not yet pinned. Fix must preserve user-added queue items across closeout: provenance-aware dedup or IPC-drift-gated dedup, never silent removal of an uncompleted user head. Plans: tasks/agent-doc/plan-queue-dedup-destroys-intentional-duplicates.md; relates review #queue-edit-lost-during-finalize-crdt, #queue-user-edit-overwrite.
8. [ ] [#clear-blocked-by-own-drafted-clear-command] #agent-doc-bug 2026-06-06: `agent-doc session clear <FILE>` is refused because the pane composer holds protected "drafted prompt input" whose tail IS the clear command itself (reason=drafted prompt input, current_command=agent-doc, tail="❯ agent-doc session clear ..."). The guard is self-defeating: the only drafted line blocking the clear is the clear command the operator just typed. Fix: exclude agent-doc control commands (session clear/interrupt-clear/stop) from the protected-prompt set so clear proceeds; keep protecting genuine drafted prompt content. Immediate recovery exists: `agent-doc session interrupt-clear <FILE>`. Plan: tasks/agent-doc/plan-session-clear-blocked-by-own-drafted-command.md
9. [ ] [#tmux-status-turn-in-progress] #agent-doc-bug 2026-06-06: tmux status line is blank while a managed turn is active; should show "agent-doc: turn in progress" when the actor is busy and clear back to idle on ready. Drive it from the supervisor busy<->ready transitions (transition_actor_state + the new idle-queue-watch stale-busy self-heal) so the indicator never diverges from actor state. Overlaps #claude-busy-status-during-active-turn, #jb-stale-busy-idle-footer. Plan: tasks/agent-doc/plan-tmux-status-turn-in-progress.md
10. [ ] [#stale-busy-self-heal-live-verify] [recommended] #agent-doc-bug live-verify the idle-queue-watch stale-busy self-heal (7b60fcdc): drive a managed Codex auto-loop into the wedge (auto_trigger_inject recursive agent-doc -> recursion-guard abandon -> actor left busy over idle composer), then confirm the supervisor logs idle_queue_watch_stale_busy_reconciled within ~2s and the queue resumes draining with no pane kill and no session status/clear. Could not repro the exact prompt-redraw-split wedge non-disruptively this cycle.
11. [/] [#jb-run-preempt-followups] #agent-doc-bug follow-ups from plan-jb-run-preempt-autoloop-priority.md (sub-parts 2-4, head-insert shipped 79e0406d): (2) empty-queue autoloop suppression — overlaps #post-clear-auto-queue-suppress; (3) 60s busy ready-wait stall audit when a queue fallback exists; (4) recursion-guard escape hatch for a wedged loop ('recursive direct invocation would deadlock' blocking the one Run Agent Doc that could break the stall) — overlaps #autoloop-command-preemption. Plan: tasks/agent-doc/plan-jb-run-preempt-autoloop-priority.md
12. [ ] [#jb-busy-active-turn-stall-live-verify] [recommended] live-verify #jb-run-agent-doc-busy-active-turn-stall end-to-end: with prompt-bearing drift present in a doc whose Codex pane is mid-turn, click JB Run Agent Doc and confirm the session-still-running notification fires IMMEDIATELY (logs route_dispatch_only_busy_active_turn_skip_wait) instead of after a 60s stall. Deterministic core shipped b93b3c3c (skip busy ready-wait on proven active-turn busy_proof_line); could not repro the exact prompt-drift-while-busy path non-disruptively on the live monsterrodholders pane this session.
13. [ ] [#adoc-route-busy-fix-superproject-pointer-push] superproject main pointer push for the #jb-run-agent-doc-busy-active-turn-stall fix is DEFERRED (local commit 13600139e): pull --rebase conflicts on src/boost-client submodule pointer (concurrent-session shared-checkout divergence). Submodule fix b93b3c3c is pushed to origin/20260422/codex-agent-backend; land the superproject pointer on a clean cycle once boost-client divergence resolves.
14. [ ] [#multi-session-shared-build-isolation] #agent-doc-bug: the multi-session model lets a concurrent session's untracked/uncommitted work break the shared src/agent-doc crate build, forcing an isolated git-worktree-inside-src verification dance to ship a fix (committed+CI-green but not live until the other session compiles). Consider session-isolated build/worktree isolation so concurrent agent-doc sessions don't block each other's build/install. Replaces the deleted feedback_verify_via_worktree_inside_src memory workaround.
15. [ ] [#sync-fast-pane-hotpath] #agent-doc-bug: editor file-switch sync blocks because the fast pane-focus (safe_passive_focus_actor_after_sync_lock, sync.rs:467) runs AFTER the sync lock (SYNC_LOCK_WAIT_BUDGET 3s) + stacked reconcile budgets (router 1s, ownership proof 750ms, prune 1s). The claiming pane is usually already known (cheap cached read) but showing it is serialized behind lock contention + heavy reconcile. Fix: pre-lock fast pane resolution from the lock-free local projection + select_pane immediately (≤300ms target), create-on-miss without blocking, defer prune/router/ownership-proof/repair to a background reconcile, and make the fast select idempotent with the reconciler's final placement (race-safe; the lock guards concurrent stash/swap). Plan: tasks/agent-doc/plan-sync-pipeline-fast-pane-hotpath.md
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
