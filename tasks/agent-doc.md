---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
---



## Status Summary (compacted 2026-03-06)

### Architecture

**tmux-router v0.2.0**: Standalone Rust crate (library + CLI). `Tmux` struct (30+ tmux ops + `capture_pane()` + `send_keys_raw()`), `Registry` (file-path-to-pane mappings), `reconcile()` (2D layout sync), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()`. CLI: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`. File-name addressing (registry lookup). 48 tests. Published to crates.io + GitHub Releases.

**agent-doc v0.9.4**: Interactive document sessions with AI agents. Depends on tmux-router 0.2. Published to crates.io, PyPI, GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Paired markers, nested supported. Config-driven modes via `.agent-doc/components.toml`.
- **`agent-doc patch`**: Replace/append/prepend component content. Shell hooks (`pre_patch` stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms), bounded cycle limiting (default 3), convergence detection.
- **`agent-doc plugin install`**: Download + install JetBrains/VS Code plugins from GitHub Releases. Auto-detect IDE dirs. Aliases: `jb`/`idea`/`code`/`codium`.
- **Comment stripping**: `strip_comments()` — strips `<!-- -->` and `[//]: # (...)` but preserves component markers.
- **Auto-resync**: `prune()` runs before route/sync/claim operations.

**Tmux routing**: Window-scoped. Plugin passes `--window`. `route` auto-starts dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Both at 9/9 SPEC feature parity: Submit, Claim, Sync Layout, Tab Sync, Prompt Polling, Popup Menu, Notifications, File Filtering, CLI Resolution.

### Component Config (`.agent-doc/components.toml`)

```toml
[log]
mode = "append"        # replace (default) | append | prepend
timestamp = true       # auto-prefix with ISO timestamp
max_entries = 100      # auto-trim in append/prepend (0 = unlimited)

[status]
mode = "replace"
pre_patch = "scripts/validate.sh"   # stdin: content, stdout: transformed
post_patch = "scripts/notify.sh"    # fire-and-forget, env: COMPONENT, FILE
```

### Namespaced Components & Capture (designed, not built)

Component names support colon-separated namespacing: `agent:type:scope`. Sibling capture + log pattern: `capture:X` ↔ `log:X` linked by shared scope name.

### Domain Ontology (overlay of base kernel)

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | Bounded interaction with temporal arc |
| **Document** | entity + context | Markdown file holding conversational state |
| **Component** | scope + abstraction | Bounded, named, re-renderable region |
| **Project** | system + scope | Bounded working context; identified by `.agent-doc/` at root |
| **Pane** | focus + scope | Tmux viewport — finite attention applied to a single document |

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5–v0.6.6 | Window-scoped routing, 2D sync, Junie backend, LayoutDetector |
| v0.7.0–v0.7.2 | Snapshot-diff-apply sync, ATTACH-first reconciliation |
| v0.8.0 | tmux-router extraction (-3133 lines) |
| v0.9.0 | Components, patch, watch daemon. 150 tests. |
| v0.9.1 | Colons in component names |
| v0.9.2 | `plugin install/update/list`, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |

### Comparison with Agent of Empires

AoE is a **session manager** (TUI dashboard, multi-agent, Docker, worktrees). agent-doc is a **document workflow** (markdown is the UI). Complementary — could run agent-doc inside AoE-managed panes.

**Ideas borrowed from AoE (priority order):**
1. Multi-agent backends (Codex, Gemini CLI via `agent/` trait) — low effort, high value
2. Git worktree isolation for `agent-doc deep` fan-out
3. `agent-doc status` TUI (dashboard + interactive modes)
4. Sub-profiles per submodule in `.agent-doc/config.toml`
5. Docker sandboxing (`agent-doc run --sandboxed`)

### Pending / Not Built

