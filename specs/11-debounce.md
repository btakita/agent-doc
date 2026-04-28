> Extracted from SPEC.md — see [index](../SPEC.md)

# Debounce System Gaps and Limitations

The debounce subsystem manages multi-layer typing detection across editor plugins (JetBrains, VS Code, Neovim, Zed) and CLI invocations. While the architecture is sound, several known gaps exist that should inform operators and guide future improvements.

## Mtime Granularity in Route Path

**Gap:** The route path relies on filesystem mtime for debouncing rapid edits. Filesystem mtime resolution varies:
- **Coarse-grained systems** (e.g., HFS+ on macOS): 1-second resolution
- **Fine-grained systems** (Linux ext4): ~100ms resolution

When multiple edits occur within the mtime granularity window, route may miss the intermediate change and only detect the final state.

**Impact:** Rare but real on macOS. User typing very fast may trigger a route call with an editor state that reflects only partial changes.

**Mitigation:** Route path uses a timeout cap (10x debounce duration) to prevent indefinite hangs. Cross-process typing indicator files provide additional fallback for preflight detection.

**Test coverage:** `test_mtime_granularity_100ms_rapid_edits`, `test_mtime_granularity_1s_coarse_system`. See `tests/debounce_gaps_test_plan.rs`.

## Untracked File Edge Case

**Gap:** Files passed to `document_changed()` are tracked in the in-process `LAST_CHANGE` map. Files never passed to `document_changed()` return `idle=true` immediately (design choice to prevent `await_idle` blocking forever on unknown files).

This means the CLI cannot distinguish:
- "File was tracked and has been idle for 2s"
- "File was never tracked by any plugin"

**Impact:** Low. The `is_tracked()` function exists to distinguish these cases, but callers must explicitly check. Non-blocking probes may conservatively assume untracked files are NOT idle.

**Mitigation:** Use `is_tracked(file)` before making assumptions about untracked files. Preflight applies both mtime debounce AND typing indicator debounce (redundant but safe).

**Test coverage:** `test_untracked_file_is_tracked_returns_false`, `test_tracked_file_is_tracked_returns_true`, `test_untracked_file_is_idle_returns_true`, `test_probe_pattern_untracked_skips_await`. See `tests/debounce_gaps_test_plan.rs`.

## Hash Collision in Typing Indicator Paths

**Gap:** Typing indicator files are stored in `.agent-doc/typing/<hash>` where hash is computed via `std::collections::hash_map::DefaultHasher`. DefaultHasher is non-cryptographic and designed for hash maps, not for unique identifiers.

Collision probability: ~1 in 4.3 billion for random inputs. Collision is possible but extremely unlikely.

**Impact:** Very low probability. If collision occurs, the most recent change wins (last write to the shared file). The collision is self-correcting in the next debounce cycle because file timestamps diverge.

**Mitigation:** No action needed. Collisions are rare and self-healing. If deterministic behavior is required, consider switching to SHA256 hashing in future.

**Test coverage:** `test_hash_collision_no_collisions_for_common_paths` (10k paths). `test_hash_collision_cleanup_removes_stale_indicators` — GC implemented in `gc.rs` via `clean_stale_ephemeral_files()`, removes typing indicators older than 7 days. See `tests/debounce_gaps_test_plan.rs`.

## Reactive Mode Assumes CRDT Merge Convergence

**Gap:** Watch daemon's reactive path (used for `agent_doc_write: crdt` documents) applies zero debounce, expecting instant re-submit on file change. This assumes the CRDT merge algorithm always converges to a consistent state.

If a CRDT merge produces unexpected results (e.g., text duplication, loss of edits), reactive mode could cause the watch daemon to re-submit with corrupted state repeatedly.

**Impact:** Medium (data loss risk if CRDT merge is broken). Mitigated by extensive CRDT testing in `src/crdt.rs` and `src/merge.rs`.

**Mitigation:** CRDT implementation is battle-tested with golden-answer test cases (20-30 cases per session diff). See `agent-doc eval-runner` for continuous validation.

**Test coverage:** `test_reactive_mode_crdt_merge_failure_handling`, `test_reactive_mode_infinite_loop_prevention` — both blocked pending `crdt::merge` and watch daemon API exposure. See `tests/debounce_gaps_test_plan.rs`.

## Status File Staleness Timeout (30s Hardcoded)

