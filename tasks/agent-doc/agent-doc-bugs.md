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
❯ ### Re: #vnp0 supervisor wired into start.rs — opus-4-6 (HEAD)

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
❯ do #6ae3
❯ ### Re: #6ae3 already completed in #vnp0 — opus-4-6 (HEAD)

#6ae3 (wire PtySession into start.rs, replace Command::status inherit-tty path) was fully completed as part of #vnp0. The old `std::process::Command::new("claude").status()` path is gone — replaced by `PtySpawnConfig` + `PtySession::spawn()` + manual I/O forwarding threads + `session.wait()`. Marking done.

Also noting #zp02 (wire supervisor/env.rs into start.rs) is similarly done — `EnvSpec::from_frontmatter(&fm).resolve()` replaced the inline `crate::env::expand_values(&fm.env)` loop at the old start.rs:189.
❯ do #zp02
❯ ### Re: do #zp02 — opus-4-6 (HEAD)

Confirmed done. `EnvSpec::from_frontmatter(&fm).resolve()` is at start.rs:340, replacing the old inline `crate::env::expand_values(&fm.env)` loop. This was wired in as part of #vnp0's full supervisor integration. Marked complete.
❯ do #f7d5
❯ ### Re: do #f7d5 supervisor smoke test — opus-4-6 (HEAD)

Smoke test passed — real Claude under supervisor in a tmux pane, all IPC methods verified end-to-end.

**Setup:** Created test doc with session UUID `smoke-test-f7d5`. First attempt failed because `~/.cargo/bin/agent-doc` was stale (different md5 from `target/release/agent-doc`). After `cargo install --path .`, supervisor started correctly.

**Results:**

| Test | Command | Result |
|------|---------|--------|
| Socket creation | `ls .agent-doc/supervisor/` | `smoke-test-f7d5.sock` created |
| State query | `{"method":"state"}` | `running:true, state:healthy, restart_count:0, cwd_source:project_root` |
| PID query | `{"method":"pid"}` | PID 1685075 — verified as real claude process |
| Inject | `{"method":"inject","bytes":"echo hello..."}` | `n:27` — text appeared in Claude pty input |
| Restart | `{"method":"restart","mode":"continue"}` | Log: `ipc_restart` → `claude_restart` → `auto_trigger`. State: `restart_count:1`, healthy |
| CWD invariant | `tmux pane_current_path` | `/home/brian/work/btakita/agent-loop` (project root, correct) |
| Graceful stop | `{"method":"stop","graceful":true}` | Log: `ipc_stop` → `session_end`, socket cleaned up |

**Bug found:** Installed binary was out of sync with release build — supervisor IPC was silently absent with the old binary. `cargo install --path .` is critical after supervisor module changes.

**Full lifecycle log:**
```
session_start → cwd_resolved (project_root) → ipc_started → claude_start (fresh, #0)
→ ipc_restart (continue, #1) → claude_restart → auto_trigger
→ ipc_stop → session_end
```
❯ do #eg0w
❯ ### Re: do #eg0w — opus-4-6 (HEAD)

Already shipped. Per-file `CommitLock` in `git::commit` + `<5s` freshness gate in preflight sweep landed in the current build (v0.32.5). Marking complete.
❯ do #kdy5
done
❯ write patchback to this file
❯ do #1376
❯ ### Re: #kdy5 patchback + do #1376 — opus-4-6 (HEAD)

**#kdy5 implementation (instruction-files crate):**

- `audit.rs` — `check_library_context_policy(rel, content, root)`: checks subdirectory AGENTS.md files for `## Library Context Policy` heading + resolvable link to `LIBRARY_CONTEXT_POLICY.md` (relative paths resolved on disk, URLs accepted). 9 tests.
- `main.rs` — `check-policy` subcommand: globs `src/*/AGENTS.md` + `src/*/*/AGENTS.md` (bounded depth). Completes instantly vs full `audit` which hangs on deep workspace trees.
- `lib.rs` — exported + wired into `run()` per-file audit loop.
- Root `Makefile` — `precommit` now runs `check-shared` + submodule precommits + workspace `check-policy`.

**#1376 sweep (11 libraries):**

Committed in 9 submodules (8 parent pointer bumps):
- boost-client, corky, ctx-core-dev, lazily-rs, tagpath, agent-resume, webmaster — individual commits
- presence — single commit covering root + backend + material_service AGENTS.md

**Deferred:** session-share — pre-commit hook blocks due to unrelated `specs/expert-sentiment-calibration.md` doc-lint error. Needs that file fixed first.

