---
agent_doc_session: 09760345-ef9d-426a-9416-250563e42548
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Implemented document-level hooks (session_start/post_write/post_commit) via frontmatter YAML. Fixed (HEAD) suffix incorrectly committed to fenced code # comments (is_fence_marker + in_fence tracking in git.rs). Fixed ❯ prefix lost in editor after transfer (normalize_patch_content bakes prefix into IPC patch body). Discussed real-time steering via tmux send-keys. Open: release, agent-doc steer command, inotify watcher, fenced-code diff classifier.

❯ ### Spec: FFI library hot-reload + versioning + GC — opus-4-7

❯ **Motivation:** IDE crashed at SIGSEGV (PC=0x0) calling `agent_doc_document_changed` via JNA, 0.6s after `cargo install` overwrote `~/.cargo/bin/libagent_doc.so` while it was mmap'd. See `tasks/agent-doc/agent-doc-bugs.md` § "IDEA crash" for full diagnosis.

❯ **Goal:** Make FFI library upgrades safe while the IDE is running. Hot-reload on mtime change; versioned filenames so concurrent processes can hold different versions; reference-counted GC of stale versions.

❯ **Components:**

❯ 1. **Versioned cdylib install** (Rust install path + Makefile)
❯    - Install writes `libagent_doc-<version>.so` (NOT just `libagent_doc.so`)
❯    - Atomic symlink update: `libagent_doc-current.so` → `libagent_doc-<version>.so` via `ln -sfn` + `rename(2)`
❯    - Cargo emits unversioned name; post-install rename in `Makefile install` target and PyPI wheel build
❯    - Distribution paths: `cargo install`, manylinux Docker wheel, GitHub Releases tarball, JB plugin install path, VS Code extension

❯ 2. **`agent-doc lib-path`** returns the symlink (`libagent_doc-current.so`), not the versioned target. dlopen resolves the symlink → mmaps the inode → kernel keeps it alive after future symlink swaps.

❯ 3. **mtime-based hot-reload in JNA loader** (`editors/jetbrains/.../NativeLib.kt`)
❯    - `AgentDocLib.get()` stats the .so on every call
❯    - If mtime > `lastLoadedMtime`, force `Native.unregister(AgentDocLib::class.java)` and re-`Native.load()` from the (possibly new) symlink target
❯    - Cost: one `stat(2)` per FFI dispatch — negligible
❯    - Closes the race window where `cargo install` swaps the symlink between calls

❯ 4. **Lockfile-tracked GC** (Rust `agent-doc gc-libs` subcommand + JNA shutdown hook)
❯    - On JNA load: write `<so-path>.lock` containing this JVM's PID
❯    - JVM shutdown hook removes only the lock file (NOT the .so itself — sibling processes may still hold it)
❯    - GC sweep walks `libagent_doc-*.so`:
❯      - Keep if it's the current symlink target
❯      - Keep if `.lock` exists AND `/proc/<pid>` is alive
❯      - Otherwise: unlink the .so AND its orphaned lock
❯    - Triggers: on JNA load (after writing own lock), on `cargo install` post-step, manual `agent-doc gc-libs`
❯    - Crash-safe: stale locks from SIGKILL'd JVMs cleaned up next sweep

❯ **Files touched:**
❯ - `src/agent-doc/src/install.rs` — versioned install + symlink update
❯ - `src/agent-doc/src/main.rs` — register `gc-libs` subcommand
❯ - `src/agent-doc/src/lib_gc.rs` (new) — versioned .so walk + `/proc/<pid>` liveness check
❯ - `src/agent-doc/Makefile` — post-install rename + symlink + `gc-libs` call
❯ - `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt` — mtime check + `Native.unregister` + reload + lock write + shutdown hook
❯ - `src/agent-doc/editors/vscode/...` — equivalent lock write/release in the VS Code extension's native loader (if it dlopens the .so directly)
❯ - PyPI wheel build (`.github/workflows/pypi.yml`) — versioned filename in wheel

❯ **Estimated diff size:**
❯ - Rust: ~80 lines (install + gc-libs subcommand + sweep logic)
❯ - Kotlin: ~30 lines (mtime check + unregister/reload + lock + shutdown hook)
❯ - Makefile / CI: ~10 lines

❯ **Open questions:**
❯ - Does VS Code's extension dlopen the .so, or does it shell out to `agent-doc` CLI? If CLI-only, no Kotlin-equivalent needed — but the CLI itself needs the GC sweep registered as an exit handler.
❯ - Should `agent_doc_version()` be called immediately after each hot-reload to verify the new symbol set is callable? Or trust the dlopen?
❯ - Versioned filename collides with how `maturin` packages the wheel. Need to verify Python `ctypes`/`cffi` consumers can find `libagent_doc-current.so` (or whether we need a separate naming scheme inside the wheel).

