> Extracted from SPEC.md — see [index](../SPEC.md)

# Debounce System Gaps and Limitations

The debounce subsystem manages multi-layer typing detection across editor plugins (JetBrains, VS Code, Neovim, Zed) and CLI invocations. While the architecture is sound, several known gaps exist that should inform operators and guide future improvements.

## Route Quiescence

**Status:** Fixed for editor-triggered route. The route path treats the
cross-process typing indicator as the authoritative quiescence signal and uses
filesystem mtime only as a fallback when no editor indicator exists. When
debounce is enabled, route waits before it inserts a session id, scrubs duplicate
prompt residue, or submits a reopen, classifying the typing indicator as one of:

- **Idle** (indicator file present but older than the debounce window): an editor
  owns the typing lifecycle and already debounced in-process. Route dispatches
  **immediately** — it does *not* re-impose the mtime settle.
- **Active** (indicator updated within the debounce window): the user is still
  typing. Route keeps waiting regardless of mtime.
- **Absent** (no indicator file): no editor is tracking the document (CLI /
  direct-disk caller). Route falls back to the filesystem mtime settle as the
  only available quiescence proof.

If the indicator stays Active (or, for the Absent case, mtime never settles)
through the bounded wait, route fails closed and asks the caller to retry after
typing stops. The CLI `route` default debounce is 500ms.

**Latency fix (`#jb-run-agent-doc-double-debounce`):** JetBrains `Run Agent Doc`
awaits typing idle in-process, then calls `saveAllDocuments()` (which bumps the
file mtime) immediately before spawning `agent-doc route`. The earlier design
required *both* mtime idle and indicator idle, so the editor's own pre-route save
re-triggered a full ~500ms mtime settle inside route — a redundant double
debounce the operator perceived as "Run Agent Doc takes several seconds to
dispatch". Because the typing indicator is only written on real keystrokes (not
on save), an Idle indicator already proves the user stopped typing; the fresh
mtime is the editor's save, not user input. Route now trusts the Idle indicator
and skips the redundant mtime wait, while the Absent path keeps the mtime guard
for non-plugin callers.

Filesystem mtime resolution still varies:
- **Coarse-grained systems** (e.g., HFS+ on macOS): 1-second resolution
- **Fine-grained systems** (Linux ext4): ~100ms resolution

The typing indicator side of the proof is what closes the mtime granularity gap
for live editor typing.

**Impact:** Low. A missing or unavailable editor typing indicator can still only
prove disk quiescence, but active editor integrations that call
`document_changed()` now prevent route from mutating or dispatching against a
live typing buffer.

**Mitigation:** Route uses a timeout cap (10x debounce duration) to prevent
indefinite hangs and fails closed instead of proceeding when the combined proof
does not settle.

## Run Agent Doc dispatch latency

**Status:** Tightened. Beyond the double-debounce fix above
(`#jb-run-agent-doc-double-debounce`), three additional `#run-agent-doc-latency`
optimizations trim the editor-triggered dispatch floor:

1. **Plugin skips the binary debounce.** The JetBrains `Run Agent Doc` action
   awaits typing idle in-process via the FFI typing tracker before saving, so
   `buildRunRouteCommand` now passes `route --debounce 0`. Route's mtime/indicator
   settle is pure redundant latency on the editor path; the binary still treats
   the cross-process typing indicator as authoritative for non-editor callers
   (which keep the default 500ms debounce).
2. **Submit-acceptance check is capture-then-sleep with empty-input
stabilization.** `send_command_unchecked` captured the pane only *after* a
fixed 300ms sleep, so even an instantly consumed trigger paid 300ms. It now
captures first and sleeps after on a tightened
`DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL` (150ms), but an empty first
capture is not enough to prove acceptance: route waits for the empty capture to
remain stable before accepting so a delayed Codex composer draft can still be
seen and re-submitted with the harness submit key. If Codex later reaches
accepted-only dispatch proof and the same prompt is visibly drafted, route sends
one late submit-key retry and rechecks dispatch-start proof.
3. **Ready-prompt poll cadence tightened.** `wait_for_agent_ready_outcome` polled
   every 500ms and requires a 2-poll ready streak to debounce a transient prompt
   flicker, giving a ~500-1000ms ready floor. The poll interval is now
   `AGENT_READY_POLL_INTERVAL` (150ms), so the streak settles in ~150-300ms while
   keeping the 2-observation debounce.

