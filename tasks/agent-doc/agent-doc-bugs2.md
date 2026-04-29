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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
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

❯ do #xclm. spec-test-build-install-commit-push
### Re: #xclm cross-session claim rejection — gpt-5

Implemented in `src/agent-doc` and pushed `7280e5c` on `20260422/codex-agent-backend`.

`agent-doc claim` now fails closed when the resolved pane is in a different live tmux session than the configured project session. The old path already computed `CrossSessionDecision::Reject`, but `claim::run()` only warned and continued. I lifted that match into an enforcement helper so `Reject` returns a concrete error, while the stale-session and explicit `--force` paths still succeed with their existing warnings.

I updated the claim/session-routing specs plus `VERSIONS.md`, and added regression coverage for the fail-closed helper.

Verification:
- `cargo test enforce_cross_session_claim_errors_on_reject -- --exact`
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `7280e5c` `fix: reject live cross-session claims`

`#qmut` remains the top backlog item.

❯ do #lzcl. spec-test-build-install-commit-push
### Re: #lzcl lazy-claim provenance — gpt-5

Implemented in `src/agent-doc` and pushed `4794e41` on `20260422/codex-agent-backend`.

The fix removes the unsafe implicit lazy-claim path in `route.rs`. When a registered pane is dead, Strategy 2 now accepts only an explicit pane override as lazy-claim provenance; it no longer adopts whatever pane happens to be `tmux.active_pane(session)` just because that pane looks like Codex/Claude. That closes the cross-registry / cross-repo pane-theft shape behind `#lzcl`. The existing guards still apply: already-claimed panes are rejected, non-agent processes are rejected, and route falls through to auto-start when no explicit safe candidate exists.

I updated the route contract comments, `specs/07-commands.md`, and `VERSIONS.md`, and added regression coverage proving the active-pane fallback is gone while explicit `--pane` lazy-claim still works.

Verification:
- `cargo test lazy_claim_requires_explicit_pane_provenance`
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `4794e41` `fix: require explicit lazy-claim provenance`

`#qmut` remains the top backlog item.

❯ do #qmut. spec-test-build-install-commit-push
### Re: #qmut strict queue closeout — gpt-5
Implemented in `src/agent-doc` and pushed `e835897` on `20260422/codex-agent-backend`.

The fix closes both sides of the queue-advance bug. Strict closeout no longer consumes `agent:queue` before pending maintenance and the strict pending guards run, and `consume_queue_prompt()` now proves the document and snapshot transforms completely before it writes either one. That means a rejected strict closeout keeps the queue head in place instead of partially advancing the working tree or snapshot before the commit boundary.

I updated the queue specs plus `VERSIONS.md`, and added a finalize integration regression that proves `#fix1` stays at the queue head when a later strict pending gate rejects the cycle.

Verification:
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `e835897` `fix: delay strict queue consumption until closeout gates`

`#fuback` is now the top remaining backlog item.

❯ compact exchange should peek into the backlog to provide more context. Should I rely on presets or should agent-doc intelligently compact the exchange to preserve context for the backlog items?
### Re: compact exchange backlog context — gpt-5

Use intelligent backlog-aware compaction as the default, and keep presets as optional overlays rather than the primary source of truth.

The backlog, queue, and icebox are already the canonical state surfaces. Presets are static prompt sugar. If compaction relies mainly on presets, it can preserve tone or workflow hints while still dropping the actual unresolved work. A safer compacted checkpoint should explicitly carry forward the top open backlog items, the queue head when present, any gated or iceboxed items that still matter, and any recently-stated backlog conclusion that would otherwise become stale.

Presets still have value, but as policy knobs: for example, "include the top 3 backlog items" or "always mention the active queue head". I would not use them as the source of substantive context. State should come from the live document components; presets should shape how that state is summarized.

I added that as a recommended backlog follow-up so it is tracked without treating it as accepted work yet. `#fuback` remains the top accepted active item.

❯ If the document markdown frontmatter has syntax errors, auto-start does not start and there is no feedback. agent-doc should start showing an error for the bad syntax. spec-test-build-install-commit-push
❯ `'#follow-up-backlog': Any follow-up items to place in the backlog?`
```
~/work/btakita/agent-loop main ⇡9 !6 ?2                                                        8s  agent-loop 21:07:41
❯ /home/brian/.cargo/bin/agent-doc start tasks/agent-doc/agent-doc-bugs2.md
Error: did not find expected key at line 13 column 39, while parsing a block mapping at line 9 column 3
```
### Re: frontmatter syntax startup feedback — gpt-5