❯ add ids for each agent:pending item
❯ ### Re: FFI hot-reload spec — open questions answered — sonnet-4-6 (HEAD)

Solid spec. Answering the three open questions:

**1. VS Code uses koffi (dlopen directly), not CLI.** `editors/vscode/src/native.ts` calls `koffi.load(libPath)` with lazy function binding. So VS Code needs its own mtime reload loop: stat the symlink target on each dispatch; if mtime changed, `koffi.unload(lib)` + reassign `lib = koffi.load(libPath)` + rebind functions + reset `loadAttempted`. Added as a pending item (parallel to `#2cyy`). The lock write / shutdown hook also applies: write a `.lock` file on load, remove it on `deactivate()`.

**2. Call `agent_doc_version()` after hot-reload.** Cheap sanity check — catches ABI mismatch before the first real call crashes. Log the returned version string to confirm the new symbol set is live.

**3. No maturin collision.** Versioned `libagent_doc-<version>.so` is for the C ABI cdylib (JNA / koffi). maturin produces a separate Python extension module (`_agent_doc.cpython-*.so`) from a different crate-type target in `Cargo.toml`. These don't share filenames. The wheel ships its own .so with maturin's naming; versioned naming only applies to the cdylib install path outside the wheel.

**IDs:** Preflight backfilled hash IDs for all pending items already — confirmed present.
❯ Add actionable steps from the answers to the 3 open questions.
❯ ### Re: actionable steps from open questions — opus-4-6 (HEAD)

Mapping each answered question to pending items:

**Q1 → `#ky3a` (updated).** VS Code koffi hot-reload already tracked. Expanded scope to include lock file write on `koffi.load()` + remove on `deactivate()` — the VS Code side of the GC protocol from `#408p`.

**Q2 → new pending item.** `agent_doc_version()` post-reload sanity check in both JNA and koffi paths. Catches ABI mismatch before the first real FFI call.

**Q3 → resolved, no action.** Versioned cdylib and maturin's Python extension module use separate crate-type targets and filenames. Confirmed no collision.
❯ do #2cyy add tests
### Re: #2cyy — JNA mtime hot-reload implemented + tests — opus-4-6

**What changed in `NativeLib.kt`:**

- Added `libMtimeChanged(path, storedMtime)` top-level function — pure mtime comparison, extracted for testability without IntelliJ runtime
- Companion object now tracks `loadedPath` and `loadedMtime` (both `@Volatile`)
- `get()` calls `libMtimeChanged()` on every invocation; if mtime differs, calls `reload()` which does `Native.load()` from the same path (resolves through symlink to new inode)
- `reload()` is `@Synchronized` with double-check to prevent concurrent reloads
- On reload failure, keeps previous instance (graceful degradation — old library may still be valid if the overwrite completed)

**Cost:** one `stat(2)` per `get()` call — negligible vs the FFI call that follows.

**Test file:** `editors/jetbrains/src/test/kotlin/.../LibMtimeChangedTest.kt` — 4 tests:
1. mtime unchanged → false
2. file modified → true (1.1s sleep for filesystem mtime resolution)
3. nonexistent file → false (returns 0L, filtered out)
4. storedMtime matches current → false

All 4 pass. Added `junit:junit:4.13.2` to `testImplementation` in `build.gradle.kts`.

**Not yet done (separate items):**
- `#ky3a` — equivalent VS Code koffi reload
- `#wc5b` — versioned filenames (this reload works with both versioned and unversioned .so)
- `#5tjq` — `agent_doc_version()` post-reload sanity check
<!-- agent:boundary:334b384c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#6rm4] Bump version + release agent-doc with hooks feature + this fix
- [ ] [#mrs0] `agent-doc steer <FILE>` — inject steering message to claim pane via tmux send-keys
- [ ] [#qyv7] (stretch) inotify watcher for automatic real-time steering on document edit
- [ ] [#f8a4] (low priority) Port `find_code_ranges` into diff classifier for fenced-code-aware `#` handling
- [ ] [#wc5b] FFI library versioning: install as `libagent_doc-<version>.so` + `libagent_doc-current.so` symlink (Rust install + Makefile + PyPI wheel build)
- [ ] [#408p] `agent-doc gc-libs` subcommand: lockfile-tracked GC of stale versioned .so files (PID liveness via `/proc/<pid>`)
- [ ] [#ky3a] FFI library hot-reload: mtime check + koffi.unload/reload + lock file write/remove on deactivate() in VS Code native.ts (parallel to #2cyy)
- [ ] [#5tjq] Post-reload sanity check: call agent_doc_version() after hot-reload in both NativeLib.kt (JNA) and native.ts (koffi), log version string, fail fast on ABI mismatch
<!-- /agent:pending -->