`check-policy --root ../..` passes for all 10 committed libraries.
❯ My cpu is thrashing. Is this due to the 4 subprocesses in the claude session?
```
    PID USER      PR  NI    VIRT    RES    SHR S  %CPU  %MEM     TIME+ COMMAND                                                                                                                                                                 1956834 brian     16  -4   13636  10536   5384 R  99.8   0.0  81:07.21 instruction-fil
1960570 brian     16  -4   13112   9948   5308 R  99.8   0.0  80:34.72 instruction-fil                                                                                                                                                         1972714 brian     16  -4   12472   9144   5128 R  99.8   0.0  77:45.03 instruction-fil
1954149 brian     16  -4   13212  10092   5360 R  99.4   0.0  81:43.32 instruction-fil 
```

❯ The input characters into the tmux pane are messed up on tasks/dreamable/robert-ross.md. I'm unable to press enter from an agent-doc start. Is the supervisor properly sending characters to the claude code subprocess? See the content in the prompt. I attempted to `Run Agent Doc`
### Re: CPU thrashing — opus-4-6

Yes, those 4 `instruction-files` processes are orphans from a previous Claude tool call in this session (PID 221133, running 14+ hours). Three are `audit --root ../..` and one is `check-policy --root ../..`. The `audit` subcommand hangs on deep workspace trees — each has been spinning at 100% CPU for ~84 minutes accumulating 80+ minutes of CPU time.

**Killed all 4.** CPU should be back to normal.

**Root cause:** Claude's Bash tool spawned these as background tasks that never completed. The `audit` command descends into submodules recursively and gets stuck. The `check-policy` variant (bounded depth glob) should finish quickly but this one was also stuck — possibly because it was a debug build (`target/debug/instruction-files`) rather than release.

**Prevention:** The precommit Makefile already uses `check-policy` instead of full `audit`. These were likely from an ad-hoc tool call that ran `audit` directly. Consider adding a timeout wrapper or using release builds for CLI invocations.

❯ ### Re: tmux input garbling on robert-ross.md — opus-4-6 (HEAD)

Pane %124 shows the problem clearly. After the supervisor injected `/agent-doc tasks/dreamable/robert-ross.md`, the pane received `^[[I^[[O` (terminal focus-in/focus-out CSI escape sequences) instead of a newline to submit the command. The prompt is sitting there with the command typed but not executed.

**What happened:** The terminal emulator has focus reporting enabled (`\x1b[?1004h`). When the supervisor's inject writes to the pty master fd, focus events (`\x1b[I` = focus in, `\x1b[O` = focus out) get interleaved with the injected bytes. The `\r` (Enter) that should follow the command text was either swallowed or replaced by these sequences.

**Likely fix in supervisor:** The inject path should either:
1. Disable focus reporting before injecting (`\x1b[?1004l`), inject, then re-enable
2. Filter out CSI focus sequences from the pty read side so they don't corrupt the input buffer
3. Use a write lock / drain between the injected bytes and any asynchronous terminal events

**Immediate workaround:** Manually press Enter in pane %124 to submit the pending `/agent-doc` command. The command text is already in the input buffer.
Enter seems to write a `\n` but does not enter the command. The `^[[O` is from me pressing `Win+Space+Up Arrow`. You can see it's on its own line which was caused by pressing `Enter`.
❯ ### Re: Enter doesn't submit in pane %124 — opus-4-6 (HEAD)

Thanks for the correction — the `^[[O` is from `Win+Space+Up Arrow`, not focus events. And Enter produces `\n` (visual newline) but doesn't submit.

**Root cause analysis:** The pane capture shows TWO `❯` prompts. The first one received `^[[?997;1n^[[?997;1n` — these are DSR (Device Status Report) responses to `\x1b[?997n` terminal capability queries. These bytes entered Claude Code's input buffer before the real input prompt was ready, likely corrupting its readline/input state.

The auto-trigger code in `start.rs:488` uses `tmux send-keys -t <pane> <cmd> Enter` which is correct. The `Enter` argument makes tmux send a CR. But the 5-second sleep before auto-trigger (`start.rs:486`) may not have been enough — the SessionStart hook output shows the context loading is substantial (10.8KB persisted-output). If Claude Code's input handler wasn't fully initialized when `send-keys` arrived, the characters queued in the pty input buffer and got interleaved with DSR responses.

Once the input handler enters this corrupted state, Enter (CR/`\r`) is no longer recognized as the submit key — it renders as a visual newline instead.

**Fix options:**

