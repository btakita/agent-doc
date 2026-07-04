---
name: feedback-jb-plugin-cdylib-hot-reload
description: "JB plugin hot-reloads the agent-doc cdylib; ship FFI/core fixes to the live editor with `agent-doc lib-install` (no plugin reinstall)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 16ae957a-8738-4e94-9b38-8d1fa9738b49
---

After an `agent-doc-core` / FFI / any `libagent_doc.so`-backed source edit, refreshing the
cdylib the JetBrains plugin loads requires `agent-doc lib-install` — **`cargo install` does
NOT do it**.

**Why:** `NativeLib.kt` resolves the native lib by running `agent-doc lib-path` (→
`~/.cargo/bin/libagent_doc.so`, a symlink to a versioned `.so`) and **hot-reloads on mtime
change** (`libMtimeChanged` → `reload`). `cargo install --path .` only installs the `agent-doc`
binary, not the cdylib, so the plugin keeps loading a stale `.so`.

**How to apply:** for a core/FFI fix to reach the *live* IDE, run:
`cargo build --release && agent-doc lib-install` (copies `target/release/libagent_doc.so` and
atomically swaps the `~/.cargo/bin/libagent_doc.so` symlink). The plugin picks it up on its next
FFI call via the mtime-changed reload — **no plugin zip rebuild/reinstall needed** when no `.kt`
changed. Extends [[feedback_cargo_build_release_after_route_edit]] (which covers the two binary
targets) with the third artifact: the plugin-loaded cdylib. A plugin zip rebuild + version bump is
only needed for actual `.kt` changes (see [[feedback_jb_plugin_version_bump_per_edit]]).