**Gap:** Response status files (`.agent-doc/status/<hash>`) expire after 30 seconds with the assumption: "if no update after 30s, the operation probably crashed."

This timeout is hardcoded in `get_status_via_file()` and not configurable.

**Impact:** Medium. Long-running operations (slow CI, expensive LLM calls, network latency) may exceed 30s and be treated as crashed, allowing duplicate submissions.

**Mitigation:** For long-running scenarios, increase the timeout (currently not exposed via config). The binary also sends `set_status()` updates, so well-instrumented operations will keep the timeout alive.

**Test coverage:** `test_status_file_staleness_30s_timeout` (29s/30s/31s boundary). `test_status_file_write_includes_current_timestamp` blocked pending `status_file_path` pub exposure. See `tests/debounce_gaps_test_plan.rs`.

## Hardcoded Timing Constants in Preflight

**Gap:** Preflight applies a hardcoded **1500ms** debounce window via `is_typing_via_file(&file_str, 1500)` in `preflight.rs:366`.

Meanwhile, the poll-based debounce used elsewhere defaults to **500ms**. This creates asymmetry:
- Typing indicator requires 1500ms to expire
- Poll-based debounce (watch, route) uses 500ms

Not configurable per-document; one-size-fits-all fails for slow CI systems or fast typists.

**Impact:** Low to Medium. CI systems that take >1500ms to write files will appear to be typing longer than expected, potentially delaying preflight. Conversely, fast typists may experience premature debounce expiry on poll-based paths.

**Mitigation:** Make timing constants configurable via frontmatter (`agent_doc_debounce_ms`, `agent_doc_typing_indicator_ms`). For now, operators can adjust via direct code modification if needed.

**Test coverage:** `test_timing_constants_are_documented` (code review pass). `test_preflight_timing_1500ms_is_configurable`, `test_preflight_3s_timeout_is_sufficient_for_debounce` blocked pending `preflight::run()` exposure and `agent_doc_debounce_ms` frontmatter wiring. See `tests/debounce_gaps_test_plan.rs`.

## Directory-Walk Double-Pop Bug (Fixed in v0.28)

**Gap (now fixed):** `typing_indicator_path()` and `status_file_path()` contained a double-pop bug: each loop iteration called `dir.pop()` twice — once unconditionally at the top of the loop, and once at the bottom to advance to the next level. This caused every other directory level to be skipped when walking up to find `.agent-doc/`.

Files at **odd depths** from the project root (1, 3, 5 levels) failed to find `.agent-doc/` and fell back to writing indicators in the file's immediate parent directory instead. For example, a file at `tasks/file.md` (1 level deep) would fail while `tasks/software/file.md` (2 levels deep) succeeded.

**Root cause:** The loop's end-of-iteration `pop()` double-counted the level already consumed by the next iteration's leading `pop()`.

**Fix:** Pop the file component once before entering the loop, then pop exactly once per iteration. This ensures every directory level is checked.

**Impact:** Cross-process typing detection and status files were silently written to wrong paths for single-level-deep documents. Indicators were effectively lost from the plugin's perspective, causing premature debounce expiry.

**Test coverage:** `typing_indicator_found_for_file_one_level_deep`, `typing_indicator_found_for_file_two_levels_deep`, `status_found_for_file_one_level_deep`. See `src/debounce.rs`.

## Recommended Improvements

1. **Expose timing constants to frontmatter** — Allow per-document control via:
   ```yaml
   agent_doc_debounce_ms: 500
   agent_doc_typing_indicator_ms: 1500
   agent_doc_status_timeout_ms: 30000
   ```

2. **Switch to cryptographic hashing** (SHA256) for typing indicator and status file paths to eliminate collision risk entirely.

3. **Make 30s status timeout configurable** — either via config.toml or frontmatter.

4. **Mtime fallback in route path** — If mtime-detected change is stale (>1s), also check cross-process typing indicator as fallback.

5. **CRDT merge monitoring** — Log merge conflicts and convergence issues to `.agent-doc/logs/merge.log` for operator visibility.

6. ~~**Stale typing indicator cleanup**~~ — **Fixed:** `agent-doc gc` now removes typing indicators older than 7 days, status files older than 24 hours, and repair-blocked diagnostics older than 7 days via `clean_stale_ephemeral_files()` in `gc.rs`.