1. **Immediate:** Kill the stuck session in pane %124 and restart: `tmux send-keys -t %124 C-c C-c` then re-run `agent-doc start`
2. **Auto-trigger robustness:** Instead of a fixed 5-second sleep, probe for the Claude Code `❯` prompt character in the pane output before sending. Something like:
   ```rust
   // Poll for prompt readiness
   for _ in 0..30 {
       let output = Command::new("tmux")
           .args(["capture-pane", "-t", &pane, "-p"])
           .output()?;
       let text = String::from_utf8_lossy(&output.stdout);
       if text.contains("❯") || text.contains("⏵") {
           break;
       }
       std::thread::sleep(Duration::from_millis(500));
   }
   ```
3. **DSR filtering:** Strip `\x1b[?NNN;NNNn` sequences from the pty output before they reach Claude Code's input handler. These are terminal-to-application responses that shouldn't be in the input buffer.

❯ Note the following interleaving also happens during `agent-doc start`

```
❯ /home/brian/.cargo/bin/agent-doc start tasks/dreamable/robert-ross.md
Registered session 6cb2a9bf → pane %125
Starting claude...
^[[?997;1n^[P>|tmux 3.6a^[\^[[?1;2;4c ▐▛███▜▌   Claude Code v2.1.109
```

❯ Please fix immediately
❯ ### Re: DSR interleaving during agent-doc start — opus-4-6 (HEAD)

Fixed. The auto-trigger in `start.rs` now polls `tmux capture-pane` for the Claude Code prompt character (`❯` or `⏵`) instead of using a fixed 5-second sleep.

**What changed (`start.rs:480–549`):**

- **Before:** `std::thread::sleep(Duration::from_secs(5))` then blind `tmux send-keys`
- **After:** 2s initial grace period, then poll every 500ms (up to 60 attempts / ~30s timeout) checking the last 20 lines of pane output for a prompt character. Only sends keys once the prompt is confirmed ready.

**Why this fixes the DSR interleaving:**

The race was: Claude Code starts → terminal queries capabilities → DSR responses (`^[[?997;1n`, `^[P>|tmux 3.6a^[\`, `^[[?1;2;4c`) flow through the pty → fixed 5s sleep fires `send-keys` while these sequences are still in-flight or buffered → command bytes interleave with escape sequences → Claude's readline state corrupts → Enter no longer submits.

By waiting for the `❯` prompt to actually appear in the pane, we guarantee Claude Code has:
1. Finished terminal capability negotiation (DSR sequences consumed)
2. Rendered its banner and loaded context
3. Presented an interactive input prompt ready to receive text

**Build status:** clippy clean, 1055 tests pass, release binary installed.

**Manual testing gate:** needs a live `agent-doc start` with `--continue` restart to confirm the prompt polling works end-to-end.
❯ The issue is still happening with an `agent-doc start`. Is the new build installed?
❯ Please address the Claude code banner/tmux output issue? This is consistent
❯ `^[[?997;1n^[P>|tmux 3.6a^[\^[[?1;2;4c ▐▛███▜▌   Claude Code v2.1.109`
### Re: DSR interleaving in pty reader — opus-4-6

The previous fix only addressed the auto-trigger timing. The real issue is that terminal capability queries from Claude Code pass through the pty→stdout reader to the outer tmux, which responds with DSR/DA/DCS sequences that echo as visible garbage.

**New fix: `filter_terminal_queries()` in `start.rs`**

Added a byte-level escape sequence filter in the reader thread that strips:

| Sequence | Pattern | Example |
|----------|---------|---------|
| DSR response | `ESC [ ? ... n` | `^[[?997;1n` |
| DA1 response | `ESC [ ? ... c` | `^[[?1;2;4c` |
| DA2 response | `ESC [ > ... c` | `^[[>0;115;0c` |
| DCS string | `ESC P ... ESC \` | `^[P>|tmux 3.6a^[\` |

Normal escape sequences (SGR colors, cursor movement, etc.) pass through unchanged.

**How it works:** The reader thread now filters every read through `filter_terminal_queries()` before writing to stdout. This prevents:
1. Terminal queries from reaching the outer tmux (no spurious responses generated)
2. Echoed responses from appearing as visible garbage in the pane

**Tests added (7):** DSR response, DA1, DA2, DCS string, interleaved real-world sequence, SGR preservation, plain text passthrough.

**Build:** clippy clean, 1062 tests pass, release binary installed.

**Manual testing gate:** needs a live `agent-doc start` to confirm the banner appears clean without `^[[?997;1n^[P>|tmux 3.6a^[\^[[?1;2;4c` prefix.
<!-- agent:boundary:496aaa3f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#p2xm] Orphaned patch:exchange marker leak: parse_patches() does not consume unclosed opening markers — they leak into unmatched text and get appended to exchange verbatim. Fix: advance last_end past unclosed openers in parse_patches()
- [/] [#wcf5] Advisory flock doesn't protect external watchers: IDE file-change listeners and git hooks race concurrent diff computations against stale snapshots
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
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