**Test coverage:** `run route command requests plain trigger for editor dispatch`
(JB `TerminalUtilTest`, asserts `--debounce 0`);
`direct_pane_acceptance_waits_for_stable_empty_capture` and
`direct_pane_acceptance_accepts_after_visible_draft_disappears` cover the
empty-input acceptance state machine; the capture/poll cadence changes are
exercised by the live-tmux `send_command_checked_*` and
`wait_for_agent_ready*` integration tests (`make tmux-ci`).

**Test coverage:** `route_debounce_fails_closed_while_typing_indicator_is_active`,
`route_debounce_allows_dispatch_after_typing_indicator_expires`,
`route_dispatches_immediately_when_idle_typing_indicator_present_despite_fresh_mtime`,
`test_mtime_granularity_100ms_rapid_edits`, `test_mtime_granularity_1s_coarse_system`.
See `src/route/tests.rs` and `tests/debounce_gaps_test_plan.rs`.

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

## Reactive Mode CRDT Merge Safety

**Fixed:** Three issues addressed:

1. **Merge failure fallback in write paths** — stream write (`write.rs:2700`) and IPC timeout fallback (`write.rs:2938`) used bare `?` on `merge_contents_crdt()`, losing the entire response on any CRDT error. Both now use `match` with `splice_pending_component()` fallback (same pattern as the IPC timeout path at `write.rs:2555`).

2. **Agent-change detection window for reactive mode** — `is_agent_change` used `debounce * 3` to detect agent-triggered changes. With reactive (zero debounce) paths, this collapsed to `Duration::ZERO`, making every change after `last_run` appear agent-triggered. Added `MIN_AGENT_CHANGE_WINDOW_MS = 500` floor so the detection window is always at least 500ms.

3. **Convergence idempotency** — CRDT merge is verified idempotent: re-merging converged content with itself produces identical output, which is what the watch daemon's hash-based convergence detection relies on.

**Test coverage:** `test_reactive_mode_crdt_merge_failure_handling` (corrupted state returns Err, None base succeeds, valid state succeeds), `test_reactive_mode_infinite_loop_prevention` (merge idempotency, corrupted state error handling). See `tests/debounce_gaps_test_plan.rs`.

## Status File Staleness Timeout (30s Hardcoded)

**Gap:** Response status files (`.agent-doc/status/<hash>`) expire after 30 seconds with the assumption: "if no update after 30s, the operation probably crashed."

This timeout is hardcoded in `get_status_via_file()` and not configurable.

**Impact:** Medium. Long-running operations (slow CI, expensive LLM calls, network latency) may exceed 30s and be treated as crashed, allowing duplicate submissions.

**Mitigation:** For long-running scenarios, increase the timeout (currently not exposed via config). The binary also sends `set_status()` updates, so well-instrumented operations will keep the timeout alive.

**Test coverage:** `test_status_file_staleness_30s_timeout` (29s/30s/31s boundary) and `test_status_file_write_includes_current_timestamp`. See `tests/debounce_gaps_test_plan.rs`.

## Configurable Timing Constants in Preflight

**Status:** Fixed for preflight. The preflight mtime debounce and cross-process typing-indicator debounce now use the same configured `agent_doc_debounce` frontmatter value, defaulting to **2000ms**.

The poll-based debounce used elsewhere defaults to **500ms**. This creates expected context-specific asymmetry:
- Preflight typing detection uses the document's `agent_doc_debounce` value, defaulting to 2000ms
- Poll-based debounce (watch, route) uses 500ms
- Long debounce values expand preflight's maximum wait beyond the historical 3s cap

Per-document configuration avoids one-size-fits-all behavior for slow CI systems or fast local sessions.

**Impact:** Low. CI systems or editor integrations that need longer settling windows can configure the document. Fast local sessions can set a smaller value.

**Mitigation:** Set `agent_doc_debounce` in frontmatter.

