# Race Condition Analysis

Concurrency hazards in agent-doc and tmux-router, with mitigations.

## Protected (Resolved)

### Registry read-modify-write

**Location:** `tmux-router/src/registry.rs`

`sessions.json` is shared state between concurrent `agent-doc` processes (claim, route, sync, resync). Without protection, two processes could read the same registry, each make a change, and one would clobber the other.

**Mitigation:** `RegistryLock` (flock-based advisory lock via `fs2`). All mutating operations acquire the lock: `register_full()`, `prune()`, `update_window_for_entry()`, `with_registry()`, `with_registry_val()`.

**Since:** v0.2.1 / v0.9.5

### Snapshot read-modify-write

**Location:** `agent-doc/src/snapshot.rs`

The snapshot file tracks the last-submitted document state for diff computation. Concurrent submits (e.g., `agent-doc run` + `agent-doc watch`) could read the same snapshot, both compute diffs, and one would overwrite the other's updated snapshot.

**Mitigation:** `SnapshotLock` (flock-based, same pattern as `RegistryLock`). `load()`, `save()`, and `delete()` all acquire the lock. `with_snapshot()` provides a transactional read-modify-write helper.

**Since:** v0.9.5

### load()/save() API footgun

**Location:** `agent-doc/src/sessions.rs`

The raw `load()` and `save()` functions do not acquire locks internally. Callers must remember to hold `RegistryLock`. If exposed publicly, new code could easily introduce unprotected mutations.

**Mitigation:** Changed visibility to `pub(crate)`. External callers must use `tmux_router::with_registry()` which enforces locking.

**Since:** v0.9.5

### Nested lock deadlock (flock non-reentrancy)

**Location:** `tmux-router/src/registry.rs`

`flock` is not reentrant on the same thread on Linux. If `prune()` is called from within a context that already holds `RegistryLock`, it would deadlock.

**Mitigation:** Thread-local `REGISTRY_LOCK_HELD` flag. `RegistryLock::acquire()` checks the flag and returns an error if already held. `acquire_or_skip()` returns `None` with a warning instead. `prune()` uses `acquire_or_skip()` so it becomes a no-op when called from within a locked context.

**Since:** v0.9.6

### Lazy parallelization (skill-level)

**Location:** `.claude/skills/agent-doc/SKILL.md`, `agent-doc/src/submit.rs`

When the user submits multiple documents (`/agent-doc A`, `/agent-doc B`), the skill must decide whether to process them sequentially or in parallel. Claude Code processes messages sequentially in the main context — Document B blocks until A completes.

**Execution model:**
- **Single document (default):** Process directly in the main agent context. No subagent, no token overhead.
- **Parallel documents:** Use `Agent(run_in_background: true)` for the 2nd+ document. Each background subagent processes its document cycle independently.
- **Same document re-submit:** Filesystem locks serialize access. The second invocation blocks until the first completes.

**Safety guarantees (tested):**
1. Different files: Independent flock + atomic rename — no shared lock contention, no interference (`parallel_different_files_no_interference`)
2. Same file: flock serializes the read-modify-write cycle — both writes land in order (`same_file_serialized_by_flock`)
3. No partial reads: flock prevents a reader from seeing a half-written document (`flock_prevents_partial_read_during_write`)

**Since:** v0.9.6

## Mitigated (Low Residual Risk)

### Watch daemon write window

**Location:** `agent-doc/src/submit.rs`, `agent-doc/src/watch.rs`

If the user saves a file at the exact moment the daemon is writing back the agent response, the write could clobber the user's save. The 3-way merge re-reads the file before writing, but there is a micro-window between re-read and write.

**Mitigations (two layers):**
1. **Atomic rename:** Document writes use `tempfile::NamedTempFile` + `persist()` (rename). The write is instantaneous from the filesystem perspective, eliminating partial-read hazards.
2. **Advisory flock:** `submit::run()` acquires an advisory lock on `<file>.md.agent-doc.lock` before the re-read/write/snapshot sequence. This serializes concurrent `agent-doc` processes writing to the same document (e.g., watch daemon vs. manual run). Editors do not respect advisory locks, so this only protects agent-doc-vs-agent-doc races.

**Since:** v0.9.6

### claim validate-to-register TOCTOU gap

**Location:** `agent-doc/src/claim.rs`

`validate_file_claim()` acquires `RegistryLock`, removes stale claims, releases the lock. Then `register_full()` acquires the lock again and inserts the new claim. Another process could claim the same file in the gap between the two lock acquisitions.

**Residual risk:** Negligible. `register_full()` deduplicates entries pointing to the same pane, so the system self-heals. The stale removal in `validate_file_claim` may be wasted work in the rare case, but no data is lost.

## Snapshot write atomicity

**Location:** `agent-doc/src/snapshot.rs`

Snapshot writes now use atomic rename (tempfile + persist) in addition to flock, ensuring that concurrent readers never see a partial snapshot file.

**Since:** v0.9.6
