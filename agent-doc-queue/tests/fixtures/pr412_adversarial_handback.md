REVIEW-CLEAN https://github.com/btakita/agent-doc/pull/412#pullrequestreview-5897902287 base=d966b06a2324be290d3383d8d44b1ff99a95fb61 head=196dbdd77a1ccff29fa16c121d42efe2e75023cf cycles=2

THREAD-FIX-ON-HEAD https://github.com/btakita/agent-doc/pull/412#discussion_r4568099735 head=196dbdd77a1ccff29fa16c121d42efe2e75023cf resolved

**Summary**
PR 412 is review-clean at head `196dbdd77a`, and its CI is green there. The red check was a timing flake that already existed on main; it was fixed at its cause, not rerun until it passed. The second independent review found nothing to change.

**What changed**
- Branch `fix/gh410`, pushed as a fast-forward from `30deef3765` (no rebase, no force push):
  - `14ab4a69ea` `fix(queue): wait through the apply-lock handoff window instead of bailing (#412)`, in `agent-doc-queue-io/src/queue_edit_owner.rs`.
    - **Cause:** the background maintenance worker clears its lease row while it still holds the apply lock. If the Stop hook tried the lock in that gap of a few milliseconds, it saw no lease and failed at once with `queue mutation deferred: another owner is applying maintenance`.
    - **Fix:** in that state the caller now keeps retrying for at most 2 seconds. A live direct owner still makes it fail on the first look, as before.
    - Two deterministic regression tests; both fail with the fix disabled.
  - `196dbdd77a` `fix(start): say where a staged start pane waits and how to show it (#410)`, in `agent-doc-start-io/src/lib.rs`. The outside-tmux start message now says the pane is waiting in the `stash` window and prints `tmux select-window -t %N` to show it.
- **Cycle 1:** review https://github.com/btakita/agent-doc/pull/412#pullrequestreview-5897349627 at `30deef3765` found nothing that must change, one optional finding, and the red check.
  - Reply on the thread (now resolved): https://github.com/btakita/agent-doc/pull/412#discussion_r4568355508
  - Summary comment: https://github.com/btakita/agent-doc/pull/412#issuecomment-6864224189
- **Cycle 2:** review https://github.com/btakita/agent-doc/pull/412#pullrequestreview-5897902287 at `196dbdd77a` found no must-fix and no optional findings.
- Delivery worktree: `/tmp/agent-doc-worktrees/gh410/agent-doc`, clean on `fix/gh410` at `196dbdd77a`. Logs are under its `target/gh410-logs/`.

**Verification**
- Does PR 412's CI failure also happen on main? Yes. It is `codex_hook_integration::a_bare_trigger_matches_the_file_trigger_through_admission_and_stop`, run 16 at a time with `target/gh410-logs/adv412-stress.sh`: original PR head `30deef3765` failed 1/48; base `d966b06a23` 2/48; current main `9a0eb19807` 9/144. Every failure printed the same message, and main's own hosted `make check` on `d966b06a23` happened to pass it.
- Fix confirmed by the cycle-2 reviewer's own runs (16 at a time, 12 rounds): new head 0/192 with the fix, 12/192 with it disabled; local merge of new head with main `9a0eb19807` 0/192 with the fix, 18/192 with it disabled.
- Cycle-2 reviewer checks (all via `agent-doc build-slot`, private tmux servers): `cargo nextest run` on the 8 crates involved: 1453 passed on the head, 1457 on the merge with main; `cargo clippy --all-targets -- -D warnings` clean on both; `cargo test -p agent-doc --test codex_hook_integration` 22 passed; `test_cli_start_` 7 passed; `start_bootstrap` tests 4 passed.
- Cycle-1 reviewer checks: the 5 crates the original PR touched gave 580 passed; live-tmux tests passed (sync-io 126, tmux-io 12, route-io in `make tmux-ci` 8); clippy clean.
- Hosted CI run 43502662841 on `196dbdd77a` (merge with current main): `make check` success, `make tmux-ci` success, `check` SUCCESS.
- Final re-query: head unchanged, the PR is mergeable, the one inline thread is resolved, and nothing new has arrived.

**Action items**
- Fix `agent-doc-queue-io/src/queue_cmd.rs:2112` (`prune_duplicates`): `apply.then(|| QueueEditGuard::acquire(file))` throws away the error, so `queue prune-duplicates --apply` writes without the apply lock whenever acquiring it fails; it probably needs `.transpose()?`. This is on main too and predates PR 412.
- Fix the flake in `agent-doc-queue-io` `build_slot::tests::run_propagates_the_exit_status_and_marks_the_slot_held`: it failed 2 of 4 times under threaded `cargo test`, likely another test thread forking while this test takes its lock back. It passes under nextest, which `make check` uses.

**Notes**
- Main still has the `codex_hook_integration` flake until PR 412 merges, because the fix lives only on this branch.
- Coordinator note: PR 417 (`#answerheadmaintrace`, `4cf578e892`) fixes the same apply-lock handoff window in `queue_edit_owner.rs`; the two PRs overlap and must be reconciled at merge.
- The main tmux-ci flake (#orphandrainclearflake) was not touched. It did not fail on this PR's hosted run.
- Merging was not authorized, so PR 412 is open, mergeable and green, waiting for a merge decision.
- A nit the reviewer did not raise: `select-window -t %N` selects the pane's window but does not focus the pane once a sync has merged it into a busy stash.
- No `make install`, release or tag was run, and the session document was not touched.

**Next actions**
- Close `[#q-23c8d4f70342]` as REVIEW-CLEAN. `do [#gh410]` and `do [#pr412fixci]` are satisfied by PR 412 at `196dbdd77a` with CI green, pending merge.
- After merge, run the single `make install`.