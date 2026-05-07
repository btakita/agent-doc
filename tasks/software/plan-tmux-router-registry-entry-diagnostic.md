# Plan — diagnose the reported `RegistryEntry` field mismatch in `tmux-router`

## Problem

The session document reports editor errors in `src/tmux-router/src/main.rs` saying:

- `session_id` is not part of `registry::RegistryEntry`
- `supervisor_instance_id` is not part of `registry::RegistryEntry`

At current HEAD, `src/tmux-router/src/registry.rs` defines both fields on
`RegistryEntry`, and `cargo check -p tmux-router` passes in
`src/tmux-router` without reproducing the failure. That means the next turn
needs to prove whether the report came from stale editor diagnostics, a
different checkout, or a real compile path that is not exercised by the crate's
current check command.

## Current evidence

- `RegistryEntry` in `src/tmux-router/src/registry.rs` includes
  `session_id: String` and `supervisor_instance_id: String`.
- `src/tmux-router/src/main.rs` constructs `RegistryEntry` with those fields.
- `cargo check -p tmux-router` completed successfully on 2026-05-07 from
  `src/tmux-router`.

## Next implementation turn

- [ ] Reproduce the exact reported failure from the same workspace state the
  user saw, including the command, crate root, and branch/worktree.
- [ ] If reproduction fails again, capture whether rust-analyzer or another
  editor index is out of sync with the current `RegistryEntry` definition.
- [ ] If reproduction succeeds, isolate which target or feature set is compiling
  against an older `RegistryEntry` shape.
- [ ] Apply the code or workspace fix only after the failing path is proven.
- [ ] Re-run the exact failing command plus the normal tmux-router verification
  commands before closing the implementation turn.

## Exit criteria

- The next execution turn either reproduces the mismatch with a concrete failing
  command or proves the report was stale/editor-local.
- Any fix is validated against the exact failing path, not only against a
  generic `cargo check`.
