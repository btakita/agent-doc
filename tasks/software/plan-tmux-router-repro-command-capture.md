# Plan — capture the exact failing tmux-router reproduction context

## Problem

The user reported `RegistryEntry` field errors in `src/tmux-router/src/main.rs`,
but the current crate-level `cargo check -p tmux-router` passes. Before any fix
turn, the next cycle needs a precise reproduction context rather than a generic
"cannot reproduce".

## Goal

Capture the exact command, crate root, editor/tooling context, and worktree
state that produced the original mismatch so a later execution turn can verify
the same path.

## Next implementation turn

- [x] Record the exact command or editor action that surfaced the
  `RegistryEntry` field mismatch.
- [x] Confirm the working directory, branch, and whether the error came from
  rust-analyzer, `cargo check`, `cargo build`, or another target.
- [x] Re-run that exact path under the same checkout before attempting a fix.

## Captured reproduction context

- Source report: `tasks/software/tmux-router.md` exchange, where the user said
  the errors appeared "In main.rs". No shell command was captured in the
  prompt, so the available failing path is an editor/rust-analyzer diagnostic,
  not a proven `cargo check` or `cargo build` command.
- Crate root checked: `/home/brian/work/btakita/agent-loop/src/tmux-router`.
- Branch and commit checked: `main` at `d6d7a4f` (`fix: delay tmux submit
  after literal send`).
- Toolchain checked: `cargo 1.94.0 (85eff7c80 2026-01-15)` and `rustc 1.94.0
  (4a4ef493e 2026-03-02)`.
- Current source: `src/tmux-router/src/registry.rs` defines
  `session_id: String` and `supervisor_instance_id: String` on
  `RegistryEntry`; `src/tmux-router/src/main.rs` constructs `RegistryEntry`
  with both fields.
- Rust-analyzer/flycheck evidence: `src/tmux-router/target/flycheck*` contains
  older successful check records for prior package versions (`0.3.2`, `0.2.7`)
  and no current `RegistryEntry` field error. That points to stale editor
  diagnostics or an older checkout/index, not the current crate state.

## Re-run commands

- `cargo check --all-targets` from `src/tmux-router`
- `cargo test` from `src/tmux-router`
- `cargo build --release` from `src/tmux-router`
- `cargo install --path . --force` from `src/tmux-router`

## Exit criteria

- The next implementation turn has a concrete reproduction recipe for the
  reported tmux-router diagnostic.
- Completed on 2026-05-07: the concrete recipe is the editor/rust-analyzer
  path above; the command-line crate paths do not reproduce the mismatch under
  the current checkout.
