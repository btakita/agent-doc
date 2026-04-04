# Debounce Test Plan: 6 Critical Gaps

## Overview

This document outlines 6 test cases for critical gaps in the agent-doc debounce system. The tests are specified but **not yet implemented** — this is the test plan/spec only.

The debounce system manages timing and synchronization across:
- **In-process state**: `LAST_CHANGE` HashMap tracks edit timestamps per file
- **Cross-process state**: `.agent-doc/typing/<hash>` files signal active editing
- **Status signals**: `.agent-doc/status/<hash>` files track agent operation state
- **Reactive mode**: CRDT-mode documents watch with zero debounce for real-time sync

**Source files:**
- `src/agent-doc/src/debounce.rs` (lines 1–378)
- `src/agent-doc/src/preflight.rs` (preflight timing, line ~366)
- `src/agent-doc/src/watch.rs` (reactive mode, line ~27–28, ~116–118)
- `src/agent-doc/src/route.rs` (1500ms sleep, line 998)

---

## Gap 1: Mtime Granularity

### What it validates
- In-process debounce (`is_idle`) is immune to filesystem mtime granularity
- Cross-process debounce (`is_typing_via_file`) correctly handles coarse granularity
- The 3-second preflight timeout doesn't incorrectly skip waiting for settling

### The problem
File systems report modification time at different granularities:
- **HFS+** (macOS): 1 second resolution
- **ext4** (Linux): 1 nanosecond (configurable to 1ms)
- **NTFS** (Windows): 100 nanoseconds

On HFS+, two edits 500ms apart may have identical mtime. Cross-process `is_typing_via_file()` reads the timestamp from `.agent-doc/typing/<hash>` on disk and may miss rapid edits if granularity is coarse.

### Prerequisites
- Test fixture that can mock filesystem mtime granularity
- Tests on both HFS+ (1s) and ext4 (ns) systems, or manual time simulation via `touch -t`

### Expected behavior (if gap is fixed)
- 10 rapid `document_changed()` calls within 100ms → all recorded in `LAST_CHANGE` map
- `is_idle()` returns `false` for all 10
- `await_idle_via_file()` correctly detects that typing is still active
- Preflight waits the full 500ms debounce, not returning early on mtime-matching files

### Current behavior (likely to fail)
- On systems with >100ms mtime granularity, the 2nd+ edits don't update file mtime
- `is_typing_via_file()` reads stale mtime and incorrectly reports idle
- Preflight proceeds before user typing finishes, causing premature agent submit

### Test signatures
```rust
#[test]
fn test_mtime_granularity_100ms_rapid_edits()

#[test]
fn test_mtime_granularity_1s_coarse_system()
```

---

## Gap 2: Untracked File Edge Case

