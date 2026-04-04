# Debounce Gaps — Quick Reference

## Test Plan Location
- **Plan (pseudocode)**: `src/agent-doc/tests/debounce_gaps_test_plan.rs`
- **Summary**: `tasks/software/debounce-test-plan-summary.md`

## Quick Checklist: 6 Gaps, 15 Tests

### Gap 1: Mtime Granularity (2 tests)
```
├─ test_mtime_granularity_100ms_rapid_edits
└─ test_mtime_granularity_1s_coarse_system
```
**Risk**: HFS+ 1-second resolution → cross-process `is_typing_via_file()` misses edits
**Mitigation**: In-process `is_idle()` uses `Instant`, immune to mtime granularity

### Gap 2: Untracked File Edge (4 tests)
```
├─ test_untracked_file_is_idle_returns_true
├─ test_untracked_file_is_tracked_returns_false
├─ test_tracked_file_is_tracked_returns_true
└─ test_probe_pattern_untracked_skips_await
```
**Risk**: Caller confusion; calling `await_idle()` on untracked files is inefficient
**Mitigation**: Type-safe API or clear docs: check `is_tracked()` before `await_idle()`

### Gap 3: Hash Collision (2 tests)
```
├─ test_hash_collision_no_collisions_for_common_paths
└─ test_hash_collision_cleanup_removes_stale_indicators
```
**Risk**: Path A and B hash to same value → false positives/negatives in typing indicator
**Root Cause**: `DefaultHasher` not collision-resistant; no cleanup for `.agent-doc/typing/`

### Gap 4: Reactive CRDT (2 tests)
```
├─ test_reactive_mode_crdt_merge_failure_handling
└─ test_reactive_mode_infinite_loop_prevention
```
**Risk**: Corrupted CRDT state → silent document loss; non-idempotent `run()` → infinite loop
**Mitigation**: Error handling on merge failure; cycle counter or content hash convergence

### Gap 5: Status Staleness (2 tests)
```
├─ test_status_file_staleness_30s_timeout
└─ test_status_file_write_includes_current_timestamp
```
**Risk**: Crashed agent doesn't clear status file → blocked for 30 seconds
**Root Cause**: Hardcoded 30-second timeout in `debounce.rs` line 260

### Gap 6: Timing Constants (3 tests)
```
├─ test_preflight_timing_1500ms_is_configurable
├─ test_preflight_3s_timeout_is_sufficient_for_debounce
└─ test_timing_constants_are_documented
```
**Risk**: Hardcoded 1500ms preflight debounce → one-size-fits-all fails for slow CI/fast local
**Root Cause**: Preflight line ~366 hardcodes `is_typing_via_file(..., 1500)`

---

## Hardcoded Constants Location

| Constant | File | Line | Context | Notes |
|----------|------|------|---------|-------|
| 1500ms | preflight.rs | ~366 | Preflight debounce window | Hardcoded, no override |
| 1500ms | route.rs | 998 | Sleep duration | Hardcoded |
| 3000ms | preflight.rs | ~300s (implied) | Preflight timeout | 3-second wait for typing + mtime settle |
| 30000ms | debounce.rs | 260 | Status file expiry | Hardcoded, no config |
| 500ms | preflight.rs | ~17 (spec) | mtime age requirement | Minimum file age before submit |
| 100ms | debounce.rs | 101 | Poll interval in await_idle | Responsive, not configurable |

---

## Criticality Ranking

1. **CRITICAL**: Gap 4 (Reactive CRDT) — silent data loss
2. **HIGH**: Gap 5 (Status staleness) — blocks all operations
3. **MEDIUM**: Gap 1 (Mtime granularity) — affects cross-process only
4. **MEDIUM**: Gap 6 (Timing constants) — UX/flexibility issue
5. **LOW**: Gap 3 (Hash collision) — unlikely but possible
6. **LOW**: Gap 2 (Untracked edge) — API clarity, not a correctness issue

---

## Testing Approach

### Phase 1: Verify Gaps Are Real
- Implement all 15 tests with `#[ignore]`
- Run: `cargo test --lib debounce_gaps_test_plan -- --ignored --nocapture`
- Count how many fail (expected: many)

### Phase 2: Fix High-Criticality Gaps
- Focus on Gap 4 (CRDT) and Gap 5 (Status) first
- Write defensive code before implementing tests
- Re-run tests; should pass after fix