- Multi-agent backends (`agent/codex.rs`, `agent/gemini.rs`)
- `agent-doc deep` — fan-out parallel subagents with git worktrees, review checkpoint for fan-in
- `agent-doc status` — Ratatui TUI with dashboard + interactive pane attachment
- Sub-profiles for submodule workflows in `.agent-doc/config.toml`
- Docker sandboxing (`--sandboxed`)
- `TmuxBatch` — command batching to reduce flicker
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend
- `agent-doc init --dashboard` — scaffold template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`

### Key Lessons

1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"

## User



## Assistant

Compacted. Reduced from 567 lines to ~105 lines.

**What was preserved:**
- Full architecture summary (updated to v0.9.4 + tmux-router v0.2.0)
- Component config reference
- Domain ontology
- Release history (added v0.9.1–v0.9.4)
- AoE comparison and borrowed ideas (priority-ordered)
- Pending/not-built list (updated: tmux-router CLI moved to done)
- Key lessons (added stash window fixes)

**What was folded in:**
- All resolved Q&A threads (submodule conversion, stash window bugs, release steps, plugin publishing, AoE comparison annotations)
- Bug reports that resulted in fixes (now captured as key lessons)
- Release log details (compressed to version table)

## User

I just did agent-doc run on this file.
Nothing happened. Please analyze why not.
Note the most recent release.

I'm going to explicitly claim this file now. So this current run will work.

Please analyze agent-doc and tmux-router for any race conditions.

## Assistant

### Most Recent Release

- **agent-doc v0.9.5** (installed), **tmux-router v0.2.1** — both added `RegistryLock` (flock-based advisory locking via `fs2`) and `validate_file_claim` (stale claim auto-pruning).

### Why the Previous Run Did Nothing

Two compounding issues:

**1. No diff detected (primary cause).** The snapshot at `.agent-doc/snapshots/6242da3...md` ends with `## User\n\n` (empty user block, line 126-128). The document at the time of the previous run also ended with `## User\n\n` — the same empty block left after the compaction. `diff::compute()` stripped comments from both, compared them, found no changes, and the skill stopped with "nothing changed." The user's new text hadn't been written yet at that point.

I did make changes. So I'm curious why there is no diff.

**2. Possible claim staleness (secondary).** If the Claude Code session in pane %0 was restarted between compaction and the failed run, the session-to-pane mapping in `sessions.json` could have been pointing at a stale PID. The `resync::prune()` at the top of `route::run()` only prunes entries with *dead panes* — if the pane itself was still alive (same pane %0, new process), the entry would survive pruning even though the new Claude session had no SKILL.md loaded or no active conversation context. The `/agent-doc tasks/agent-doc.md` command would have been sent via `tmux send-keys` to a pane that received it as raw text input but didn't have the skill registered to process it.

**Root cause:** The empty `## User` block after compaction created a state where the document matched the snapshot. The skill correctly found no diff, but "nothing happened" is confusing UX. A better behavior would be to at least print "No changes detected since last submit" to the console.

### Race Condition Analysis

#### What v0.2.1/v0.9.5 Fixed

`RegistryLock` (flock) now protects the `sessions.json` read-modify-write cycle. Functions using it:
- `register_full()` in `sessions.rs` — acquire lock, load, mutate, save
- `prune()` in `registry.rs` — acquire lock, load, filter, save
- `update_window_for_entry()` — acquire lock, load, mutate, save
- `with_registry()` / `with_registry_val()` — transactional helpers in tmux-router

#### Remaining Race Conditions

**1. Snapshot read-modify-write (NOT protected)**

`snapshot.rs` has no locking. `snapshot::load()` and `snapshot::save()` use plain `fs::read_to_string` / `fs::write`. If two subagents process the same document concurrently (e.g., rapid re-submit while a subagent is mid-cycle), this sequence is possible:

```
Subagent A: reads snapshot → computes diff → responds
Subagent B: reads snapshot → computes diff → responds
Subagent A: writes snapshot (with A's response appended)
Subagent B: writes snapshot (with B's response appended, overwrites A's)
```

The document file itself has the 3-way merge safety net, but the snapshot does not. After this race, the next diff will re-detect A's response as "new" content.

**Severity:** Low in practice — the SKILL.md subagent model serializes processing per-pane (only one Claude session per pane). Would become a real issue with `agent-doc deep` (parallel subagents) or `agent-doc watch` (daemon auto-submit).

**Fix:** Add `SnapshotLock` analogous to `RegistryLock`, or use the same lock file.

Yes. Please implement.

**2. `validate_file_claim` double-lock potential**

In `claim.rs`, `validate_file_claim()` acquires `RegistryLock`, then later `register_full()` also acquires `RegistryLock`. Since `validate_file_claim` drops its lock before `register_full` runs (it returns, releasing the RAII guard), this is safe. BUT there's a TOCTOU window between them:

```
validate_file_claim: acquire lock → load → remove stale → save → release lock
                                                                     ↕ (window)
register_full:       acquire lock → load → insert new → save → release lock
```

Another process could register for the same pane in this gap. In practice this is benign — `register_full` already deduplicates stale entries pointing to the same pane. But if another process *claims the file for a different pane* in the gap, the stale removal in `validate_file_claim` was wasted work.

**Severity:** Negligible. Self-healing.

**3. `sessions.rs` load/save without lock**

The free functions `sessions::load()` and `sessions::save()` do NOT acquire `RegistryLock` internally — they're raw I/O helpers. Callers must remember to acquire the lock. `register_full()` does this correctly, but `validate_file_claim` calls `sessions::save()` while holding a separately-acquired lock, which works but is fragile — if someone later calls `save()` without a lock, there's no compiler-enforced protection.

**Recommendation:** Make `load()` and `save()` private (or `pub(crate)`) and expose only `with_registry()` as the public API, forcing all mutations through the lock.

Implement.

**4. `resync::prune()` calls `tmux_router::prune()` which acquires its own lock**

This is correct but worth noting: if `prune()` is called from within a context that already holds `RegistryLock`, it will deadlock (flock is not reentrant on the same thread on all platforms). Currently this doesn't happen because `prune()` is always called before lock acquisition, but it's a latent hazard.

Should there be a panic or a noop with a warning?

**5. Watch daemon concurrent file access**

`watch.rs` uses per-file debounce (500ms) and cycle limiting, which prevents most races. But if the user saves the file at exactly the moment the daemon is writing back the agent response, `fs::write` in `submit.rs` (line 113) could clobber the user's save. The 3-way merge mitigates this (re-reads the file before writing), but there's still a micro-window between the re-read (line 103) and the write (line 113).

**Severity:** Low. Editor auto-save intervals are typically longer than this window.

Can a lock protect against this?

#### Summary Table

| Race | Protected? | Severity | Fix |
|------|-----------|----------|-----|
| Registry read-modify-write | Yes (flock) | N/A | Done in v0.2.1 |
| Snapshot read-modify-write | No | Low (serial pane model) | Add SnapshotLock |
| claim validate→register gap | Benign TOCTOU | Negligible | Self-healing |
| load()/save() without lock | Caller discipline | Medium (API footgun) | Make private, expose with_registry() |
| Nested lock (prune + caller) | Not reentrant | Latent deadlock | Document or assert |
| Watch daemon write window | 3-way merge | Low | Atomic rename |

## Assistant

### Addressing Each Annotation

**1. "I did make changes. So I'm curious why there is no diff."**

You are right that you made changes. The most likely explanation is a **stale snapshot**: the snapshot file matched the document at the time `agent-doc run` executed, even though you had edited the document before invoking the command. This can happen if:

- The snapshot was updated by a previous run *after* your edits were already present (e.g., a compaction cycle that wrote the snapshot with your text already in it)
- The claim was stale and the command was routed to a pane that didn't process it at all (the `send-keys` went to a pane without the skill loaded)

The second explanation is more likely given that you said "nothing happened" rather than "it said nothing changed." If the skill had actually run and found no diff, it would have printed "Nothing changed since last submit." If nothing at all happened, the command text was probably injected as raw input into a pane that wasn't listening.

Please add logging so you can investigate what happened here. I don't want to have to report the nuances of the behavior. It should be logged.

**2. "Yes. Please implement." (SnapshotLock)**

Done. Added `SnapshotLock` to `agent-doc/src/snapshot.rs`:

- `SnapshotLock` struct -- RAII flock guard using `fs2`, same pattern as `RegistryLock`
- Lock file: `<snapshot_path>.md.lock` (sibling to the snapshot file)
- `snapshot::load()` now acquires lock before reading (skips lock if no snapshot exists)
- `snapshot::save()` now acquires lock before writing
- `snapshot::delete()` now acquires lock before deleting (skips lock if no snapshot exists)
- `with_snapshot(doc, closure)` -- transactional helper for read-modify-write under a single lock hold
- Internal `load_unlocked()` / `save_unlocked()` helpers for callers already holding the lock
- Added `fs2 = "0.4"` to `agent-doc/Cargo.toml`
- All 150 tests pass