### What it validates
- `is_tracked()` correctly distinguishes "never-tracked" from "tracked and idle"
- A non-blocking probe can use `is_tracked()` to avoid unnecessary `await_idle()` calls
- Untracked files are safely handled (don't cause `await_idle()` to block indefinitely)

### The problem
- `is_tracked(file)` returns `true` only if `document_changed()` was called at least once
- `is_idle(file)` returns `true` for both "never-tracked" AND "tracked but idle" files
- A probe needs to distinguish these to avoid blocking on untracked files:
  - "never-tracked" → conservative (assume not idle, skip the wait)
  - "tracked and idle" → safe to proceed

The API makes this distinction subtle. A probe might unconditionally call `await_idle()` on an untracked file, which works (returns immediately) but is inefficient.

### Prerequisites
- Fresh `LAST_CHANGE` state (test isolation)
- Two separate test files: one tracked, one not

### Expected behavior (if gap is fixed)
- `is_tracked("/tmp/never-seen.md")` returns `false`
- `is_idle("/tmp/never-seen.md", 1500)` returns `true` (safe default)
- After `document_changed("/tmp/tracked.md")`: `is_tracked()` returns `true`
- `is_idle()` returns `false` immediately after change, then `true` after debounce

### Current behavior (likely to fail)
- The distinction is correct in code, but its use is not enforced by the type system
- Callers might ignore `is_tracked()` and call `await_idle()` unconditionally
- For untracked files, `await_idle()` returns immediately (due to `is_idle()` returning `true`)
- This is a silent fallback, not explicit error handling

### Test signatures
```rust
#[test]
fn test_untracked_file_is_idle_returns_true()

#[test]
fn test_untracked_file_is_tracked_returns_false()

#[test]
fn test_tracked_file_is_tracked_returns_true()

#[test]
fn test_probe_pattern_untracked_skips_await()
```

---

## Gap 3: Hash Collision Risk

### What it validates
- No hash collisions occur for a reasonable set of test paths
- If a collision is detected, behavior is defined (error or graceful handling)
- Typing indicator files are correctly isolated per file path

### The problem
`typing_indicator_path()` hashes the file path using `DefaultHasher` to `.agent-doc/typing/<hash>`. If file A and file B hash to the same value, they share one typing indicator file:
- When A is edited, the indicator shows "A is typing"
- When B checks if it's typing, it reads A's indicator (false positive)
- When A's timestamp expires, B's timestamp also expires (false negative)

`DefaultHasher` uses SipHash-1-3, designed for HashMap security (not collision resistance). Over a large file set or with adversarial paths, collisions are possible. Current code has no detection or mitigation.

### Prerequisites
- A set of "likely to collide" file paths (same file name, different directories)
- Or: exhaustive collision test for 10,000+ files from a realistic project layout

### Expected behavior (if gap is fixed)
- Generate 10,000 distinct paths from a realistic project
- Compute hash for each via `typing_indicator_path()`
- All hashes are unique (or collisions are detected and logged)
- No two files share the same indicator file

### Current behavior (likely to fail)
- No collision detection
- Over a large file set, collisions are statistically possible
- No cleanup mechanism for typing indicator files (`.agent-doc/typing/` grows unboundedly)

### Test signatures
```rust
#[test]
fn test_hash_collision_no_collisions_for_common_paths()

#[test]
fn test_hash_collision_cleanup_removes_stale_indicators()
```

---

## Gap 4: Reactive Mode CRDT

### What it validates
- Reactive mode (zero debounce) handles CRDT merge failure gracefully
- Infinite loop prevention works correctly when CRDT merge produces non-idempotent output
- Corrupted CRDT state does not silently corrupt the document

### The problem
Reactive mode watches CRDT-mode documents with `Duration::ZERO` debounce. Every file change triggers an immediate `run::run()` call. `run()` calls `crdt::merge()` to handle concurrent edits.

**Failure modes:**
1. If CRDT merge fails (malformed state file, corruption), the document may be lost or partially overwritten
2. If `run()` is non-idempotent (writes differ each time), the file-watch loop re-triggers: `watch → run() → write → watch → ...`

No test for merge failure recovery or loop prevention in reactive mode.

### Prerequisites
- A CRDT-mode document (`agent_doc_format: template`, `agent_doc_write: crdt`)
- Ability to corrupt `.agent-doc/crdt/<hash>.yrs` file
- A test that induces merge failure
- Access to watch event loop internals for loop prevention test

### Expected behavior (if gap is fixed)
**Merge failure:**
- File change triggers `run()` → `crdt::merge()` reads corrupted state file
- `merge()` returns `Err` instead of panicking or returning garbage
- Document is NOT written (no partial/corrupted content)
- Error is logged and propagated to watch daemon
- Watch daemon logs error and continues monitoring (doesn't crash)

**Infinite loop prevention:**
- A cycle counter prevents >N consecutive cycles (e.g., `max_cycles = 5`)
- Or: content hash convergence detection stops the loop (same content = no re-run)
- Watch daemon logs loop warnings and halts after `max_cycles`

### Current behavior (likely to fail)
- If CRDT state is corrupted, `merge()` may panic or return invalid data
- Document is overwritten with corrupted/partial content
- Watch daemon may crash or retry indefinitely
- Infinite loop possible if `run()` produces non-idempotent output

### Test signatures
```rust
#[test]
fn test_reactive_mode_crdt_merge_failure_handling()

#[test]
fn test_reactive_mode_infinite_loop_prevention()
```

---

## Gap 5: Status File Staleness

### What it validates
- `get_status_via_file()` correctly enforces the 30-second timeout
- Timestamps exactly at the boundary return the correct status
- Clock skew (timestamp in future) doesn't break behavior
- Edge cases (missing file, malformed format) are handled safely

### The problem
`get_status_via_file(file)` reads `.agent-doc/status/<hash>` on disk. The file contains `"status:timestamp_ms"`. If the timestamp is older than 30 seconds, status is considered stale and returns `"idle"`.

**Gap:** No test for the 30-second timeout boundary. If an agent operation crashes and forgets to clear the status file, it remains `"busy"` for up to 30 seconds, blocking subsequent operations.

The 30-second timeout is hardcoded in `debounce.rs` line 260:
```rust
if now.saturating_sub(ts) < 30_000 {
    return parts[0].to_string();
}
```

### Prerequisites
- Ability to mock system clock OR set file timestamps
- A test file with status files at various ages

### Expected behavior (if gap is fixed)
- Status file created at T=0 with status `"generating"`
- At T=29s: `get_status_via_file()` returns `"generating"`
- At T=30s: `get_status_via_file()` returns `"idle"` (timeout applies)
- At T=31s: `get_status_via_file()` returns `"idle"`
- Future timestamp (clock skew): behavior is defined (treat as not stale? or safe default?)

### Current behavior (likely to fail)
- 30-second timeout is hardcoded, not configurable
- If an agent takes 25+ seconds, a timeout is likely during its execution
- No distinction between "truly idle" and "timed out" status
- If clock skews backward, stale files might never expire

### Test signatures
```rust
#[test]
fn test_status_file_staleness_30s_timeout()

#[test]
fn test_status_file_write_includes_current_timestamp()
```

---

## Gap 6: Timing Constants (Preflight 1500ms)

### What it validates
- The 1500ms debounce constant in preflight is configurable OR documented with rationale
- The 3-second preflight timeout has sufficient margin over the debounce period
- Constants are explained in code comments (why chosen, when to change, trade-offs)

### The problem
`preflight.rs` line ~366 hardcodes:
```rust
let typing_active = agent_doc::debounce::is_typing_via_file(&file_str, 1500);
```

The 1500ms is hardcoded, not configurable. Different projects might need different debounce windows:
- **Slow CI**: might need 5000ms to settle
- **Fast local**: might want 500ms

Also, `route.rs` line 998 has another hardcoded constant:
```rust
std::thread::sleep(std::time::Duration::from_millis(1500));
```

### Prerequisites
- Access to preflight config or frontmatter settings
- Ability to pass `debounce_ms` to `preflight::run()`
- A test that compares different debounce values

### Expected behavior (if gap is fixed)
- Frontmatter can specify `agent_doc_debounce_ms: 5000` (for slow CI)
- `preflight::run()` respects the frontmatter value
- Or: CLI flag `--debounce=<ms>` overrides default
- Default is 1500ms (reasonable compromise)
- Preflight timeout (3 seconds) is appropriately longer than debounce
- Each hardcoded constant has a comment explaining:
  - Why this value was chosen
  - When it should be changed
  - Trade-offs (responsiveness vs. battery/CPU)

### Current behavior (likely to fail)
- 1500ms is hardcoded in `preflight.rs`
- No frontmatter field controls it
- No CLI override
- All projects use the same debounce, regardless of build/edit speed
- Constants lack documentation (only visible in test evals, line 32 of debounce.rs)

### Test signatures
```rust
#[test]
fn test_preflight_timing_1500ms_is_configurable()

#[test]
fn test_preflight_3s_timeout_is_sufficient_for_debounce()

#[test]
fn test_timing_constants_are_documented()
```

---

## Summary Table

| Gap | Test Count | Key Risk | Mitigation |
|-----|-----------|----------|-----------|
| 1: Mtime Granularity | 2 | HFS+ coarse timing breaks cross-process detection | In-process state is immune; cross-process needs validation |
| 2: Untracked File Edge | 4 | Caller confusion, inefficient awaits | Type-safe API or clear documentation |
| 3: Hash Collision | 2 | Path collisions cause false positives/negatives | Collision-resistant hash or per-file locking |
| 4: Reactive CRDT | 2 | Merge failure or infinite loop on buggy output | Error handling + cycle counter |
| 5: Status Staleness | 2 | Hung agent blocks all subsequent operations | Configurable timeout + documentation |
| 6: Timing Constants | 3 | One-size-fits-all debounce breaks projects | Configurable via frontmatter + documented constants |
| **Total** | **15 tests** | | |

---

## Test Implementation File

Full pseudocode and docstrings in: `src/agent-doc/tests/debounce_gaps_test_plan.rs`

Each test includes:
- **Spec**: What the gap is and why it matters
- **Prerequisites**: Setup required before running
- **Expected behavior (if gap is fixed)**: Passing test criteria
- **Current behavior (likely to fail)**: Why the test would fail today

---

## Next Steps

1. **Review** this plan with the team for alignment on priorities
2. **Implement** tests in `debounce_gaps_test_plan.rs` (currently `#[ignore]`)
3. **Run** tests to confirm which gaps are real
4. **Fix** gaps in order of criticality:
   - Gap 4 (CRDT merge failure) — silent data loss is critical
   - Gap 5 (Status staleness) — blocks operations, needs timeout config
   - Gap 1 (Mtime granularity) — affects cross-process only
   - Gap 6 (Timing constants) — UX/flexibility issue
   - Gap 3 (Hash collision) — unlikely but possible
   - Gap 2 (Untracked edge) — API clarity issue

---

## References (HEAD)

- **debounce.rs**: In-process state, cross-process signals, typing indicators, status files
- **preflight.rs**: Pre-agent checks, 3-second timeout, debounce wait
- **watch.rs**: Reactive mode, zero-debounce CRDT watching
- **route.rs**: 1500ms hardcoded sleep (line 998)
