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

## Automated path (opt-in, `#supautoinstall`)

The build+install half of this procedure (steps 1–2 below) can run automatically
**in the supervisor at an idle boundary** instead of by hand. Enable it for a
dogfooding session with:

```bash
export AGENT_DOC_SUPERVISOR_AUTO_INSTALL=1   # default OFF
```

When set, after a `finalize` that committed an edit to agent-doc's own source the
supervisor's idle-queue watch detects that the source is newer than the installed
binary and runs `cargo build --release && cargo install --path . && agent-doc
lib-install` at the next turn boundary (debounced, never mid-turn). That makes the
installed binary newer than the running process, so the existing `#ctlrecycle`
recycle path hot-reloads onto it on the same/next boundary — closing the whole
loop without an operator step. The build runs in the **supervisor** (idle), never
in the finalize client, so it cannot cause the mid-session-install drift the
manual procedure warns about. ops.log proof: `supervisor_auto_install_started` →
`supervisor_auto_install_succeeded` → `supervisor_binary_stale_self_recycled`.

It is **dogfood-only** — `dogfood_agent_doc_crate_root` resolves a crate root only
when the served document's project tree contains the agent-doc crate, so it never
fires for an ordinary user's document. Heavy (a full build blocks the idle watch
for its duration) and high blast-radius, hence default OFF; a failed build latches
off for the session and falls back to the manual procedure below. When OFF, a
source-ahead-of-binary state logs `supervisor_source_newer_detected` once.

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

## Troubleshooting

### `controller launch already in progress` / `os error 11` during hot-reload

Symptom — the supervisor self-recycle (a `next_queue_item` hot-reload that
`execve`s onto the freshly-installed binary while preserving the live agent
child) aborts to the pane with:

```
Error: controller launch already in progress: …/.agent-doc/locks/controller-launch.lock

Caused by:
    Resource temporarily unavailable (os error 11)
```

Cause — the just-`execve`'d supervisor re-ran `start` →
`ensure_controller_running` → `connect_or_launch` → `LaunchLock::acquire`, which
used a **non-blocking** `try_lock_exclusive` on the *shared per-project-root*
`.agent-doc/locks/controller-launch.lock`. With several sessions open in the
same superproject (equityfundingsource, tsift, lazily-rs, …), another launcher
was mid-launch holding that lock, so the recycle failed immediately with
`EWOULDBLOCK` (os error 11). Launch-lock contention is a *benign* race
("someone else is launching right now"), not a fatal condition.

Fixed — `#suprecyclelock` (agent-doc `ce7f3e7d`): `connect_or_launch` now uses
`LaunchLock::acquire_blocking`, which polls `try_lock_exclusive` until the holder
releases (bounded 8s `LAUNCH_LOCK_WAIT`, sized above the launch+wait window) and
then adopts the controller the other launcher published; only a genuinely wedged
holder still errors.

Recovery on a binary that predates the fix — the refresh procedure above is
idempotent and the response is already committed, so just re-run it: restart the
supervisor (`agent-doc session restart-supervisor <FILE> --force`) and re-invoke
`/agent-doc <FILE>`. Once a supervisor on `ce7f3e7d`+ is hosting the session, the
blocking acquire waits the contender out instead of crashing the recycle. Live
proof that the race no longer surfaces the error is operator-drive (`#1j8q`).

## See Also

- `runbooks/persist-closeout.md` — response-ordering + closeout boundary.
- `runbooks/commit.md` — binary-owned closeout ordering.
- `runbooks/harness-invocation.md` — supervisor / route / startup invariants.
- `specs/07-session-tmux-commands.md` — `session restart-supervisor`.
