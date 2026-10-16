# Handoff: #tmuxscopesteward — steward systemd tmux-spawn scopes

Handed off from Claude Code (opus-5.5) to Codex on 2026-10-16. Owning session document: `tasks/agent-doc/agent-doc-bugs.md`. The backlog item `#tmuxscopesteward` and its queue line are there. The coordinator, not the worker, closes them.

## Problem
tmux 3.8 is built with systemd support, so it asks the user manager for one transient `tmux-spawn-<uuid>.scope` per pane. Two things make these scopes pile up:
- **Panes that exit too fast:** if a pane exits before systemd attaches it, the unit logs `No PIDs left to attach` and fails with `result 'resources'`. A failed unit stays loaded until `systemctl --user reset-failed`.
- **Leaked daemons:** a process that daemonizes out of a pane keeps that pane's scope running after the pane, or its whole tmux server, is gone.

Before the 2026-10-16 X11 crash, 3,911 scopes were loaded for about 50 live panes. The previous boot's journal shows 2,718 of these failures, from many short-lived tmux servers with 120–135 panes each. Those were agent-doc's real-tmux test runs.

## State
- Worktree: `/home/brian/work/btakita/agent-doc-wt/tmuxscopes`
- Branch: `fix/tmux-spawn-scope-stewardship`
- One commit, `24db3120da`, based on `origin/main` `46bc541bd8`. **Not pushed, not merged.**
- Untracked scratch: `.scratch-build.log`, `.scratch-scopes/`. Delete both before you finish; never commit them.
- **Not yet passed:** `make check`. The last full run had not finished when the agent was stopped. A build warning shows `#[warn(duplicate_macro_attributes)]` at a `#[test]` near line 536 of some file; find it and fix it.

### What the commit does (see its message)
- **Prevention:** tmux has no per-server way to turn scopes off, but it skips creating them when the session bus is unreachable. Every Makefile test target (`check`, `test`, `tmux-ci`, `sim-medium`, `sim-net`, `cross-editor-simworld`) now exports an unreachable `DBUS_SESSION_BUS_ADDRESS`. The export is target-specific, so install and release keep the real bus.
- **New crate `agent-doc-tmux-scope-io`:**
  - `classify.rs` is a pure classifier over `systemctl --user show` text.
  - `lib.rs` holds the sweep. It resets failed `tmux-spawn-*` scopes and keeps scopes whose tmux server and pane are alive.
  - It stops an orphaned scope only when the scope is empty, or holds nothing but agent-doc processes rooted in tempfile `.tmp*` fixture roots that are removed or at least 10 minutes old. Everything else is only reported.
- **Controller hook:** in `agent-doc-controller-io/src/project_controller/rpc.rs`, each non-temporary project controller checks once a minute on its idle tick. The sweep runs off the accept loop, behind a host-wide flock'd stamp (default every 10 minutes; overrides via `AGENT_DOC_TMUX_SCOPE_SWEEP_*`). It logs `tmux_scope_sweep` to ops.log.
- **On-demand command:** `agent-doc admin tmux-scopes [--dry-run] [--json]` in `src/main.rs`.
- **Test:** `tests/test_tmux_servers_skip_scopes.rs`.
- **Docs:** `docs/reference/features.md` and an `AGENTS.md` line.

### Open question the agent was checking when stopped
`origin/main` routes test runs through `scripts/with-cargo-cache` and `scripts/test-run-record`, and has a new `isolated_test_tmux` helper. Check whether either script or the helper rewrites or clears the environment. If one does, the `DBUS_SESSION_BUS_ADDRESS` export never reaches tmux and the prevention does nothing. If so, set it inside `isolated_test_tmux` (or wherever test tmux servers start) rather than only in the Makefile.

## Remaining work
1. `git fetch origin` and rebase onto the latest `origin/main` if it moved.
2. Resolve the open question above. Prove the prevention works: run a real-tmux test target, then confirm `systemctl --user list-units --all 'tmux-spawn-*'` gained no scope launched by the test servers.
3. Fix the `duplicate_macro_attributes` warning.
4. Mutation-check the classifier's stop/keep/report decisions and the env prevention. Break each one, confirm its test goes red, and restore from a scratch copy inside the worktree, then `touch` the restored file. Never restore with `git checkout --`.
5. Run `make check` in the worktree and capture `$?` explicitly, because RTK can mask exit codes. Run `make tmux-ci`.
6. Push the branch, fast-forward only.

## Rules
- Work only in this worktree. Do not touch `/home/brian/work/btakita/agent-loop` or its `src/agent-doc` checkout; other sessions use them, and that checkout's local `main` is about 807 commits stale.
- On this host you may run `systemctl --user list-units/show`, `journalctl --user`, and `systemctl --user reset-failed 'tmux-spawn-*'`. Do not stop or kill any running scope or process.
- Format only touched files: `rustfmt --edition 2024 <file>`, never `cargo fmt -- <files>`.
- Never fix a flaky test by rerunning it.
- Never `make install`, build for release, bump the version, or tag. Never write to the session document. The coordinator integrates: it fast-forwards `main` and runs one `make install`, then closes `#tmuxscopesteward`.

## Report
Branch, commit SHA(s), files changed, exact test commands and their pass/fail counts and exit codes, mutation results, and how you proved the prevention works.
