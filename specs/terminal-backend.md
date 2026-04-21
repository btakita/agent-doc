# TerminalBackend Trait Spec

## Motivation

agent-doc currently hardcodes terminal raw mode via `#[cfg(unix)]`/`#[cfg(not(unix))]` gates in `start.rs` (lines 117-169). This works for the Unix+tmux case but doesn't cleanly extend to:

- Windows (ConPTY via `SetConsoleMode`)
- Non-tmux environments (direct pty, headless/CI)
- Future terminal multiplexers (Zellij, custom)

A trait abstraction provides a stable extension point at negligible runtime cost (called a handful of times per session, not per-keystroke).

## Trait Definition

```rust
/// Terminal raw mode management.
///
/// Implementations handle platform-specific terminal mode switching.
/// The supervisor loop calls these methods at well-defined points:
/// - `enable_raw()`: before entering the supervisor loop
/// - `suspend()`: before any interactive prompt (read_line)
/// - `resume()`: after interactive prompts complete
/// - `restore()` / Drop: on supervisor exit
pub trait TerminalBackend: Send {
    /// Switch stdin to raw mode (disable line discipline translation).
    fn enable_raw(&mut self) -> anyhow::Result<()>;

    /// Temporarily restore cooked mode for interactive prompts.
    fn suspend(&mut self) -> anyhow::Result<()>;

    /// Re-enable raw mode after a suspend.
    fn resume(&mut self) -> anyhow::Result<()>;

    /// Restore original terminal state. Called explicitly before drop
    /// when the supervisor needs to guarantee restoration order.
    fn restore(&mut self) -> anyhow::Result<()>;
}
```

## Implementations

### UnixTerminal (current behavior, extracted)

```rust
#[cfg(unix)]
pub struct UnixTerminal {
    original: libc::termios,
}

#[cfg(unix)]
impl TerminalBackend for UnixTerminal {
    fn enable_raw(&mut self) -> Result<()> {
        unsafe {
            libc::tcgetattr(libc::STDIN_FILENO, &mut self.original);
            let mut raw = self.original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
        }
        Ok(())
    }
    fn suspend(&mut self) -> Result<()> {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original); }
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        unsafe {
            let mut raw = self.original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
        }
        Ok(())
    }
    fn restore(&mut self) -> Result<()> {
        self.suspend() // same operation
    }
}
```

### WindowsTerminal (stub, future)

```rust
#[cfg(windows)]
pub struct WindowsTerminal {
    original_mode: u32,
}

#[cfg(windows)]
impl TerminalBackend for WindowsTerminal {
    fn enable_raw(&mut self) -> Result<()> {
        // Save current mode via GetConsoleMode
        // Remove ENABLE_PROCESSED_INPUT, ENABLE_LINE_INPUT, ENABLE_ECHO_INPUT
        // Apply via SetConsoleMode
        Ok(())
    }
    // suspend/resume/restore: save/restore console mode
}
```

### NoopTerminal (headless/CI/non-tmux)

```rust
pub struct NoopTerminal;

impl TerminalBackend for NoopTerminal {
    fn enable_raw(&mut self) -> Result<()> { Ok(()) }
    fn suspend(&mut self) -> Result<()> { Ok(()) }
    fn resume(&mut self) -> Result<()> { Ok(()) }
    fn restore(&mut self) -> Result<()> { Ok(()) }
}
```

## Integration Points

### start.rs

Replace the current `RawMode` struct with:

```rust
fn select_backend() -> Box<dyn TerminalBackend> {
    #[cfg(unix)]
    if sessions::in_tmux() {
        return Box::new(UnixTerminal::new());
    }

    #[cfg(windows)]
    {
        return Box::new(WindowsTerminal::new());
    }

    Box::new(NoopTerminal)
}
```

The supervisor loop changes from:
```rust
let raw_mode = RawMode::enable();
// ... loop ...
drop(raw_mode);
```

To:
```rust
let mut backend = select_backend();
backend.enable_raw()?;
// ... loop (suspend/resume around prompts) ...
backend.restore()?;
```

### File location

New file: `src/terminal_backend.rs` (separate from existing `terminal.rs` which handles terminal emulator launching).

## Testing

- Unit tests for `UnixTerminal` are limited (requires a real tty). Test via integration tests in a pty.
- `NoopTerminal` is trivially testable.
- `select_backend()` can be tested by mocking `in_tmux()` return value.

## Migration

1. Create `terminal_backend.rs` with trait + three implementations
2. Update `start.rs` to use `select_backend()` instead of `RawMode`
3. Remove `RawMode` struct and its cfg blocks from `start.rs`
4. Add `mod terminal_backend;` to `main.rs`
