# Spec: Commit Reliability — Uncommitted Response Patches

**Problem:** Agent responses are written to documents but `git::commit` is not called, leaving the response visible in the editor but uncommitted. Preflight catches this on the next cycle, but there's a window where the user sees green gutter instead of no gutter for the agent response.

**Confirmed via `cycles.jsonl`:** Entries with no `commit_hash` field indicate `git_head_hash` returned None after a commit attempt — observed for `corky.md` at 18:49:51 and 18:50:15 on 2026-04-10. The OAuth response written after 18:50 had no commit log entry for 20 minutes until swept by manual preflight.

---

## Failure Modes

### 1. IPC timeout → `process::exit(75)` (primary, frequent)

**File:** `src/agent-doc/src/write.rs:970`

```rust
std::process::exit(75); // EX_TEMPFAIL
```

When the JetBrains plugin doesn't acknowledge in time, `run_stream` saves a patch file to `.agent-doc/patches/<hash>.json` and hard-exits. The commit check in `main.rs` is unreachable — the entire process terminates. The plugin applies the patch later (file + snapshot updated), but `git::commit` is never called.

**Scope:** Affects all environments with the JetBrains plugin installed.

### 2. `run_stream` returns `Err` → `result?` short-circuits commit

**File:** `src/agent-doc/src/main.rs`

```rust
let result = write::run_stream(&file, baseline.as_deref(), force_disk);
result?;           // ← early return propagates error
if do_commit {     // ← never reached
    git::commit(&file)?;
}
```

If `run_stream` returns `Err` (e.g., `verify_pane_ownership` fails, `apply_patches_with_overrides` fails, empty response guard fires) **after** the snapshot was already saved, the snapshot has new content but `git::commit` is never called. The `result?` propagation bypasses the commit block.

**Scope:** Any write that fails after partial completion.

### 3. Concurrent git lock — staged but not committed

**File:** `src/agent-doc/src/git.rs`

`git update-index --cacheinfo` stages the snapshot successfully, then `git commit` fails due to `index.lock` from another concurrent session. No retry logic. The staged content accumulates in the git index and gets swept into the next successful `git commit` from any document. This explains `committed=False` log entries that nonetheless appear in `git log` (committed via another doc's preflight).

**Scope:** Multi-session workspaces (several agent-doc sessions running in parallel).

### 4. Session interruption between write and commit

Agent process killed/timed out after `agent-doc write` updates snapshot but before `--commit` triggers. Already handled by preflight step 2 on the next cycle, so recovery is automatic — just delayed by one cycle.

---

## Fixes

### Fix 1 — Commit before `process::exit(75)` ★ primary

**File:** `src/agent-doc/src/write.rs` (~line 968)

Before exiting on IPC timeout, save the snapshot (already computed as `content_ours`) and call `git::commit`:

```rust
// IPC timeout path
let _ = snapshot::save(file, &content_ours);
if crate::git::is_in_git_repo(file) {
    let _ = crate::git::commit(file);
}
std::process::exit(75);
```

`content_ours` is already computed at this point (baseline + patches applied). Committing here ensures the agent response is tracked before the process dies, even if the plugin hasn't applied the patch yet.

### Fix 2 — Commit before propagating write error

**File:** `src/agent-doc/src/main.rs`

Reorder so commit runs even when `run_stream` returns `Err`:

```rust
let result = write::run_stream(&file, baseline.as_deref(), force_disk);
if do_commit {
    if git::is_in_git_repo(&file) {
        if let Err(e) = git::commit(&file) {
            eprintln!("[commit] warning: {}", e);
        }
    }
}
result?;  // propagate write error AFTER commit attempt
```

### Fix 3 — Retry with backoff on git lock

**File:** `src/agent-doc/src/git.rs`

Detect `index.lock` in stderr and retry up to 3 times with exponential backoff:

```rust
let mut attempts = 0;
let commit_output = loop {
    let out = Command::new("git")
        .current_dir(&git_root)
        .args(["commit", "-m", &msg, "--no-verify"])
        .output()?;
    if out.status.success() { break out; }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if attempts < 3 && (stderr.contains("index.lock") || stderr.contains("Unable to create")) {
        attempts += 1;
        std::thread::sleep(std::time::Duration::from_millis(50 * (1 << attempts)));
        continue;
    }
    break out;
};
```

### Fix 4 — Plugin post-apply commit (defense-in-depth)

After the JetBrains plugin successfully applies a patch, have the IPC handler call `git::commit` via FFI. This ensures a commit fires in the IPC success path even if the shell-side commit was skipped for any reason.

**File:** `src/agent-doc/src/ffi.rs` (new export `agent_doc_commit`), `editors/jetbrains/.../PatchWatcher.kt` (call after successful apply).

Follows the FFI-first convention: all git logic in Rust, plugin makes FFI call.

### Fix 5 — Preflight cross-document sweep (catch-all backstop)

When preflight runs for any document, scan all tracked documents in the same project for snapshot-ahead-of-committed state and commit them. Turns preflight into a sweep, not just a per-document operation.

**Implementation sketch:** Load `sessions.json` (all tracked docs), call `git::commit` for any doc where snapshot mtime > last `git log -1` mtime.

---

## Priority (HEAD)

| Fix | Impact | Complexity | Notes |
|-----|--------|------------|-------|
| Fix 1 (IPC timeout commit) | High — eliminates primary case | Low — ~5 lines | Do first |
| Fix 2 (commit before `result?`) | Medium — error propagation | Low — reorder 4 lines | Do with Fix 1 |
| Fix 3 (retry on lock) | Medium — concurrent sessions | Low — add retry loop | Do with Fix 1 |
| Fix 5 (preflight sweep) | High — catches all future cases | Medium — scan + commit | Do after |
| Fix 4 (plugin post-apply) | Low — defense-in-depth | Medium — FFI + Kotlin | Do last |

Fixes 1–3 are a single small PR. Fix 5 is a follow-up. Fix 4 is optional defense-in-depth.