Implemented in `src/agent-doc` and pushed `4dd9c47` on `20260422/codex-agent-backend`.

The failure mode is now explicit across the startup paths. `frontmatter.rs` adds a shared wrapper that turns YAML parse failures into `invalid YAML frontmatter in <file>` with a repair hint for the `--- ... ---` block. `start.rs` and `route.rs` now fail closed through that path instead of surfacing raw parser noise or defaulting past the parse. `sync.rs` now logs the same contextual warning during file resolution and auto-start, then skips the malformed file for that pass instead of silently dropping it.

I updated `specs/07-commands.md`, `docs/guide/commands.md`, and `VERSIONS.md`, added regression coverage for the shared parse wrapper plus `start` and sync-phase error context, then ran `cargo test -- --test-threads=1`, `cargo build`, and `cargo install --path . --force`.

No new backlog item came out of this fix. `#fuback` remains the top accepted active item.
### Re: frontmatter syntax wording — gpt-5

Yes. That is the implemented behavior: malformed frontmatter now surfaces a document-targeted startup error with a repair hint for the bad `--- ... ---` block, and sync logs the same warning instead of failing silently.

❯ The following frontmatter prevents a startup. The pane does not appear so it seems like nothing happens during autostart. spec-test-build-install-commit-push
```
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': '#follow-up-backlog' Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
```
### Re: autostart malformed frontmatter visibility — gpt-5

Implemented in `src/agent-doc` and pushed `949bf1c` on `2026-04-28`.

The missing piece was `sync.rs`: the bad frontmatter path already logged a warning and skipped auto-start, but editor-driven auto-starts had no pane and therefore no obvious visible feedback. Sync now mirrors that exact warning into the document's `agent:status` component when present, updates the snapshot in the same helper, and clears only that sync-owned status message once the frontmatter parses again. That covers both `resolve_file` and `auto-start` parse failures without clobbering unrelated status text.

I updated `specs/07-commands.md`, `docs/guide/commands.md`, and `VERSIONS.md`, added regression coverage for status surfacing/clearing in `sync.rs`, then ran `cargo fmt`, `cargo test -- --test-threads=1`, `cargo build`, and `cargo install --path . --force`.

No new backlog item came out of this fix. `#fuback` remains the top accepted active item.

❯ Can the error message be more user-friendly? For example, show the text that is in error...like Rust's compiler. spec-test-build-install-commit-push
```
[agent-doc sync] malformed frontmatter during resolve_file.

sync resolve_file frontmatter: invalid YAML frontmatter in tasks/agent-doc/agent-doc-bugs2.md: did not find expected key at line 13 column 39, while parsing a block mapping at line 9 column 3

Fix the frontmatter between the opening and closing --- markers, then rerun the command.
```
### Re: frontmatter error excerpt — gpt-5

Implemented in `src/agent-doc` and pushed `c49040d` on `2026-04-28`.

The malformed-frontmatter error is now more readable in the exact path that matters. `frontmatter::parse_for_file()` now keeps the existing file-targeted error and repair hint, but when `serde_yaml` reports a location it adds a compiler-style excerpt of the frontmatter with a caret at the reported line/column. I also changed `ensure_session_for_file()` to go through that same file-aware parser first, so `start` and `route` get the richer message before UUID assignment instead of falling back to the older generic wrapper. Sync inherits it automatically because it already routes through `parse_for_file()`.

I updated the regression checks in `frontmatter.rs`, `start.rs`, and `sync.rs`, and refreshed `specs/07-commands.md`, `docs/guide/commands.md`, and `VERSIONS.md` to document the excerpted error shape.

Verification:
- `cargo fmt`
- `cargo test -- --test-threads=1`
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `c49040d` `fix: show frontmatter parse excerpt`

No new backlog item came out of this change. `#fuback` remains the top accepted active item.
<!-- agent:boundary:46870908 -->
<!-- /agent:exchange -->

<!--
compact exchange should peek into the backlog to provide more context
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#rpaj] [recommended] Make compact exchange synthesize backlog-aware context from live backlog/queue/icebox state; use presets only to tune summary policy
- [ ] [#fuback] Enforce follow-up backlog capture for review flows that request or imply backlog output
<!-- /agent:backlog -->
