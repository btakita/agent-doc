---
name: cargo install and lib-install on release
description: Always run cargo install --path . AND agent-doc lib-install after publishing agent-doc to update both binary and shared library
type: feedback
originSessionId: 2b7b9681-69bf-4e3f-82bf-9c24346bd3f1
---
Always run `cargo install --path .` AND `agent-doc lib-install` after publishing agent-doc releases. Three targets must be current:
- `~/.cargo/bin/agent-doc` — binary used by tmux-spawned sessions
- `target/release/agent-doc` — binary used via `.bin/agent-doc` symlink
- `~/.cargo/bin/libagent_doc.so` — shared library loaded by JB plugin via FFI

**Why:** `cargo install` only installs the binary, not the shared library. v0.33.10 had a critical FFI fix (bool→i32 return types) but the JB plugin kept loading the old `.so` from `~/.cargo/bin/` because `lib-install` wasn't run. Every `isClaimedByForceDisk` call returned garbage → true, silently blocking all IPC patch applies for days.

**How to apply:** After `cargo install --path .`, always run `agent-doc lib-install`. The `/dev` skill should include both steps. After lib-install, IDEA restart is needed for the IPC listener thread to pick up the new library.