### Phase 3: Fix Medium-Criticality Gaps
- Gap 1 and Gap 6 may require configuration changes
- Consider frontmatter field: `agent_doc_debounce_ms: <u64>`
- Propagate through `preflight::run()` signature

### Phase 4: Polish Low-Criticality Gaps
- Gap 3: collision detection + cleanup scheduler
- Gap 2: API documentation or breaking change for type safety

---

## Key Files to Modify

| File | Gap(s) | Change |
|------|--------|--------|
| `src/agent-doc/src/debounce.rs` | All | Add test fixtures, improve docs |
| `src/agent-doc/src/preflight.rs` | 6 | Accept `debounce_ms` parameter; remove hardcode |
| `src/agent-doc/src/watch.rs` | 4 | Add loop prevention, merge error handling |
| `src/agent-doc/src/crdt.rs` | 4 | Defensive parsing, error propagation |
| `src/agent-doc/src/frontmatter.rs` | 6 | Add `agent_doc_debounce_ms` field |
| (new) `src/agent-doc/tests/debounce_gaps_test_plan.rs` | All | Test plan |

---

## Test Data / Fixtures

```rust
// Mtime granularity test
let tmp = tempfile::TempDir::new().unwrap();
let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
std::fs::create_dir_all(&agent_doc_dir).unwrap();
let doc = tmp.path().join("test.md");
std::fs::write(&doc, "initial").unwrap();

// Status file staleness test
let status_dir = tmp.path().join(".agent-doc").join("status");
std::fs::create_dir_all(&status_dir).unwrap();
let status_file = status_dir.join("test_hash");
// Write: "generating:123456789"

// CRDT merge failure test
let doc = tmp.path().join("reactive.md");
let frontmatter = r#"---
agent_doc_format: template
agent_doc_write: crdt
---"#;
std::fs::write(&doc, frontmatter).unwrap();
let crdt_dir = tmp.path().join(".agent-doc").join("crdt");
std::fs::create_dir_all(&crdt_dir).unwrap();
std::fs::write(crdt_dir.join("corrupted.yrs"), &[0xFF, 0xFE, 0xFD]).unwrap();
```

---

## Questions for Code Review

1. **Gap 1**: Should cross-process debounce read file mtime or poll-based timestamp? (Current: reads mtime from `.agent-doc/typing/<hash>` file)
2. **Gap 3**: Worth migrating to `blake3` hash for collision resistance? (Current: `DefaultHasher`)
3. **Gap 4**: Is CRDT state corruption recoverable? (Snapshot? manual intervention?)
4. **Gap 5**: Should 30-second timeout be configurable? (Trade-off: stale status vs. flexibility)
5. **Gap 6**: Frontmatter field `agent_doc_debounce_ms` or CLI flag `--debounce`? (Precedence?)

---

## Related Code Patterns

### Type-Safe Debounce Check (Gap 2 mitigation)
```rust
// Current: probe might call await_idle unconditionally
if debounce::await_idle(file, 1500, 3000) {
    // Proceed
}

// Proposed: check is_tracked first
if !debounce::is_tracked(file) {
    // Skip wait for untracked files
    return Ok(());
}
if !debounce::await_idle(file, 1500, 3000) {
    return Err("Debounce timeout");
}
```

### Configurable Debounce (Gap 6 mitigation)
```rust
// Current
let typing_active = debounce::is_typing_via_file(&file_str, 1500);

// Proposed
let fm = frontmatter::parse(&doc)?;
let debounce_ms = fm.agent_doc_debounce_ms.unwrap_or(1500);
let typing_active = debounce::is_typing_via_file(&file_str, debounce_ms);
```

### Merge Error Handling (Gap 4 mitigation)
```rust
// Current: may panic or silently corrupt
let result = crdt::merge(&state, &ours, &theirs);

// Proposed: explicit error
match crdt::merge(&state, &ours, &theirs) {
    Ok(merged) => { /* write merged */ }
    Err(e) => {
        eprintln!("CRDT merge failed: {}", e);
        return Err(e);
    }
}
```

---

## Acceptance Criteria (HEAD)

Test is "passing" when:
1. Code compiles without `#[ignore]`
2. Test runs without panic
3. All assertions pass
4. Behavior matches "Expected behavior (if gap is fixed)" from plan

Test is "complete" when:
1. All 15 tests are implemented (not `#[ignore]`)
2. All gaps have ≥1 test
3. Critical gaps (4, 5) have 2+ tests each
