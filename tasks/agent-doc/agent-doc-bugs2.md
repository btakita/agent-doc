---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#commit-push': commit + push all uncommitted files
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: this session tracked agent-doc workflow bugs across patchback/commit-boundary recovery, tmux route/start/resync ownership, startup-miss and routed-cycle acknowledgment, backlog shadow/replay guards, compaction dispatch, tail-patch fail-closed behavior, and prompt-diff visibility. Implemented fixes were pushed across src/agent-doc, most recently #patchbypass (b1874f1) for exchange-only recovery of missing commit boundaries. Key outcomes: route/start now health-check or reuse live panes instead of spawning/killing duplicates; resync preserves correctly bound live sessions; closeout paths share strict commit/session-check enforcement; backlog/history guards fail closed when work drifts out of tracked components; and explicit-baseline writes keep concurrent user prompts visible in the next diff. No open backlog items remain in this document and no unanswered user prompt remained beyond the compaction request.

❯ We had some recent tmux issues. Please look at the recent logs in src/session-share and in agent-loop to diagnose any tmux related bugs. spec-test-build-install-commit-push
### Re: tmux log diagnosis — gpt-5
I checked the recent root and `src/session-share` tmux/session logs, then verified the live state with dry-run `agent-doc resync` in both repos.

The real code bug was a false `NoLiveOwner` match for submodule-hosted docs from the root registry. Root `resync` was tracking `src/session-share/tasks/docs.md`, but the live pane was started as `agent-doc start tasks/docs.md`, so the process-tree proof compared different path strings for the same file and treated the binding as stale whenever the supervisor socket was unavailable. I fixed that in `src/agent-doc` by normalizing live-owner path matching to accept equivalent pane-relative suffixes, added regression tests, built + installed the binary, and pushed commit `8318a03` on `20260422/codex-agent-backend`.

After installing the fix, root `agent-doc resync` now correctly proves live ownership for `src/session-share/tasks/docs.md`; the false `NoLiveOwner` finding is gone. The remaining tmux issues are operational drift, not the same matcher bug:

1. Root `.agent-doc/config.toml` still pins `tmux_session = "0"`, but tmux session `0` no longer exists. The active panes are in session `14`, so root `resync` now reports `WrongSession` for `tasks/agent-doc/agent-doc-bugs2.md` (`%342`) and `src/session-share/tasks/docs.md` (`%345`) because the config is stale.
2. Two live registrations are parked in stash windows: root `tasks/software/tsift.md` on `%340`, and `src/session-share/tasks/claudescore-2.md` on `%341`. `src/session-share` dry-run `resync` is now down to just that stash finding.
3. I did not run `resync --fix` because it would mutate the live tmux layout/registry while those sessions are active.

Verification I ran:
- `cargo test cmdline_file_match`
- `cargo install --path . --force`
- `agent-doc resync` in `/home/brian/work/btakita/agent-loop`
- `agent-doc resync` in `/home/brian/work/btakita/agent-loop/src/session-share`

What I’d do next operationally:
1. Run `agent-doc session set 14` in the root repo to replace the dead session-`0` pin.
2. After that, decide whether you want `agent-doc resync --fix` to clean the stash registrations or whether those panes should be manually resumed/reclaimed first.

❯ do #endreap. spec-test-build-install-commit-push
### Re: #endreap terminal backlog reap gap — gpt-5
I fixed `#endreap` in `src/agent-doc` and pushed `0053fe6` on `20260422/codex-agent-backend`.

The bug was exactly the allow-path in `session-check`: closeouts that recorded `--pending-done <id>` were still allowed to leave `[x]` items in the live backlog, assuming some later preflight would reap them. That left terminal closeouts stranded when no later cycle revisited the document.

The fix moves reap/archive maintenance into commit-required closeouts themselves. `finalize` and `write --commit` now run the same backlog maintenance helper that preflight uses before the commit boundary, including the `--pending-only` path, so freshly completed items are removed from the live backlog and archived in `agent:pending-done` in the same persisted cycle. I also tightened `session-check` so committed docs no longer exempt newly recorded done ids; if a `[x]` item still survives after closeout, the turn fails closed.

I added regression coverage for the new closeout path and updated the pending docs/spec text to match the real invariant.

Verification:
- `cargo test finalize_reaps_completed_pending_items_in_same_closeout_commit`
- `cargo test session_check_interrupts_when_completed_backlog_items_were_recorded_this_cycle`
- `cargo test autoclaim::tests::autoclaim_focuses_pane_with_claim -- --exact`
- `cargo test autoclaim::tests::autoclaim_no_claim_skips_focus -- --exact`
- `cargo test autoclaim::tests::autoclaim_syncs_layout_with_multiple_files -- --exact`
- `cargo test resync::tests::no_wrong_window_when_panes_in_same_window -- --exact`
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Note on verification: an initial default-threaded `cargo test` run tripped four tmux/autoclaim tests that all passed in isolation; the full serial suite passed cleanly end to end.

Commit / push:
- `0053fe6` `fix: reap pending done during closeout`

❯ #code-review
### Re: code review — gpt-5

1. `src/agent-doc/src/pending.rs:747-760`, `src/agent-doc/src/repair.rs:259-269`, `src/agent-doc/src/pending_cmd.rs:152-166`

