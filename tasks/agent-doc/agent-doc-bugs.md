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
### Re: patchback root cause + uncommitted content + ignored tests — opus-4-6

**Root cause fixed.** Two code paths in `write.rs` manually built IPC patches from `patches` without boundary synthesis — when all response content is "unmatched" (no explicit `<!-- patch:exchange -->` block), `patches` is empty and these paths produced patch files with 0 components.

**Bug locations and fix:**

1. **`run_stream()` timeout fallback** (line ~1064) — when `try_ipc()` times out, `run_stream()` writes a NEW patch file for later plugin pickup. This path manually iterated `patches` instead of calling `build_ipc_patches_json()`. Now uses the shared helper with synthesis + dedup guard.

2. **`run_ipc()` explicit IPC mode** (line ~1339) — same manual iteration pattern. Now uses `build_ipc_patches_json()` + dedup guard. Also fixed a logging bug that reported `patches.len()` instead of `ipc_patches.len()` (masked the 0-component issue in logs).

**Verification:** 1036 tests pass, clippy clean, release binary built.

**Why so much uncommitted content:** The previous turn's response was lost to this bug. The snapshot was updated as if the write succeeded (plugin consumed the 0-component patch file as a no-op), so this turn's diff includes both the compacted exchange summary and the manually-pasted previous response — all appearing as "new" since the snapshot thought the document was already up to date.

**19 ignored tests — breakdown:**

| Count | Reason | Tests |
|-------|--------|-------|
| 11 | `set_current_dir` not thread-safe | autoclaim (3), sessions (5), watch (2) |
| 1 | Integration test requiring real filesystem notify | `watcher_detects_change` |
| 2 | Submodule IPC routing (need git repo setup) | `try_ipc_falls_back...`, `try_ipc_routes_to...` |
| 5 | Blocked on missing pub API surface | `debounce_gaps_test_plan` tests (no cleanup fn, no crdt::merge pub, no watch daemon API, no status_file_path pub, no preflight::run pub) |

The `set_current_dir` tests (11) are the bulk. They use `#[ignore]` because `std::env::set_current_dir` is process-global and races with other tests in `cargo test`'s thread pool. The 5 `debounce_gaps_test_plan` tests are stubs blocked on API surface that hasn't been exposed yet.
<!-- agent:boundary:f97224c5 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
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