**3. "Implement." (load/save visibility)**

Done. Changed `sessions::load()` and `sessions::save()` from `pub` to `pub(crate)`:

- `load()` -- added doc comment: "Not locked internally. Callers performing read-modify-write must acquire RegistryLock first. Read-only callers can call directly for a point-in-time snapshot."
- `save()` -- added doc comment: "Not locked internally. Callers MUST hold RegistryLock before calling. Prefer tmux_router::with_registry() for safe read-modify-write cycles."
- All existing callers are within the `agent-doc` crate, so no external breakage
- External crates (if any depended on agent-doc as a library) would be forced to use `tmux_router::with_registry()` instead

**4. "Should there be a panic or a noop with a warning?"** (non-reentrant flock deadlock)

Recommendation: **noop with a warning** (`eprintln!`), not a panic. Reasoning:

- A panic in `prune()` would kill the entire CLI invocation, which is worse than skipping the prune
- `prune()` is a best-effort cleanup -- skipping it is safe because the next command will retry
- The warning makes the latent hazard visible in logs without crashing the user's workflow
- Implementation: add a thread-local `bool` flag in `registry.rs` that tracks whether the current thread holds the lock. If `prune()` detects the flag is already set, log a warning and return `Ok(0)` instead of deadlocking

Shall I implement this? It would touch `tmux-router/src/registry.rs` (adding the thread-local guard).

Yes. noop with a warning

**5. "Can a lock protect against this?"** (watch daemon micro-window)

A file lock *can* protect it, but only if the editor also respects the lock -- which no editor does. The practical options:

- **Atomic rename** (`write to temp file` then `rename`): eliminates the partial-write window. The 3-way merge already re-reads, so the only gap is between re-read and write. Atomic rename makes the write instantaneous from the filesystem's perspective. This is the standard solution.
- **flock on the document file**: would protect agent-doc against itself (e.g., watch daemon vs. manual `agent-doc run`), but editors ignore advisory locks, so it wouldn't prevent editor-vs-daemon races.
- **Both**: use flock for agent-doc-vs-agent-doc coordination, and atomic rename for editor-vs-agent-doc safety.

Recommendation: **atomic rename** is sufficient. The 3-way merge already handles the editor case correctly in practice -- the micro-window is theoretical (sub-millisecond). Atomic rename closes it completely with minimal code change (replace `fs::write(file, content)` with `write-to-tempfile + rename`).

Do an atomic rename.

> use flock for agent-doc-vs-agent-doc coordination

Is this useful or does the atomic rename already cover this. If yes, add a flock as well.

Please document the race condition analysis and whatever mitigations we have done.

### Updated Summary Table

| Race | Protected? | Severity | Fix | Status |
|------|-----------|----------|-----|--------|
| Registry read-modify-write | Yes (flock) | N/A | Done in v0.2.1 | Done |
| Snapshot read-modify-write | Yes (flock) | N/A | SnapshotLock | **Done** |
| claim validate-register gap | Benign TOCTOU | Negligible | Self-healing | N/A |
| load()/save() without lock | pub(crate) | N/A | Visibility restricted | **Done** |
| Nested lock (prune + caller) | Not reentrant | Latent deadlock | Noop + warning (proposed) | Awaiting approval |
| Watch daemon write window | 3-way merge | Low | Atomic rename (proposed) | Awaiting approval |

Please update this table in the next response.

## Assistant

All six items implemented. Tests pass (48 tmux-router, 150 agent-doc).

Please add tests for race conditions.

### 1. Diagnostic logging

Added `[diff]` and `[submit]` log lines to `agent-doc/src/diff.rs` and `agent-doc/src/submit.rs`. Every diff computation now logs: document path, snapshot path, byte lengths (raw and stripped), and whether changes were detected. Submit logs entry, diff size, and the "nothing changed" case with the file path. These will make silent no-ops self-diagnosable from logs without user intervention.

### 2. Noop + warning for nested flock

`tmux-router/src/registry.rs` now has a thread-local `REGISTRY_LOCK_HELD` flag. `RegistryLock::acquire()` checks the flag and returns an error if already held (fail-fast for unexpected nesting). New `acquire_or_skip()` returns `Ok(None)` with an `[registry]` warning instead of deadlocking. `prune()` uses `acquire_or_skip()` so it becomes a harmless no-op when called from within a locked context. The `Drop` impl clears the flag.

