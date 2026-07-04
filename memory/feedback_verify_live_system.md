---
name: verify-live-system-after-fix
description: After fixing code that affects running processes (tmux, daemons, binaries), verify the live system state — not just tests
type: feedback
---

After fixing code that affects running processes, always verify the live system state — passing tests alone is insufficient.

**Why:** Fixed a window index bug in repair_layout but didn't verify the actual tmux session was corrected. User had to point out the fix wasn't applied to the running system. Also forgot `cargo install --path .` so tmux-spawned sessions ran the old binary.

**How to apply:**
1. After building a fix for agent-doc or tmux-router: run `cargo install --path .` to update `~/.cargo/bin`
2. After fixing tmux/window management code: check `tmux list-windows` to verify actual state
3. After any process-affecting fix: demonstrate the fix works on the live system, not just in tests