**Test coverage:** `test_timing_constants_are_documented`, `test_preflight_timing_1500ms_is_configurable`, `test_preflight_3s_timeout_is_sufficient_for_debounce`. See `tests/debounce_gaps_test_plan.rs`.

## Directory-Walk Double-Pop Bug (Fixed in v0.28)

**Gap (now fixed):** `typing_indicator_path()` and `status_file_path()` contained a double-pop bug: each loop iteration called `dir.pop()` twice — once unconditionally at the top of the loop, and once at the bottom to advance to the next level. This caused every other directory level to be skipped when walking up to find `.agent-doc/`.

Files at **odd depths** from the project root (1, 3, 5 levels) failed to find `.agent-doc/` and fell back to writing indicators in the file's immediate parent directory instead. For example, a file at `tasks/file.md` (1 level deep) would fail while `tasks/software/file.md` (2 levels deep) succeeded.

**Root cause:** The loop's end-of-iteration `pop()` double-counted the level already consumed by the next iteration's leading `pop()`.

**Fix:** Pop the file component once before entering the loop, then pop exactly once per iteration. This ensures every directory level is checked.

**Impact:** Cross-process typing detection and status files were silently written to wrong paths for single-level-deep documents. Indicators were effectively lost from the plugin's perspective, causing premature debounce expiry.

**Test coverage:** `typing_indicator_found_for_file_one_level_deep`, `typing_indicator_found_for_file_two_levels_deep`, `status_found_for_file_one_level_deep`. See `src/debounce.rs`.

## Live-buffer classifier diagnostics (`#f5d2` / `#pcp6`)

`live_buffer_diverges_from_content(file, content)` decides whether the editor
holds a genuine unsaved edit *ahead* of disk (returns the snapshot) or whether an
apparent divergence is explained away (returns `None`). It has four
prove/disprove-relevant outcomes, each now recorded best-effort to
`.agent-doc/logs/ops.log` so a live editor test can grep exactly why a buffer was
or was not treated as a pending edit:

| ops.log line | Meaning |
|--------------|---------|
| `live_buffer_classify decision=diverges reason=unsaved_buffer_ahead_of_disk` | Genuine unsaved editor edit detected — divergence fails closed. |
| `live_buffer_classify decision=suppressed reason=editor_content_equals_disk` | Editor's full buffer text equals disk (`#pcp6` content match) — no unsaved edit. |
| `live_buffer_classify decision=suppressed reason=write_provenance_newer_than_buffer` | agent-doc's own recorded disk write postdates the editor digest (`#pcp2`). |
| `live_buffer_classify decision=suppressed reason=disk_mtime_stale_vs_buffer` | Disk mtime is confidently newer than the editor digest — digest lags a disk write. |

Logging only; the return value is unchanged. The trivial "snapshot already equals
expected content" early return is intentionally not logged to avoid per-check
noise. Pairs with the write-path outcome logs in `write.rs`
(`visible_write_live_buffer_matches_disk` / `visible_write_deferred_current_changed`);
the live-buffer-matches-disk marker must carry expected/disk/live lengths and
hashes plus `live_ts` so a live editor proof is self-contained.

## Recommended Improvements

1. **Expose remaining timing constants to frontmatter** — Allow per-document control via:
   ```yaml
   agent_doc_debounce: 500
   agent_doc_status_timeout_ms: 30000
   ```

2. **Switch to cryptographic hashing** (SHA256) for typing indicator and status file paths to eliminate collision risk entirely.

3. **Make 30s status timeout configurable** — either via config.toml or frontmatter.

4. ~~**Mtime fallback in route path**~~ — **Fixed:** route now checks the cross-process typing indicator alongside mtime and fails closed when the combined proof does not settle.

5. **CRDT merge monitoring** — Log merge conflicts and convergence issues to `.agent-doc/logs/merge.log` for operator visibility.

6. ~~**Stale typing indicator cleanup**~~ — **Fixed:** `agent-doc gc` now removes typing indicators older than 7 days, status files older than 24 hours, and repair-blocked diagnostics older than 7 days via `clean_stale_ephemeral_files()` in `gc.rs`.