### 3. Atomic rename for document writes

`submit.rs` now writes via `atomic_write()`: creates a `tempfile::NamedTempFile` in the same directory as the target file, writes content, then calls `persist()` (POSIX rename). This eliminates the partial-write window entirely.

### 4. Atomic rename for snapshot writes

`snapshot.rs` `save_unlocked()` now uses the same tempfile + persist pattern.

### 5. Flock for agent-doc-vs-agent-doc coordination

Yes, the flock is useful in addition to atomic rename. Atomic rename makes individual writes atomic, but the critical section in `submit::run()` spans re-read + merge + write + snapshot-update. Without a lock, two concurrent `agent-doc` processes (e.g., watch daemon fires while a manual `run` is mid-flight) could both re-read the same state and both try to write, with the second clobbering the first's merge result. The flock serializes the entire re-read/write/snapshot sequence.

`submit::run()` now acquires `<file>.md.agent-doc.lock` (advisory flock) before the re-read/merge/write/snapshot block, released after both the document and snapshot are written.

### 6. Race condition documentation

Created `agent-doc/docs/reference/race-conditions.md` with full analysis of all identified race conditions, their mitigations, and version history.

### Updated Summary Table

| Race | Protected? | Severity | Fix | Status |
|------|-----------|----------|-----|--------|
| Registry read-modify-write | Yes (flock) | N/A | Done in v0.2.1 | **Done** |
| Snapshot read-modify-write | Yes (flock) | N/A | SnapshotLock | **Done** |
| claim validate-register gap | Benign TOCTOU | Negligible | Self-healing | N/A |
| load()/save() without lock | pub(crate) | N/A | Visibility restricted | **Done** |
| Nested lock (prune + caller) | Thread-local guard | N/A | noop + warning via `acquire_or_skip()` | **Done** |
| Watch daemon write window | Atomic rename + flock | N/A | tempfile+persist + doc advisory lock | **Done** |

## User

Please add tests for race conditions.

## Assistant

Added race condition tests to both repos. All tests pass cleanly.

### tmux-router (`src/registry.rs`) -- 8 new tests

1. **`acquire_lock_succeeds`** -- `RegistryLock::acquire` returns `Ok`
2. **`lock_released_on_drop`** -- after dropping a lock, a second `acquire` succeeds
3. **`nested_acquire_returns_error`** -- re-entrant `acquire` on the same thread returns an error containing "already held"
4. **`acquire_or_skip_returns_none_on_reentrant`** -- `acquire_or_skip` returns `Ok(None)` when the thread already holds the lock
5. **`acquire_or_skip_returns_some_when_free`** -- returns `Some(lock)` when free
6. **`with_registry_read_modify_write`** -- inserts an entry under lock and verifies it persists
7. **`with_registry_val_returns_value`** -- closure return value propagates through
8. **`concurrent_with_registry_serializes_writes`** -- 10 threads with a `Barrier` all write different keys; final registry has all 10 entries (proves flock serialization works)

### agent-doc -- 11 new tests

**`snapshot.rs`:**
1. **`flock_acquire_and_release_on_drop`** -- raw `fs2` flock acquire, release, re-acquire cycle
2. **`flock_serializes_concurrent_access`** -- 10 threads increment a counter under flock; final value is exactly 10 (proves no lost updates)
3. **`atomic_write_via_tempfile_produces_correct_content`** -- tempfile+persist writes expected content
4. **`atomic_write_overwrites_existing`** -- atomic write replaces existing file
5. **`concurrent_atomic_writes_no_partial_content`** -- 20 threads atomically write; final content is always a complete valid string (no partial/corrupted reads)

**`submit.rs`:**
6. **`acquire_doc_lock_succeeds`** -- `acquire_doc_lock` returns a valid locked file
7. **`doc_lock_released_on_drop`** -- after dropping, second acquire succeeds
8. **`atomic_write_correct_content`** -- `atomic_write()` produces correct file
9. **`atomic_write_overwrites_existing`** -- overwrites old content
10. **`concurrent_atomic_writes_no_corruption`** -- 20 concurrent writers; final content is a valid complete write
11. **`merge_clean_no_conflicts`** -- 3-way merge with no user edits preserves agent response (uses `git merge-file` with explicit `current_dir` for CWD isolation)

