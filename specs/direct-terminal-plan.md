# DirectTerminal Implementation Plan

## Goal

Remove tmux as a hard dependency for agent-doc by implementing a `DirectTerminal` backend that manages pty sessions directly, making tmux an optional "premium" backend.

## Spec

See [terminal-backend.md](terminal-backend.md) for the `TerminalBackend` trait definition, implementation sketches, and integration points.

## Phases

### Phase 1: Extract TerminalBackend trait (no behavior change)

- [ ] Create `src/terminal_backend.rs` with trait + `UnixTerminal` + `NoopTerminal`
- [ ] Replace `RawMode` in `start.rs` with `select_backend()` dispatch
- [ ] Verify: all existing tests pass, `agent-doc start` works identically in tmux

### Phase 2: Abstract pane management

The tmux dependency goes beyond raw mode. These tmux-specific operations need abstraction:

| Operation | Current (tmux) | DirectTerminal equivalent |
|-----------|----------------|---------------------------|
| Create pane | `tmux split-window` | `portable-pty` spawn |
| Send command | `tmux send-keys` | Direct pty write |
| Capture output | `tmux capture-pane` | Read from pty master fd |
| Resize | `tmux resize-pane` | `TIOCSWINSZ` ioctl (already in `supervisor/pty.rs`) |
| Session persistence | tmux server survives terminal close | Not available without tmux |
| Pane targeting | `tmux select-pane -t %N` | Process/fd tracking |

- [ ] Define `PaneManager` trait abstracting pane lifecycle
- [ ] Implement `TmuxPaneManager` wrapping current tmux calls
- [ ] Implement `DirectPaneManager` using `portable-pty`

### Phase 3: DirectTerminal backend

- [ ] Wire `DirectPaneManager` into `start.rs` as fallback when tmux unavailable
- [ ] Handle graceful degradation: no session persistence, single-pane mode
- [ ] Update `install.rs` to note tmux as optional
- [ ] Add `--backend tmux|direct|auto` flag to `agent-doc start`

### Phase 4: Windows support

- [ ] Implement `WindowsTerminal` backend (ConPTY via `SetConsoleMode`)
- [ ] Test `DirectPaneManager` with `portable-pty` on Windows
- [ ] CI: add Windows test target

## Decisions to make before implementation

1. **Session persistence without tmux:** Accept the limitation, or implement a lightweight daemon that survives terminal close?
2. **Multi-pane layout:** In direct mode, is single-pane acceptable? Or should we implement our own splitting (significantly more work)?
3. **Auto-detection order:** `auto` backend should prefer tmux if available, fall back to direct. Configurable via `config.toml`?

## Dependencies

- `portable-pty` (already in Cargo.toml)
- No new crate dependencies expected for Phase 1-2

## Risk

Low for Phase 1 (pure refactor). Phase 2-3 introduce new code paths that need thorough testing. Phase 4 requires CI infrastructure changes.
