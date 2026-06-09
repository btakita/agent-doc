# Plan: JB "Claim for Tmux Pane" cross-session reject UX

## Report

`agent-doc andreas-kovountzis.md` — JB "Claim for Tmux Pane" fails:

```
Claim failed (exit 1): Error: pane %43 is in tmux session '5' but project session is '0'; switch to the configured session or pass --force
```

## Root cause

`agent-doc-orchestration/src/claim.rs`:

- `cross_session_decision(pane_session, configured, configured_alive, force)` returns `Reject` when the target pane is in a tmux session different from the configured project session, the configured session is still alive, and `--force` was not passed (claim.rs:121-137).
- `enforce_cross_session_claim` turns `Reject` into `anyhow::bail!("pane {} is in tmux session '{}' but project session is '{}'; switch to the configured session or pass --force", ...)` (claim.rs:161).

The guard itself is correct — it prevents silently registering a pane that lives in the wrong tmux session (the chronic "session drift: registered panes span 2 tmux sessions: 0, 5" failure mode this very `agent-doc-bugs2.md` session keeps resyncing). The defect is **ergonomic**: the JB "Claim for Tmux Pane" action invokes `agent-doc claim <file>` without `--force` and renders the raw exit-1 `bail!` text in an error dialog. From the button the user has no path to "switch to the configured session" or "pass --force" short of dropping to a terminal.

## Fix (FFI-first; live-IntelliJ-gated)

1. **Structured reject signal** — give `claim` a machine-readable cross-session-reject result (typed exit code or a JSON/marker line carrying `pane_session`, `configured`, `pane_id`) so the plugin can branch on it instead of string-matching the bail message. Keep the human bail text for terminal use.
2. **JB plugin choice dialog** — on the structured reject, the "Claim for Tmux Pane" action prompts: pane is in session `<pane_session>`, project is `<configured>` → **[Force claim]** (re-run `agent-doc claim --force`), **[Switch project session]** (`agent-doc session set <pane_session>` / migrate, then re-claim), **[Cancel]**. No more raw exit-1 dialog.
3. **VS Code parity** — mirror the same branch in the VS Code claim command.
4. Verify the UX end-to-end in a live IntelliJ with a genuinely cross-session pane (pairs with the live-verify batch `#xkpf`).

## Tests

- Unit: `cross_session_decision` table already covers Accept/AcceptStale/AcceptForce/Reject — add a case asserting the structured-reject payload carries `pane_session`/`configured`/`pane_id`.
- Plugin UX: live IntelliJ verification only (gated).

## Spec

Update `specs/07-session-tmux-commands.md` (claim cross-session contract) with the structured-reject result shape.