### Test results

- **tmux-router:** 56 passed, 0 failed (8 new)
- **agent-doc:** 140 passed, 0 failed, 2 ignored (11 new); 21 integration tests pass
- **clippy:** clean (only pre-existing `with_snapshot` unused warning)

## User

Add a test for with_snapshot

## Assistant

Added three tests for `with_snapshot` in `/home/brian/work/btakita/agent-loop/agent-doc/src/snapshot.rs`:

1. **`with_snapshot_creates_when_none_exists`** -- closure receives `None` when no snapshot exists, returns `Some(content)`, verifies snapshot is created
2. **`with_snapshot_reads_and_updates_existing`** -- seeds a snapshot, verifies closure receives existing content, updates it, verifies new content persisted
3. **`with_snapshot_skips_write_when_closure_returns_none`** -- seeds a snapshot, closure returns `None`, verifies original content unchanged

All tests use `set_current_dir` to the temp dir (since `path_for` returns relative paths) and run with `--test-threads=1` to avoid CWD races. All 16 snapshot tests pass. The `dead_code` warning on `with_snapshot` remains (it's only used in tests for now).

## User

How can we fix the dead_code warning?

## Assistant

Four options, ordered by preference:

**Option 1 (recommended): Use `with_snapshot` in `submit.rs`**
In `submit.rs` lines 112-131, the code does a manual lock-acquire, re-read, write, snapshot-save, lock-release sequence. The snapshot `save()` call on line 129 acquires its own separate `SnapshotLock`, meaning the doc lock and snapshot lock are briefly independent. Replacing the standalone `snapshot::save(file, &final_content)?` with `snapshot::with_snapshot()` doesn't directly help here since `with_snapshot` is for read-modify-write on snapshots, and submit just does a write. However, if there's any future code path that needs to read-then-update a snapshot atomically, `with_snapshot` is the right tool.

The real win: `with_snapshot` is designed for the watch daemon and `agent-doc deep` (future features). When those are implemented, `with_snapshot` gets used in production code and the warning disappears naturally.

**Option 2: `#[cfg(test)]` the function**
Mark `with_snapshot` as `#[cfg(test)]` since it's only used in tests today. Downside: you'd have to remove the attribute when you actually need it in production code.

**Option 3: `#[allow(dead_code)]` with a comment**
```rust
#[allow(dead_code)] // Used by tests; production use planned for watch daemon + deep
pub fn with_snapshot<F>(doc: &Path, f: F) -> Result<()>
```
Simple, explicit about intent. Not ideal but honest.

**Option 4: Delete it**
Remove `with_snapshot` and its tests. Re-add when needed. YAGNI principle. Downside: you just wrote the tests.

My recommendation is **Option 3** for now -- it's honest, zero-risk, and preserves the work. When the watch daemon or `agent-doc deep` lands, the `#[allow]` gets removed as part of that PR. Option 1 doesn't apply cleanly since `submit.rs` doesn't need read-modify-write on the snapshot.

## User

Why did we implement with_snapshot if we are not using it? Is it mainly for programmers using the library?

## Assistant

Honest answer: it was premature. The subagent that implemented the race condition mitigations added `with_snapshot` speculatively, anticipating that the watch daemon and `agent-doc deep` command would need a transactional read-modify-write pattern on the snapshot file. But neither of those features exist yet, so nothing calls it in production -- only the tests do.

And no, it's not mainly for library consumers. `agent-doc` is an application binary, not a library others import. The `pub` visibility was cargo-cult from `with_registry` in `tmux-router`, which actually IS a library where `pub` makes sense. Here it's just noise.

The right call is **Option 4: delete it**. YAGNI. The function, its tests, and the `pub` visibility should all go. When the watch daemon or `deep` command actually lands, we add it back as part of that PR with a real caller from day one. The `SnapshotLock` type can stay since `save()` and `load()` use it.

The tests we lose aren't wasted work -- they proved the locking logic is sound. That knowledge carries forward. But shipping dead code with an `#[allow(dead_code)]` annotation is a code smell we don't need.

Want me to go ahead and remove `with_snapshot` and its tests?

Yes

## User
