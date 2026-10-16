# Handoff: #syncioredlivetmux — fix the 14 red live-tmux tests in agent-doc-sync-io

Handed off from Claude Code (opus-5.5) to Codex on 2026-10-16. Owning session document: `tasks/agent-doc/agent-doc-bugs.md` (backlog `#syncioredlivetmux` and its queue line; the coordinator closes them).

## Problem (backlog text)
14 live-tmux tests in `agent-doc-sync-io` fail on current `main`, and CI does not run them. The `#tmuxpanesreversed` agent found them; they fail the same way with the original `sync.rs`, so the pane-order change did not cause them. Fix each failure, then add the tests to `make tmux-ci` so they cannot rot again. Do not delete or ignore tests to make the suite pass.

## State
- Worktree: `/home/brian/work/btakita/agent-doc-wt/syncioredlivetmux`
- Branch: `fix/sync-io-live-tmux-tests`, created from `origin/main` `46bc541bd8`, which already includes the pane-order fix. **No commits yet.**
- Uncommitted WIP: 147 lines added and 25 removed:

| File | Lines changed |
|------|---------------|
| `agent-doc-supervisor-io/src/ipc.rs` | 127 |
| `agent-doc-sync-io/Cargo.toml` | 5 |
| `agent-doc-sync-io/src/resync/stash.rs` | 7 |
| `agent-doc-sync-io/src/sync.rs` | 32 |
| `agent-doc-sync-io/src/sync/layout.rs` | 1 |

- When stopped, the agent's last note was: "Now let me add the helper and fix the three session-log tests." The WIP is unreviewed and possibly incomplete. Read `git diff` first and treat it as a draft.
- No full test run on this branch is recorded.

## Remaining work
1. List the 14 failing tests exactly: the test target in `agent-doc-sync-io`, their names, and why CI skips them (`#[ignore]`, a feature gate, or a nextest filter). Group them by root cause; at least three are "session-log" tests.
2. Finish fixing each root cause. Real-tmux tests must use isolated `tmux -L <unique socket>` servers that are always killed, even on panic. Fix the nondeterminism itself: rerunning until green or raising timeouts does not count.
3. Add the tests to `make tmux-ci`.
4. **Possible overlap:** `#tmuxscopesteward` (`handoff-tmuxscopesteward.md`, branch `fix/tmux-spawn-scope-stewardship`) changes the Makefile test targets and how test tmux servers get their environment. Keep any shared-harness change minimal, and name it in your report so the coordinator can reconcile the two.
5. Mutation-check the key fixes. Restore from a scratch copy inside the worktree and `touch` the file; never restore with `git checkout --`.
6. Run the 14 tests, `make tmux-ci`, and `make check`, capturing `$?` explicitly.
7. Commit (`git commit -F -`) and push the branch, fast-forward only. Never push `main`.

## Rules
- Work only in this worktree. Do not touch `/home/brian/work/btakita/agent-loop`, its `src/agent-doc` checkout, or the other `agent-doc-wt/*` worktrees.
- Format only touched files: `rustfmt --edition 2024 <file>`.
- Never `make install`, build for release, or tag. Never write to the session document.

## Report
Per test: its root cause and fix. Also the branch, commit SHA, files changed, exact test results, mutation results, and any shared-harness changes.
