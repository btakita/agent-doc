# Dogfood Supervisor Refresh

**Fallback runbook — dogfooding only.** This procedure is only needed when an
`agent-doc` session is editing **agent-doc's own source** (`src/agent-doc/...`)
and the live route-owned supervisor that hosts the session is therefore running
an older binary than the one the turn just built. A normal `agent-doc` user
never builds the binary mid-session, so this never applies to them.

The binary is meant to handle this itself once the self-heal work is fully
landed (`#supselfheal` idle-boundary self-recycle, `#anw0` blue/green
drain-and-supersede restart, `#jbrestale` stale-supervisor auto-restart on
dispatch, `#recyclerestart` recycle-restarts-the-session). Until those close,
the bootstrap step below is manual because the running supervisor predates the
fix it would need to recycle itself, and it is the **parent process** of the
live agent — a session cannot cleanly pull the binary out from under itself.

## When to run

After a normal `finalize` / `write --commit` closeout (response committed,
`session-check` green), **and** all of:

- the turn edited `src/agent-doc` source (Rust, `SKILL.md`, or a bundled
  runbook), and
- `preflight` / `session-check` reported `supervisor_binary_stale` (the
  route-owned host supervisor started before the now-installed binary), and
- the remaining backlog needs that fresh binary to make progress.

Do **not** run this just because the queue is churning, or to "pick up" changes
that were not built this turn. If the turn changed no agent-doc source, the
running supervisor is not stale and there is nothing to refresh.

## Procedure

Run from the agent-doc submodule root (`src/agent-doc`). Order matters: finish
all source work, verification, and the binary-owned closeout **first**, because
the final step can recycle this very session.

1. **Verify + build + install the new binary.**
   ```bash
   cd src/agent-doc
   make check                       # must be green (real exit 0, zero `result: FAILED`)
   cargo build --release
   cargo install --path .           # refreshes ~/.cargo/bin (tmux-spawned sessions)
   agent-doc lib-install            # refreshes the cdylib for JB plugin hot-reload
   ```
   `make install` is shorthand for the build+install half if the Makefile
   defines it; the `lib-install` step is still required for the JB `.so`.

2. **Commit + push the source change** (submodule, then superproject pointer)
   through the normal git path before restarting — never leave the fix
   uncommitted, since the restart proves nothing if the running binary's source
   is not in HEAD.

3. **Restart the supervisor onto the new binary — do this LAST.**
   ```bash
   agent-doc session restart-supervisor <FILE>          # continue-mode (preferred)
   agent-doc session restart-supervisor <FILE> --force  # if the pane is busy/wedged
   ```
   Continue-mode is designed to swap the supervisor binary while preserving the
   live agent child. `--force` interrupts a busy pane first. Because the running
   (old) supervisor is the one executing the restart, it may recycle the pane
   rather than execve cleanly — that is acceptable here: the response is already
   committed, and a recycled session comes back up on the new binary, which is
   the goal.

4. **Confirm the refresh.** After the restart, `agent-doc session status <FILE>`
   should show the supervisor process started **after** the installed binary
   mtime, and `preflight` should no longer emit `supervisor_binary_stale`.

## What Not To Do

- **Do not `cargo install` before the binary-owned closeout.** Installing a
  newer binary mid-cycle makes the finalize client newer than the running
  supervisor and triggers `live_prompt_drift` / exit-75 with dropped queue
  edits (`#no-mid-session-install`). Install only after `finalize` +
  `session-check` for the turn have landed.
- **Do not restart the supervisor before committing + pushing** the source
  change — a restart against an uncommitted tree proves nothing and risks losing
  the build's provenance.
- **Do not solve this with agent memory.** This is a product gap; track it in
  the agent-doc backlog (the `#supselfheal` / `#jbrestale` / `#recyclerestart`
  family) so it ships to every operator, not one.

## See Also

- `runbooks/persist-closeout.md` — response-ordering + closeout boundary.
- `runbooks/commit.md` — binary-owned closeout ordering.
- `runbooks/harness-invocation.md` — supervisor / route / startup invariants.
- `specs/07-session-tmux-commands.md` — `session restart-supervisor`.
