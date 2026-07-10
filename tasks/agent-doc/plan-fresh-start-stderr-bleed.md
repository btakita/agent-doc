# Plan: Fresh route-owned start stderr bleed into the agent pane (`#restartstderrbleed2`)

## Symptom (live, sitscape.md, 2026-07-10)

JB `Run Agent Doc` on a document with no tmux pane created the pane, ran
`agent-doc start --route-owned`, and dispatched the reopen. Separately from the
"prompt not submitted" drift (fixed by `#jbtsiftnosub2` in 0.34.74), when the
operator **manually submitted** the stranded prompt, raw stderr diagnostics bled
through into the console.

## Distinction from `#restartstderrbleed`

`#restartstderrbleed` (shipped) covers stderr from **restart/recycle/auto-install
children** (e.g. `make install` in `run_auto_install_steps_once`) — those are
routed through `auto_install_child_stdio` (`project_controller/rpc.rs`) to the
supervisor log fd. This follow-up is the **initial** `agent-doc start
--route-owned` boot: before `SupervisorStderrRedirect::start`
(`agent-doc-supervisor-process-io/src/lib.rs`) installs the fd2→
`supervisor-stderr.log` redirect, the CLI's own `[route] …` / `[start] …`
`eprintln!` diagnostics go to the freshly-created pane's inherited stderr. They
sit in the pane and become visible on the next redraw (the operator's manual
submit), reading as "stderr bled through."

## Root-cause candidates to confirm on a live repro

1. **Pre-redirect window.** `SupervisorStderrRedirect::maybe_start` runs after
   the route/start CLI has already emitted boot diagnostics to fd2. Options:
   install the redirect earlier (before the noisy route/start eprintln lines on
   the route-owned path), or route those diagnostics to the session log /
   `display-message` instead of fd2, consistent with the existing invariant
   "Managed capability proof stays out of pane transcripts" (`start.rs`).
2. **Redirect scope.** Confirm `supervisor_stderr_redirect_needed` fires for the
   fresh route-owned start (`route_owned && harness.is_tui_harness()`), and that
   nothing after the redirect writes to the saved (pre-redirect) fd.

## Deliverable

- A deterministic SimWorld / fd-plumbing test proving the initial route-owned
  start emits no CLI boot diagnostics to the agent pane's stdout/stderr stream.
- Keep aligned: `agent-doc-supervisor-process-io/src/lib.rs`
  (`SupervisorStderrRedirect`), `start.rs`, `route.rs` boot diagnostics, and the
  `#restartstderrbleed` invariant in `AGENTS.md`.
- Operator-verify: live JB `Run Agent Doc` on a no-pane document, confirm no raw
  stderr appears in the pane on start or on the first manual submit.
