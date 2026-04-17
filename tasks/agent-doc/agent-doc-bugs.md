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
Session covered v0.32.5→v0.32.6 development. Key fixes: #a004 (drop in-process status Mutex, file-based only), 0-component IPC patchback bug (boundary synthesis in all write paths), #jg0d/#b486/#40ct (supervisor submodules: state.rs, resize.rs, ipc.rs — 52 tests), #esws (single patch_id per logical write + plugin-side dedup TTL set), #1kap (poll ack-content sidecar instead of 200ms sleep), #s7wk/#1497 (sweep-commit guard + --pending-only flag), #p2xm (orphaned patch:exchange marker fix in parse_patches), --status flag (skill passes text, binary handles component targeting), #08yv (pre-lock file mod race), #wcf5 (snapshot-before-document ordering), #vefa (set_current_dir→base_dir refactor, 10 tests re-enabled), #3t02 (submodule IPC routing fixtures, 2 tests), #cjpp (un-ignored watcher_detects_change). Decisions: [/] vs [x] semantics codified (gate after impl, done on confirm); typed gates [/release]/[/deploy] added to pending; replace:* restricted to pending only; --status flag mirrors --pending-add pattern. State: 1055 tests passing, pending has 7 gated items.
❯ do #vnp0
### Re: #vnp0 supervisor wired into start.rs — opus-4-6

Implemented. The restart loop in `start.rs` now uses the full supervisor stack:

**What changed:**

1. **`supervisor/pty.rs`** — Added `ResizeHandle` (raw fd + TIOCSWINSZ ioctl for thread-safe resize), `take_writer()`, `clone_reader()`, `process_id()`, `resize_handle()`. Reverted master to `Box` (Arc not needed since ResizeHandle uses duped fd).

2. **`supervisor/mod.rs`** — Removed `#![allow(dead_code)]`. Added targeted `#[allow(dead_code)]` on individual API items that are test-only or future surface (sealed, send_command, is_active, etc.).

3. **`start.rs`** — Full rewrite of the restart loop:
   - **Pre-loop**: resolves CWD (`cwd::resolve`), env (`EnvSpec::from_frontmatter.resolve()`), base args, terminal size, project root. Starts `SupervisorIpc` listener. Creates `CrashPolicy`.
   - **Loop body**: builds `PtySpawnConfig`, spawns `PtySession`, extracts writer/reader for shared I/O (IPC inject + stdin forwarding), starts `ResizeWatcher` via `resize_handle()`, spawns auto-trigger thread, blocks on `wait()`.
   - **Exit handling**: checks IPC flags (stop/restart), then `CrashPolicy::on_exit()` → `PromptUser` / `RestartAfter{delay, with_continue}` / `Halt`.
   - **Shared state**: `SupervisorShared` struct with atomic flags, inject writer, child PID. IPC handler sends SIGTERM via `libc::kill()` to unblock `wait()`.

**Verification:**
- `cargo clippy` — clean (no warnings)
- `cargo test` — 1055 passed, 6 ignored
- `cargo build --release` — builds clean
- Manual testing gate: needs live `agent-doc start` session to confirm pty I/O, resize, restart, and IPC work end-to-end
<!-- agent:boundary:9cf0a6da -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#p2xm] Orphaned patch:exchange marker leak: parse_patches() does not consume unclosed opening markers — they leak into unmatched text and get appended to exchange verbatim. Fix: advance last_end past unclosed openers in parse_patches()
- [/] [#wcf5] Advisory flock doesn't protect external watchers: IDE file-change listeners and git hooks race concurrent diff computations against stale snapshots
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
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
- [ ] [#77mf] Event-driven gate resolution: typed gate markers like [/release] and [/deploy] that auto-resolve on matching events (cargo publish, git push). Extends the binary [/] invariant with machine-actionable gate types — reaper checks event log before skipping.
- [/] [#6hgf] Add --status flag to agent-doc write: skill passes status text as CLI arg, binary handles component targeting with replace semantics
<!-- /agent:pending -->
## Assistant
