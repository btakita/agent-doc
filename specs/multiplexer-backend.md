# Multiplexer Backend

## Purpose

The session layer uses a `Multiplexer` trait for operations that cross the
pane/window/session boundary. Tmux remains the production backend, but
backend-neutral code must not shell out to `tmux` directly when the operation
fits this trait.

This is intentionally narrower than replacing all tmux layout behavior. The
first boundary covers operations that are common to a future Zellij or direct
pane backend:

- query pane metadata with `display_message`
- list pane geometry with `list_panes`
- check pane/session liveness
- capture pane output, including ANSI capture
- send a single key
- submit one single-line command, including harness-aware submit behavior

## Contract

- `sessions::Multiplexer` is the shared Rust trait.
- `tmux_router::Tmux` implements `Multiplexer`.
- New session helpers that only need pane state, geometry, capture, or command
  submission should accept `&dyn Multiplexer` or a generic `M: Multiplexer`
  instead of constructing `Command::new("tmux")` directly.
- Backend-specific command formatting remains inside the concrete backend
  implementation.
- The registry schema remains pane-id based for now; changing the binding
  identity from tmux pane ids to a backend-neutral id is future work.

## Current Coverage

The following `sessions.rs` helpers already route through `Multiplexer`:

- `pane_pid`
- `pane_window`
- `current_pane`
- `pane_by_position`
- `pane_by_position_in_window`
- `capture_pane`
- `capture_pane_with_ansi`
- `send_key`
- `send_submitted_text`
- `send_submitted_text_for_harness`

The position-selection parser is unit-tested with a mock backend so default
tests can cover geometry behavior without a live tmux server. Live tmux behavior
still belongs in `make tmux-ci`.

## Non-Goals

- Do not replace `tmux-router` reconciliation in this step.
- Do not introduce a Zellij backend until binding identity, stash semantics,
  and editor focus semantics are specified.
- Do not move managed supervisor owned-PTY input behind this trait; owned PTY is
  a child-process input path, while `Multiplexer` is the visible pane boundary.