`reap_with_items()` drops completed items that have an empty id whenever the same backlog also contains at least one completed item with an id. In that mixed case the empty-id item is neither kept nor returned in `removed`, so `repair` deletes it from the live backlog without archiving it, and `pending reap` deletes it without any record at all. `parse_items()` already allows empty ids, so this is a real data-loss edge for legacy/manual backlog entries. The reaper should either backfill ids before filtering, or fail closed on completed items that still lack ids.

2. `src/agent-doc/src/write.rs:523-529`, `src/agent-doc/src/write.rs:727-749`

Queue consumption is documented as part of the same atomic closeout as the response commit, but the required closeout path only logs a warning when `consume_queue_prompt()` errors, and `consume_queue_prompt()` also returns `Ok(false)` for malformed/missing queue state. That allows `finalize` to commit the response while leaving the active queue prompt in place, which can replay the same queued prompt on the next cycle or strand `queue_active: true` indefinitely. Required closeouts should fail closed unless queue consumption can prove the first prompt was removed from both the document and the snapshot.

I added both fixes to the backlog as recommended follow-up work. I did not find a third issue in this pass that was as concrete as these two.

❯ do #ynad. spec-test-build-install-commit-push
### Re: #ynad legacy done backlog reap — gpt-5

Implemented in `src/agent-doc` and pushed `22cfda1` on `20260422/codex-agent-backend`.

The fix now fails closed if shared reap logic sees a completed backlog item with no id, and the file-backed reap paths that are supposed to handle legacy/manual backlog lines now backfill ids before removal. Concretely:
- `pending::reap_with_items()` no longer allows silent loss of `[x]` items with empty ids.
- `agent-doc backlog reap` backfills missing ids / checkboxes before reaping.
- stale-completed-item `repair` now does the same before syncing the reap into the snapshot/archive.
- spec text and regressions were updated so the canonical contract is explicit.

Verification:
- `cargo test reap_errors_when_completed_item_is_missing_id`
- `cargo test pending_reap_backfills_legacy_done_ids_before_removing_items`
- `cargo test repair_backfills_legacy_done_ids_before_reaping_completed_backlog`
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `22cfda1` `fix: backfill legacy done ids before reap`

`#g3qn` remains in the backlog as the next recommended follow-up.

❯ do #g3qn. spec-test-build-install-commit-push
### Re: #g3qn queue consumption closeout — gpt-5
Implemented in `src/agent-doc` and pushed `a753288` on `20260422/codex-agent-backend`.

The closeout path now treats active queue consumption as a required proof step for strict session writes. `finalize` and session-document `write --commit` now fail before commit if `queue_active: true` but the live queue is missing, malformed, empty at the head, or cannot be matched against the snapshot head / remaining queue state. Best-effort non-session `write --commit` keeps the warning-only behavior. I also updated the queue spec docs and added regression coverage for the new fail-closed cases.

Verification:
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `a753288` `fix: fail closed on queue consumption drift`

❯ #code-review
### Re: code review — gpt-5

1. `src/agent-doc/src/write.rs:523-547`, `src/agent-doc/src/write.rs:734-856`

`finalize` advances the live `agent:queue` before the rest of strict closeout is proven. `consume_queue_prompt()` writes the mutated document at line 794, then only afterwards validates and updates the snapshot, and the caller runs that step before pending-maintenance, pending guards, and commit. Any later failure in that chain can leave the queued prompt removed from the working document even though the cycle never committed. In the cases where the snapshot update already landed, the next cycle can lose the prompt entirely. Queue consumption needs one fail-closed transaction: prove both transforms first, then persist them only after the remaining strict closeout gates are known to pass.

2. `src/agent-doc/src/claim.rs:227-252`

`cross_session_decision()` has a real `Reject` state, but `claim::run()` never enforces it. The `Reject` match arm only logs an “auto-forcing claim” warning and then falls through into the normal claim path, so `agent-doc claim` can silently rebind a document into the wrong live tmux session even when the configured session is healthy and the user did not pass `--force`. That breaks the current binding/session invariant and is a plausible root cause for cross-session pane drift.

3. `src/agent-doc/src/route.rs:688-721`, `src/agent-doc/src/route.rs:1166-1175`

Lazy-claim still has a pane-theft path. When a registered pane dies, route builds `claimed_panes` from the current repo’s `sessions.json` only, then adopts `tmux.active_pane(session_name)` if it is alive and looks like any agent process. There is no proof that the candidate belongs to the same repo, the same document family, or even the same `agent-doc` registry. In a superproject/submodule setup, an active pane from `src/session-share` or any unrelated Codex/Claude pane in the same tmux session but outside the local registry can be silently commandeered.

I added `#qmut`, `#xclm`, and `#lzcl` to the backlog as recommended follow-ups. The top backlog item is `#qmut`.
<!-- agent:boundary:663fca51 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#qmut] [recommended] Make strict queue consumption prove document + snapshot state before mutating either side or advancing the live queue
- [ ] [#xclm] [recommended] Enforce cross-session claim rejection unless --force is explicitly present
- [ ] [#lzcl] [recommended] Strengthen lazy-claim ownership proof so route cannot adopt unrelated active agent panes across registries/repos
<!-- /agent:backlog -->
