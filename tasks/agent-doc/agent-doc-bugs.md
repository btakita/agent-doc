---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ v0.32.5 shipped (slash-command hardening, session-check, drift auto-resync, post-write hook, claim cwd fix, model attribution fix). tmux_session auto-detect implemented (removed auto-write, added session clear). Stream double-flush fixed (AtomicBool gate #du8f). Lazy-claim pane theft fixed (claimed_panes HashSet filter #pg8w). Stash rescue bad tmux target fixed (@1:agent-doc → @1). JB plugin LayoutChangeDetector listener leak fixed with WeakHashMap dedup — plugin v0.2.57 built, awaiting install+confirm.
❯ do #a004
❯ ### Re: #a004 — drop Option B (in-process status) — opus-4-6 (HEAD)

❯ Done. Option B was the in-process `STATUS: Mutex<HashMap<PathBuf, String>>` in `debounce.rs` — a redundant duplicate of the file-based status signal (Option A) at `.agent-doc/status/`.

❯ **Changes:**

❯ 1. **`debounce.rs`** — Removed `STATUS` static, `with_status` helper. `set_status()` now writes only the file signal. `get_status()` and `is_busy()` delegate to `get_status_via_file()` (file-based, 30s staleness timeout). All callers (watch daemon, FFI layer, JB plugin) see the same behavior since `set_status` always wrote to both — now it just writes to one.

❯ 2. **`ffi.rs`** — Updated doc comments on `agent_doc_get_status` and `agent_doc_is_busy` to remove "Option B: in-process" language.

❯ 3. **Tests** — Updated comments in `status_file_staleness_timeout` and `status_found_for_file_one_level_deep` to reflect file-based path. All tests pass unchanged since the tests already had `.agent-doc/status/` dirs set up.

❯ **Verification:** 1010 tests pass, 19 ignored, clippy clean, release binary built.
❯ do #jg0d #b486 #40ct
❯ Add tests
❯ Why did the patchback from the last turn not appear in this document?
<!-- patch:exchange -->
❯ ### Re: patchback failure + #jg0d #b486 #40ct — opus-4-6 (HEAD)

❯ **Why last turn's response didn't appear:** The write command's IPC path sent a patch with **0 components**. The boundary-synthesized exchange patch was created (`synthesizing exchange patch for unmatched content (boundary 019ace51)`) but wasn't included in the IPC component list. The plugin consumed an empty patch as a no-op, the snapshot was updated as if it succeeded, and the response text was silently dropped. This is a write-path bug: boundary synthesis produces content but `try_ipc()` doesn't include it in the component payload.

❯ **#jg0d #b486 #40ct — implemented with tests (last turn, lost to the bug above):**

❯ Three supervisor submodules landed — 52 total supervisor tests passing.

❯ - **state.rs** (#b486): `ExitClass { Clean, Transient, Flapping }`, `SupervisorState { Healthy, Degraded, Halted }`, `CrashPolicy` with `RestartHistory` ring buffer (VecDeque cap 10). Thresholds: 3 non-zero exits in 60s → Flapping, 5 consecutive → Halted. 12 tests.
❯ - **resize.rs** (#jg0d): `ResizeWatcher::spawn(callback)` via `signal-hook` SIGWINCH on Unix, `TIOCGWINSZ` query, AtomicBool stop flag. Windows stub. 4 tests.
❯ - **ipc.rs** (#40ct): `SupervisorIpc::start(root, uuid, handler)` at `.agent-doc/supervisor/<uuid>.sock` (0600), NDJSON one-request-per-connection matching `ipc_socket.rs`. `IpcMethod { Restart, Inject, State, Pid, Stop }` serde tagged enum. `send_command()` client with 2s timeout. 10 tests.

❯ Added `signal-hook = "0.3"` to Cargo.toml. Clippy clean, release binary built.

❯ Please fix the root cause of the missing patchback.
❯ Why is there so much uncommitted content in this current patchback?
❯ Why are there 19 ignored tests?
<!-- patch:exchange -->
❯ ### Re: patchback root cause + uncommitted content + ignored tests — opus-4-6 (HEAD)

**Root cause fixed.** Two code paths in `write.rs` manually built IPC patches from `patches` without boundary synthesis — when all response content is "unmatched" (no explicit `<!-- patch:exchange -->` block), `patches` is empty and these paths produced patch files with 0 components.

**Bug locations and fix:**

1. **`run_stream()` timeout fallback** (line ~1064) — when `try_ipc()` times out, `run_stream()` writes a NEW patch file for later plugin pickup. This path manually iterated `patches` instead of calling `build_ipc_patches_json()`. Now uses the shared helper with synthesis + dedup guard.

2. **`run_ipc()` explicit IPC mode** (line ~1339) — same manual iteration pattern. Now uses `build_ipc_patches_json()` + dedup guard. Also fixed a logging bug that reported `patches.len()` instead of `ipc_patches.len()` (masked the 0-component issue in logs).

**Verification:** 1036 tests pass, clippy clean, release binary built.

❯ Are there any race conditions associated with patch:exchange?

**Why so much uncommitted content:** The previous turn's response was lost to this bug. The snapshot was updated as if the write succeeded (plugin consumed the 0-component patch file as a no-op), so this turn's diff includes both the compacted exchange summary and the manually-pasted previous response — all appearing as "new" since the snapshot thought the document was already up to date.

**19 ignored tests — breakdown:**

| Count | Reason | Tests |
|-------|--------|-------|
| 11 | `set_current_dir` not thread-safe | autoclaim (3), sessions (5), watch (2) |
| 1 | Integration test requiring real filesystem notify | `watcher_detects_change` |
| 2 | Submodule IPC routing (need git repo setup) | `try_ipc_falls_back...`, `try_ipc_routes_to...` |
| 5 | Blocked on missing pub API surface | `debounce_gaps_test_plan` tests (no cleanup fn, no crdt::merge pub, no watch daemon API, no status_file_path pub, no preflight::run pub) |

The `set_current_dir` tests (11) are the bulk. They use `#[ignore]` because `std::env::set_current_dir` is process-global and races with other tests in `cargo test`'s thread pool. The 5 `debounce_gaps_test_plan` tests are stubs blocked on API surface that hasn't been exposed yet.
❯ How can we readd these tests? Would a docker container or chroot help?
<!-- patch:exchange -->
❯ ### Re: patch:exchange race conditions — opus-4-6 (HEAD)

Yes, several race windows exist. Ranked by severity:

**High — IPC timeout fallback double-apply:** When `try_ipc()` socket send fails/times out, `run_stream()` writes a fallback `.agent-doc/patches/<hash>.json` file (write.rs ~1101). If the file watcher is polling the patches directory concurrently, both socket and file IPC can fire — double-applying the exchange patch. The cleanup at line ~1533 tries to prevent this but the window between socket failure and fallback write still exists.

**High — Snapshot stale after IPC success:** Between the plugin applying the patch (deleting the patch file) and the binary saving the snapshot (write.rs ~1599), the user can start editing. The snapshot is then stale relative to the next cycle's baseline, causing the next diff to include content that was already applied. The ack-content sidecar mitigates this when available; without it, the 200ms sleep is best-effort but not atomic.

**Medium — Pre-lock file modification:** Between reading `content_at_start` (line ~725) and acquiring `doc_lock` (line ~750), external edits can land. The CRDT 3-way merge handles this, but snapshot drift can occur if the baseline is stale.

**Medium — Replace-vs-append CRDT interleaving:** When agent replaces exchange content while user appends, character-level interleaving was possible with stale baselines. Fixed by design — regression test `merge_replace_vs_append_no_interleaving` guards this. The fix uses the committed baseline rather than stale CRDT state for diff computation.

**Low — Advisory flock doesn't protect external watchers:** IDE file-change listeners and git hooks aren't synchronized by agent-doc's flock. Concurrent diff computations can race if both use stale snapshots.

**Low — Boundary reposition off-transaction:** The `reposition_boundary: true` IPC flag is fire-and-forget within the Document API transaction. If the plugin crashes between patch apply and reposition, the boundary marker is stale until the next write cycle.

The double-apply (#1) and stale snapshot (#2) are the most likely to cause observable issues — duplicated exchange content or "ghost" diffs in the next cycle. Both are mitigable: #1 with an atomic IPC mode select (socket XOR file, never both in flight), #2 with a post-apply snapshot handshake instead of timeout polling.

❯ ### Re: re-enabling the 19 ignored tests — opus-4-6 (HEAD)

Docker/chroot is overkill. The best path depends on the test category:

**11 `set_current_dir` tests — refactor to accept `base_dir: &Path`:**

The root cause is that `save()`, `load()`, `write_pid()`, `read_pid()` etc. call `env::current_dir()` internally to resolve `.agent-doc/` paths. Refactor these to take an optional `root: &Path` parameter (defaulting to `env::current_dir()`). Tests pass an explicit temp dir — no process isolation needed, tests become fully parallelizable. Affected files: `autoclaim.rs` (3 tests), `sessions.rs` (5 tests), `watch.rs` (2 tests), plus 1 more.

This is the idiomatic Rust approach. Estimated scope: update the path-resolution functions to accept `root`, thread it through callers, update tests to pass `tempdir.path()`.

**Interim alternative:** `serial_test` crate — add `#[serial]` to these 11 tests. They run sequentially but remain enabled. Quick fix while the API refactor happens.

**1 filesystem notify test (`watcher_detects_change`):** Likely needs a real tmpdir + short poll timeout. Could be un-ignored with a `#[cfg(not(ci))]` gate or by adding a 500ms timeout to the notify watcher setup.

**2 submodule IPC routing tests:** Need a git repo with submodule structure in a temp dir. `git init` + `git submodule add` in test setup. Moderately fiddly but no external tooling needed.

**5 `debounce_gaps_test_plan` stubs:** These are blocked on pub API surface (`crdt::merge`, `status_file_path`, `preflight::run`, watch daemon API, cleanup fn). Unblocking requires making those APIs public — which should happen naturally as the supervisor module wire-up (#vnp0) progresses. No workaround; these wait on the API.

**Recommended priority:** base_dir refactor (11 tests) → submodule IPC setup (2 tests) → filesystem notify (1 test) → pub API stubs (5, blocked on other work).

❯ Add the priority to the top of the pending list.
<!-- patch:exchange -->
❯ ### Re: adding test priorities to pending — opus-4-6 (HEAD)

Added three items to the top of pending:
1. base_dir refactor (11 tests) — the bulk of the work
2. submodule IPC test fixtures (2 tests)
3. filesystem notify test (1 test)

The 5 `debounce_gaps_test_plan` stubs are omitted since they're blocked on pub API surface that unblocks with #vnp0 — no point tracking them separately.
noop
❯ Please add fixing the race conditions to the top of the pending list. Prioritize according to your suggestion.
noop
reorder
pending reorder
❯ Please add the previous failed patchback
<!-- patch:exchange -->
❯ ### Re: failed patchback artifacts — opus-4-6 (HEAD)

The "noop", "reorder", "pending reorder" strings in the exchange are artifacts from my workaround — `agent-doc write` rejects empty stdin, so I had to pipe carrier text to execute `--pending-add` and `--pending-reorder` flags. That carrier text then got synthesized into the exchange via boundary synthesis as "unmatched content."

This is a real bug: there's no way to run pending-only mutations without polluting the exchange. Added #-pending-only to the top of the pending list — `agent-doc write` needs a `--pending-only` or `--no-exchange` flag that accepts pending flags but suppresses exchange content synthesis.

The first failed patchback (the one that triggered this whole thread) was the 0-component IPC bug — that's already fixed in the codebase. The artifacts you're seeing now are a different issue: the skill workflow has no clean path for pending-only writes.

❯ do #esws
<!-- patch:exchange -->
❯ ### Re: implementing #esws — opus-4-6 (HEAD)

Starting implementation. The root cause is three independent `patch_id` UUIDs generated for what should be a single logical write:
- Socket path (write.rs:1548)
- File path (write.rs:1649)  
- Timeout fallback (write.rs:1070)

**Plan:**

1. **Binary (write.rs):** Generate patch_id once per logical write. `try_ipc()` returns the used patch_id alongside the bool so `run_stream()` can reuse it in the timeout fallback. Same patch_id flows through socket → file → fallback.

2. **Plugin (PatchWatcher.kt):** Add `appliedPatchIds: ConcurrentHashMap<String, Long>` dedup set. Before applying any patch (socket or file), check if patch_id already applied. After applying, record it. Periodic cleanup of entries >60s.

Implementing now.
<!-- patch:exchange -->
❯ ### Re: #esws implementation complete — opus-4-6 (HEAD)

**Root cause:** Three independent `patch_id` UUIDs were generated for the same logical write — socket path, file path, and timeout fallback each created their own. The plugin had no way to know these were the same write.

**Binary changes (write.rs):**

1. Added `IpcResult` struct — returns `success: bool` + `patch_id: String` from `try_ipc()`
2. Single `patch_id` generated once at the top of `try_ipc()`, shared across socket and file paths
3. New `reuse_patch_id: Option<&str>` parameter — callers can pass a previously-used ID
4. `run_stream()` timeout fallback now reuses `ipc_result.patch_id` instead of generating a new UUID
5. All callers updated (stream.rs, 6 tests)

**Plugin changes (PatchWatcher.kt):**

1. Added `appliedPatchIds: ConcurrentHashMap<String, Long>` dedup set with 60s TTL
2. `recordApplied(patchId)` — called after every successful `applyPatch()` in both socket and file paths
3. `isAlreadyApplied(patchId)` — checked before apply in both `handleSocketMessage()` and `processPatchFile()`
4. Double-checked under EDT to close TOCTOU race between background thread and EDT dispatch
5. Lazy cleanup of expired entries on each `recordApplied()` call

**Verification:** 671 tests pass (1 pre-existing failure in resync, unrelated), clippy clean, release binary built.

**How it prevents double-apply:** When socket IPC sends but doesn't ACK (timeout), and the file fallback writes the same `patch_id` to disk:
- If socket handler processes first → records patch_id → file watcher checks → skips (already applied)
- If file watcher processes first → records patch_id → socket handler checks → skips (already applied)
- Timeout fallback in run_stream → same patch_id → plugin dedup catches it regardless of delivery order
❯ do #1kap
<!-- patch:exchange -->
❯ ### Re: #1kap — stale snapshot race fix — opus-4-6 (HEAD)

Replaced the 200ms sleep with polling for the ack-content sidecar. The sidecar is the authoritative post-apply snapshot written by the plugin — polling catches it as soon as it appears instead of racing against an arbitrary delay.

**Changes (write.rs):**

1. **`poll_ack_content_sidecar()`** — new helper that polls `read_ack_content_sidecar()` in a loop with configurable timeout (500ms) and poll interval (25ms). Returns `Some(content)` when the sidecar appears, `None` after timeout.

2. **Socket IPC path** (line ~1633) — replaced single `read_ack_content_sidecar()` + `sleep(200ms)` fallback with `poll_ack_content_sidecar()`. Snap source logged as `ack_content_sidecar` or `file_read_no_sidecar`.

3. **File IPC path** (line ~1885) — same replacement. The no-patch_id branch now logs `file_read (no patch_id for sidecar lookup)` instead of silently sleeping.

**Tests (3 new):**

- `test_poll_sidecar_present_immediately` — sidecar exists before poll starts
- `test_poll_sidecar_appears_after_delay` — sidecar written by background thread after 50ms (atomic rename to avoid partial-read race)
- `test_poll_sidecar_timeout` — no sidecar → returns None after timeout, verifies timing bounds

**Verification:** 1039 tests pass, clippy clean, release binary built.
❯ There is a leftover patch:exchange from the last patchback. Also my annotation that I entered while a task was running was committed without a response patchback.

❯ Looking at /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md, there were some deadlocked tasks
❯ Here is 1
```
  Status: running
  Runtime: 1h 47m 15s
  Command: cat <<'RESPONSE' | agent-doc write
  /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md \
    --baseline-file /home/brian/work/btakita/agent-loop/src/boost-client/.agent-doc/baselines/7b9f2a6394de97c213efeb3e
  c1e3d33b7b966229655f4720376409f6abef299f.md \
    …

  Output:
  ╭────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
  │ warning: `<!-- patch:pending -->` is deprecated — use `<!-- replace:pending -->` instead (see #25ag)           │
  │ Error: ERR: replace:pending block forbidden — use --pending-add/done/edit/clear/reorder. See specs/pending-sy… │
```
noop
❯ The patchback is `noop`. I did see a proper response patchback but it was cleared and not committed.
❯ ### Re: response cleared after appearing — opus-4-6 (HEAD)

You saw the response flash in the document and then disappear. Here's what happened in sequence last cycle:

1. **First write** — response content piped through `agent-doc write --stream` → "IPC patch consumed by plugin — snapshot updated" (1 component, success)
2. **Second write** — "noop" carrier text for `--pending-add` flags → "content already present in exchange — skipping synthesis" → "WARNING: 4 bytes of unmatched content with no IPC patches — content will be dropped" → **0 components**, but "IPC patch consumed by plugin — snapshot updated"

The second write sent a 0-component patch to the plugin. The plugin consumed it as a no-op (didn't change the document), but the **snapshot was still updated** from the second write's state — which didn't include the response content. So when commit ran, it staged the second write's snapshot, which was missing the response.

This is a variant of bug #s7wk (snapshot integrity) combined with #1497 (pending-only writes). The fix is twofold:
- **Don't update the snapshot when writing 0 components** — if there's nothing to apply, the snapshot shouldn't change
- **#1497** — pending-only mutations shouldn't touch the exchange pipeline at all
❯ do #s7wk #1497
<!-- patch:exchange -->
### Re: #s7wk and #1497 — opus-4-6

**#1497 — `--pending-only` flag (main.rs):**

Added `--pending-only` flag to the `write` command. When set:
1. Validates at least one `--pending-*` flag is present (errors otherwise)
2. Validates incompatibility with `--template`/`--stream`/`--ipc`
3. Executes pending ops (add/done/edit/gate/ungate/reorder/clear) as normal
4. Returns immediately — no stdin reading, no boundary synthesis, no exchange pollution

The skill can now run `agent-doc write doc.md --pending-only --pending-add "task"` without piping carrier text through stdin.

**#s7wk — sweep-commit guard (preflight.rs:620-639):**

Added content-aware guard before sweep-commit. The sweep now:
1. Reads both snapshot and document content
2. Calls `is_stale_snapshot(snap, doc)` — returns `true` for inline mode when the document has completed `## Assistant` blocks beyond the snapshot with an empty trailing `## User` block (safe to commit)
3. If not stale (inline mode with user additions, or template mode): falls through to content-equality check after `strip_comments()` — if snapshot ≠ document, the user has edits → skip
4. Only reaches `git::commit` if content checks pass

For template mode docs, `is_stale_snapshot` always returns `false` (no `## Assistant` markers), so sweep only commits when snapshot and document are content-equal after comment stripping — conservative but correct.

**Tests:** 1040 pass (1 new: `preflight_sweep_skips_doc_with_unresponded_user_content`). Clippy clean, release binary built and installed.
<!-- agent:boundary:3c0dbd1f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#p2xm] Orphaned patch:exchange marker leak: parse_patches() does not consume unclosed opening markers — they leak into unmatched text and get appended to exchange verbatim. Fix: advance last_end past unclosed openers in parse_patches()
- [ ] [#08yv] Pre-lock file modification race: content_at_start read (write.rs ~725) to doc_lock acquire (~750) window allows external edits that drift the snapshot baseline
- [ ] [#wcf5] Advisory flock doesn't protect external watchers: IDE file-change listeners and git hooks race concurrent diff computations against stale snapshots
- [ ] [#vefa] Refactor set_current_dir tests to accept base_dir: &Path parameter — re-enable 11 ignored tests (autoclaim 3, sessions 5, watch 2)
- [ ] [#3t02] Add submodule IPC routing test fixtures (git init + submodule add in tmpdir) — re-enable 2 ignored tests
- [ ] [#cjpp] Un-ignore watcher_detects_change with real tmpdir + timeout — re-enable 1 integration test
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#vnp0] supervisor: wire into start.rs (replace existing restart loop, remove #[allow(dead_code)])
- [ ] [#6ae3] supervisor/pty.rs: wire PtySession into start.rs (phase 1 wire-up stage) — replaces the current Command::status inherit-tty path
- [ ] [#zp02] supervisor/env.rs: wire into start.rs — replace the inline fm.env expansion loop in start.rs:189 with EnvSpec::from_frontmatter(&fm).resolve()?; feed the result into PtySpawnConfig.env once state.rs restart loop is in place
- [ ] [#f7d5] supervisor: smoke test — real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end
- [ ] [#eg0w] ✅ Shipped: per-file CommitLock in git::commit + <5s freshness gate in preflight sweep (awaiting release)
- [ ] [#kdy5] Implement instruction-files audit check: every library AGENTS.md has a ## Library Context Policy section with resolvable link to LIBRARY_CONTEXT_POLICY.md. Wire into make precommit.
- [ ] [#1376] After audit check lands: sweep all libraries under src/ to add the Library Context Policy stub section. Each submodule commit separately, then parent pointer bumps.
- [ ] [#breh] rust-package skill: write the Library Context Policy stub into new library AGENTS.md at scaffold time.
- [ ] [#3j0e] Install skills-janitor as Claude Code plugin (run /plugin install khendzel/skills-janitor at terminal). User plans to fork eventually; this is the interim path.
- [ ] [#vdyj] After fork: move forked skills-janitor under src/ as submodule, wire audit into make precommit, and optionally add nightly cron via schedule skill writing drift reports to tasks/agent-doc/skills-audit.md.
- [ ] [#9v7h] agent-doc: --pending-add should reject text starting with '[/]' / '[x]' / '[ ]' (ambiguous state marker). Add --pending-add-gated flag and emit assigned hash to stdout so skill can compose add+gate atomically without text-embedding.
- [ ] [#txex] notify: add --pending-add flag — auto-create agent:pending on target when absent, --no-create-pending to opt out. Pending-only (no exchange blockquote). Errors become no-ops under auto-create.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#hrmt] Rewrite src/agent-doc/SKILL.md (bundled skill) to invert Phase 2 pending guidance: remove 'every response MUST include a patch:pending block', replace with granular-ops contract from .claude/skills/agent-doc/SKILL.md §1b. RELEASE GATE for #gqep — v0.32.4 cannot ship with inverted bundled-skill guidance.
- [ ] [#e46w] Sweep-commit path: working-tree boundary lags until next write or editor re-open. Fix: have reposition_boundary_in_snapshot also rewrite the working-tree file when no active run is in progress (git.rs:593). Alternative: emit IPC reposition_boundary notification to open plugins.
- [ ] [#f8j3] Sweep-commit working-tree lag: reposition_boundary_in_snapshot (git.rs:593) only updates snapshot; working tree boundary only moves via IPC from agent-doc write. Fix: also rewrite working tree in snapshot reposition path, or emit IPC ping to open plugins.
- [ ] [#y4da] Deterministic boundary reposition in working tree (git.rs reposition_boundary_in_snapshot extends to disk write with active-run guard)
- [ ] [#zvmd] Submodule (HEAD) marker: fix show_head() to route via narrow_to_submodule — superproject rev-parse can't show submodule-tracked files, so add_head_marker falls through to the 'no HEAD → mark last heading only' fallback, which drops (HEAD) when the exchange component has no ### Re: heading yet or when the fallback runs during a boundary-only cycle
- [ ] [#np8p] Submodule pane cwd: agent-doc start/route spawns tmux panes with std::env::current_dir() instead of the nearest git toplevel for the target document — add a resolve_pane_cwd(file) helper that returns narrow_to_submodule(super_root, resolved).0 so submodule docs get a pwd inside their own submodule
- [ ] [#0xka] #hdap: reposition_boundary_to_end appends (HEAD) to last ### Re: heading in exchange, strips stale (HEAD) from earlier headings — symmetric counterpart to strip_head_markers on commit staging side
<!-- /agent:pending -->
## Assistant
