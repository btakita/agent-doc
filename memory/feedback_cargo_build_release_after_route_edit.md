---
name: cargo build release after source edits
description: After editing agent-doc source, run both cargo install --path . AND cargo build --release — the .bin symlink and ~/.cargo/bin both need updating
type: feedback
---

After editing any `src/agent-doc/` source file, deploy both binaries before declaring the change live:

1. `cargo install --path src/agent-doc` → updates `~/.cargo/bin/agent-doc` (used by tmux-spawned sessions)
2. `cd src/agent-doc && cargo build --release` → updates `target/release/agent-doc` (target of `.bin/agent-doc` symlink, used by IDE `Run Agent Doc`)

**Why:** `cargo build` (debug) produces `target/debug/agent-doc` which neither runtime consumer uses. The IDE JB plugin invokes `.bin/agent-doc` (release symlink). Tmux-spawned Claude sessions run `~/.cargo/bin/agent-doc` (cargo install target). Both must be rebuilt or the change is invisible at runtime, even though tests pass against the source.

**How to apply:** Any route/ipc/write path edit. Immediately after `cargo build` passes, also run install + release build before saying "fix is live" or inviting the user to test. Tests validate correctness; deploy makes it reachable.
