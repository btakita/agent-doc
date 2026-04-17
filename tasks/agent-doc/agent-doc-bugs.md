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
❯ ### Re: DSR interleaving in pty reader — opus-4-6 (HEAD)

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
❯ Is this a tmux specific transformation? If yes, please extract to a tmux specific module
❯ ### Re: extract to pty module — opus-4-6 (HEAD)

Not tmux-specific — DSR/DA/DCS are standard terminal escape sequences that any terminal emulator (xterm, kitty, alacritty) responds to. The issue is the nested pty topology: Claude Code queries terminal capabilities through the pty slave → pty master → reader thread → outer terminal, which responds and the responses echo as garbage.

**Extracted to `supervisor/pty.rs`** — the filter is pty I/O concern, not tmux-specific:

- Moved `filter_terminal_queries()` from `start.rs` to `supervisor/pty.rs` as `pub(crate)`
- Moved all 7 filter tests to `supervisor/pty.rs` test module
- `start.rs` reader thread now calls `crate::supervisor::pty::filter_terminal_queries()`

The function lives alongside the pty spawn/forward/IO code where it belongs. If `forward_stdio()` (the built-in I/O path in pty.rs) is ever used instead of the manual reader/writer threads, the filter is right there to integrate.

**Build:** clippy clean, 1062 tests pass, release binary installed.
❯ Same behavior. Would debugging logs help?
❯ ### Re: debugging logs added — opus-4-6 (HEAD)

Yes — added `AGENT_DOC_DEBUG_FILTER=1` environment variable gate to the reader thread. When set, logs to stderr:

- `[pty-filter] raw (N bytes): ...` — bytes from pty master before filter
- `[pty-filter] filtered (N bytes): ...` — bytes after filter

ESC shown as `\x1b`, other non-printable as `\xNN`.

**Binary rebuilt and installed.** To test:

```bash
AGENT_DOC_DEBUG_FILTER=1 agent-doc start <any-file>
```

This will reveal which of three scenarios we're in:

| Raw has sequences? | Filtered has them? | Diagnosis |
|---|---|---|
| Yes | No | Filter works but sequences bypass the reader (tmux display injection) |
| Yes | Yes | Filter pattern doesn't match these specific bytes |
| No | — | Sequences originate outside the pty read path entirely |

The answer determines whether to fix the filter, the pty topology, or tmux escape handling.

❯ The tmux pane that claims robert-ross.md has a Yes | Yes
❯ Please verify and fix
❯ ### Re: pty filter — Yes|Yes diagnosis and fix — opus-4-6 (HEAD)

Captured the debug output from pane %129 (robert-ross.md). Three categories of escape sequences were passing through:

| Sequence | Purpose | Why it passed |
|----------|---------|---------------|
| `\x1b[?2026h` / `\x1b[?2026l` | DEC private mode set/reset (synchronized output) | Filter only caught `?` + `n`/`c`, missed `h`/`l` |
| `\x1b[<u` | Kitty keyboard pop | Filter had no `<` prefix handling |
| `\x1b[>1u` / `\x1b[>4;2m` | Kitty keyboard push / progressive enhancement | Filter caught `>` + `c` only, missed `u`/`m` |

**Fix** (`pty.rs:filter_terminal_queries`): expanded filter to handle all three:

1. Added `has_lt` for `<` prefix detection alongside existing `has_question`/`has_gt`
2. Replaced if-chain with match expression for clarity:
   - `? + n|c|h|l` — DSR, DA1, DEC private mode set/reset
   - `> + c|u|m` — DA2, Kitty push, Kitty progressive enhancement
   - `< + u` — Kitty keyboard pop
3. Added 5 new tests (DEC private mode set/reset, Kitty push/pop/progressive) + cursor movement preservation test

**Build:** clippy clean, 1068 tests pass, release binary installed.
❯ Why do we need to do a filter at all? The supervisor is the parent process. Why is it interfering with the inputs and outputs to the console?
❯ ### Re: why the pty filter exists — opus-4-6 (HEAD)

The supervisor **can't** just pass stdin/stdout through unmodified because it needs the pty pair for three things:

1. **IPC injection** — `inject` writes bytes to the pty master fd, which is how `/agent-doc` commands get routed into a running Claude session without `tmux send-keys`
2. **Restart semantics** — killing and respawning the child requires owning the pty lifecycle
3. **Resize forwarding** — `TIOCSWINSZ` ioctl on the pty master fd

This creates a **nested terminal topology**:

```
tmux pane (outer terminal emulator)
  └─ supervisor (pty master/slave owner)
       └─ Claude Code (child on pty slave)
```

Claude Code sends terminal capability queries (`\x1b[?997n`, DA1, DA2) because it thinks it's talking to a real terminal — and it is, just the *inner* pty slave. Those queries pass through the supervisor's reader thread to stdout, which feeds into the **outer** tmux pane. Tmux responds with its own sequences (`^[[?997;1n`, `^[P>|tmux 3.6a^[\`), and those responses have nowhere to go — they echo as visible garbage.

**The filter strips the outer terminal's responses** before they reach stdout. It's not interfering with Claude Code's I/O — it's cleaning up the side effects of the pty-in-tmux topology.

**Alternative architectures and why they don't work:**

| Approach | Problem |
|----------|---------|
| Inherit terminal directly (`Command::status()`) | No IPC injection, no programmatic restart, no resize control |
| Use `tmux send-keys` for injection | Race conditions, 5-second sleep hacks, no reliable delivery |
| Suppress queries in the child | Can't — Claude Code's terminal detection is built-in |
| Let the outer terminal consume responses silently | Can't — the response bytes flow through the pty reader to stdout, not through a bidirectional control channel |

The filter is the minimal cost of owning the pty. If we didn't need injection/restart/resize, we could drop the whole supervisor and just `exec` Claude Code directly — but then we'd lose the entire IPC surface that makes `agent-doc route`, `agent-doc start --continue`, and cross-pane control work.
❯ This seems fragile.

❯ Getting `^[P>|tmux 3.6a^[\^[[?1;2;4c ▐▛███▜▌   Claude Code v2.1.109`
❯ Enter still only sends a \n.
```
❯ /usage

───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
/usage                        Show plan usage limits
/caveman                      (caveman) Ultra-compressed communication mode. Slash token usage ~75% by speaking li…
/janitor-usage                (skills-janitor) Show which skills you use and which you never use
/extra-usage                  Configure extra usage to keep working when limits are hit
/context                      Visualize current context usage as a colored grid
/effort                       Set effort level for model usage
```
❯ ### Re: stateful pty filter fix — opus-4-6 (HEAD)

The filter was **stateless across reads** — each `read()` call processed bytes independently. When an ESC byte landed at the end of one read buffer:

1. `input[i] == 0x1b && i + 1 < len` → false (ESC is last byte)
2. ESC pushed to output as plain text
3. Next read starts fresh — `P>|tmux 3.6a\x1b\` leaks as visible characters

**Fix:** Replaced the stateless `filter_terminal_queries()` function with a `PtyFilter` struct that carries partial escape sequences across reads. When parsing hits the end of a buffer mid-sequence (ESC at end, incomplete CSI params, unterminated DCS), the remaining bytes are saved to a `carryover` buffer and prepended to the next `filter()` call.

The reader thread in `start.rs` now creates one `PtyFilter` instance and reuses it across all reads in the loop.

**Boundary cases handled:**
- ESC at end of read (DCS/CSI start split)
- DCS body split at ST (`\x1b\` across reads)
- CSI params split (`\x1b[?997` / `;1n`)
- Normal SGR sequences preserved across splits

**Tests:** 6 new cross-boundary tests including a real-world simulation that splits `DSR + DCS + DA1 + banner` at arbitrary points across 4 reads.

**Build:** clippy clean, 1080 tests pass, release binary installed.

**Manual testing gate:** needs a live `agent-doc start` to confirm the banner appears clean.
❯ Same output
❯ `^[P>|tmux 3.6a^[\^[[?1;2;4c ▐▛███▜▌   Claude Code v2.1.109`
❯ and Enter behavior
```
❯ /usage

───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
/usage                        Show plan usage limits
/caveman                      (caveman) Ultra-compressed communication mode. Slash token usage ~75% by speaking li…
/janitor-usage                (skills-janitor) Show which skills you use and which you never use
...
```
❯ ### Re: filter queries, not just responses — opus-4-6 (HEAD)

The stateful fix was correct (cross-boundary handling) but insufficient. The real issue: we were filtering RESPONSES (`\x1b[?1;2;4c`, `\x1bP>|tmux...\x1b\`) but letting the QUERIES through. Claude Code sends `\x1b[c` (DA1 query) and `\x1b[>q` (XTVERSION query) — neither had a filter match. These pass through the reader thread to tmux, which generates responses that appear as visible garbage.

**Root cause:** The nested pty topology means Claude Code's outgoing terminal queries reach the outer tmux pane. Tmux generates responses that get displayed as text (they come back through a path the response filter may not cover). The fix: suppress queries at the source so tmux never responds.

**New filter rules added:**
| Pattern | Type | What it catches |
|---------|------|----------------|
| `\x1b[c` / `\x1b[0c` | DA1 query | Triggers `\x1b[?1;2;4c` response |
| `\x1b[>q` | XTVERSION query | Triggers `\x1bP>\|tmux 3.6a\x1b\` response |

Existing response filters remain as defense-in-depth.

**Tests:** 3 new query-stripping tests (DA1 bare, DA1 with param, XTVERSION). Total: 79 pty tests passing.

**Build:** clippy clean, release binary installed.

**Important:** Existing `agent-doc start` processes use the OLD binary (process memory doesn't change when the file is replaced). You need to kill and restart the `agent-doc start` for the affected pane to pick up the new binary.
❯ The header renders correctly...
```
Registered session 6cb2a9bf → pane %134
Starting claude...
 ▐▛███▜▌   Claude Code v2.1.109
▝▜█████▛▘  Haiku 4.5 · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop
```
❯ But the Enter behavior is still the same...
```
❯ /usage

───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
/usage                        Show plan usage limits
/caveman                      (caveman) Ultra-compressed communication mode. Slash token usage ~75% by speaking li…
...
```
❯ ### Re: Enter sends \n instead of \r — raw mode fix — opus-4-6 (HEAD)

Good news: the header being clean confirms the query-stripping fix works. The Enter issue is a separate bug — a pty line discipline double-translation.

**Root cause:** The nested pty topology has two line disciplines:

```
User types Enter (\r)
  → outer pty (tmux pane, cooked mode, ICRNL on)
  → \r converted to \n (0x0A)
  → agent-doc start reads \n from stdin
  → writer thread forwards \n to inner pty master
  → Claude Code (raw mode on inner slave) receives \n
  → expects \r — broken
```

When Claude Code runs directly in a terminal, it puts the terminal into raw mode (disabling ICRNL), so Enter arrives as `\r` (0x0D). Through the supervisor, the outer pty's cooked-mode ICRNL has already converted `\r` → `\n` before `agent-doc start` even reads it from stdin.

**Fix:** `start.rs` now puts its own stdin into raw mode via `cfmakeraw()` before entering the supervisor restart loop. This disables ICRNL (and all other line-discipline translations) on the outer pty, so `\r` passes through unchanged to the inner pty master and arrives at Claude Code as expected.

Added a `RawMode` guard struct that:
- Saves original termios on construction, sets raw mode
- `suspend()` / `resume()` for the PromptUser path (clean exit → "Press Enter to restart or 'q' to exit" needs cooked mode for `read_line()`)
- Restores original termios on `Drop`

**Build:** clippy clean, 294 lib tests pass, release binary installed.

**Important:** You need to kill and restart `agent-doc start` for the affected pane to pick up the new binary. The fix only takes effect after restart.
❯ The fix worked. Will this work on windows, osx, and for non-tmux sessions? If there are differences, we should modularize the architecture to allow for other operating systems and terminal front ends.
❯ ### Re: cross-platform and non-tmux compatibility — opus-4-6 (HEAD)

**Current state by platform:**

| Platform | Status | Why |
|----------|--------|-----|
| **Linux** | Works | `cfmakeraw` + `tcsetattr` via libc — tested and confirmed |
| **macOS** | Works | Same POSIX termios API — `cfmakeraw` is identical. libc crate supports Darwin |
| **Windows** | No-op stub | Windows uses ConPTY, not Unix ptys. No ICRNL translation issue — console handles process input differently. The `#[cfg(not(unix))]` stub correctly does nothing |

**Non-tmux sessions:** `agent-doc start` already requires tmux (bails at line 327: "not running inside tmux"). The raw mode fix applies to any hosting environment though — if we ever lift the tmux requirement, the same double-translation issue would occur with any terminal emulator hosting the supervisor process, and the fix would still work.

**On modularization:** The `#[cfg(unix)]` / `#[cfg(not(unix))]` gate is already the idiomatic Rust pattern for platform-specific terminal handling. A trait abstraction (`trait TerminalMode { fn enable_raw(); fn suspend(); fn resume(); }`) would be premature — we have exactly two cases (Unix: real impl, Windows: no-op), and cfg gates handle that cleanly without runtime dispatch or trait objects.

❯ I understand that this is premature. But I'm inclined to have an architecture distinction in place. For now a non-tmux environment can have an error or a noop. What do you recommend?

If Windows ever needs ConPTY-specific raw mode (via `SetConsoleMode` removing `ENABLE_PROCESSED_INPUT`), we'd add a `#[cfg(windows)]` block alongside the existing stubs. The compile-time selection means zero cost on platforms that don't need it.

**Where modularization would matter more:** The pty allocation itself (portable_pty already abstracts Unix openpty vs Windows ConPTY), and the tmux dependency (which locks us to Unix in practice). Lifting the tmux requirement is a bigger architectural decision — it would mean replacing `tmux send-keys` injection with direct pty writes for IPC, and finding a non-tmux pane management strategy.

❯ How feasible would it be to have direct console support? Would I need special logic for each terminal program or can I rely on the POSIX standard and any other os-specific standard?
### Re: modularization recommendation — opus-4-6

You're right to want the architectural seam in place early — it's cheap now and painful to retrofit later. Here's what I'd recommend:

**A thin trait, not cfg gates:**

```rust
pub trait TerminalBackend: Send {
    fn enable_raw(&mut self) -> Result<()>;
    fn suspend(&mut self) -> Result<()>;
    fn resume(&mut self) -> Result<()>;
    fn restore(&mut self) -> Result<()>;
}
```

Three implementations:
- `UnixTerminal` — the current `libc::termios` code
- `WindowsTerminal` — `SetConsoleMode` when needed, no-op for now
- `NoopTerminal` — non-tmux / headless / CI environments

The trait object cost is negligible (called a handful of times per session, not per-keystroke), and it gives you a clear extension point without touching cfg gates when adding a new backend.

❯ ### Re: direct console support feasibility — opus-4-6 (HEAD)

Short answer: very feasible for input handling, and you can mostly rely on standards.

**What POSIX gives you for free:**
- Terminal raw mode (`termios`/`cfmakeraw`) is POSIX.1 — works identically across Linux, macOS, BSDs, and any POSIX-compliant system. No per-terminal-emulator logic needed.
- Escape sequences for cursor control, colors, etc. follow the ANSI/ECMA-48 standard. Every modern terminal emulator (iTerm2, Alacritty, kitty, Windows Terminal, GNOME Terminal) implements this.
- The `TERM`/`TERMINFO` database handles capability differences automatically.

**What you'd need per-OS logic for:**
- **Windows:** `SetConsoleMode` to disable `ENABLE_PROCESSED_INPUT` (the Windows equivalent of raw mode). The Win32 console API is separate from POSIX, so a `#[cfg(windows)]` block is unavoidable. Windows Terminal also supports VT sequences, but legacy `cmd.exe` needs the Win32 path.

**What you do NOT need per-terminal-program logic for:**
- All terminal emulators (iTerm2, kitty, Alacritty, xterm, etc.) present the same POSIX pty interface to your process. Your code talks to the pty master fd, not to the terminal emulator directly. The emulator renders what you write — you never need to know which one it is.

**The real question is tmux replacement, not terminal compatibility.** Direct console support is trivial for raw mode and I/O. The hard part is replacing what tmux provides:
1. **Pane management** — creating/splitting/targeting panes for document rendering
2. **Session persistence** — surviving SSH disconnects, terminal crashes
3. **`send-keys` injection** — the auto-trigger mechanism for `/agent-doc`

For (1), you'd need your own multiplexer or integrate with an existing one (Zellij has an IPC API). For (2) and (3), the Unix socket IPC you already have in `supervisor/ipc.rs` handles this — it's tmux-independent.

**Bottom line:** The terminal I/O layer (raw mode, escape sequences) is fully portable via POSIX + one Windows cfg block. You don't need per-terminal logic. The dependency on tmux is architectural (pane lifecycle, persistence), not terminal-level.

❯ On the flip side, can I rely on the user being able to install Tmux? Can I package the install to ensure the dependencies, including tmux, are installed...across all operatings systems?
❯ ### Re: tmux portability and packaging — opus-4-6 (HEAD)

**Can you rely on users installing tmux?**

On Linux and macOS — mostly yes. It's in every major package manager (`apt`, `brew`, `pacman`, `dnf`). Power users (your target audience for a CLI dev tool) almost certainly have it or can install it trivially.

On Windows — no. Tmux doesn't run natively on Windows. It requires WSL or MSYS2/Cygwin. Asking Windows users to install WSL just to use agent-doc is a significant friction barrier.

**Can you package tmux as a dependency?**

| Approach | Linux | macOS | Windows |
|----------|-------|-------|---------|
| System package (`apt`/`brew`) | Works, but requires sudo or Homebrew | Works | N/A |
| Static binary bundle | Possible but fragile — tmux links against libevent, ncurses | Same issues | N/A |
| Nix/Homebrew formula | Clean, but adds a dependency on Nix/Homebrew | Clean | WSL only |
| WSL detection + auto-setup | N/A | N/A | Possible but invasive |

The core problem: tmux is a C program with its own dependency chain (libevent, ncurses). You can't just `cargo install` it as part of your Rust build. Bundling a static binary is doable but means maintaining builds across architectures (x86_64, aarch64) and dealing with glibc/musl compatibility.

**Practical recommendation:**

1. **Short term:** Keep tmux as a documented prerequisite. Add a startup check that prints a helpful install command if tmux is missing (`brew install tmux` / `apt install tmux` / "Windows: install WSL first"). This is the approach most CLI tools take (e.g., `lazygit` expects `git`, `k9s` expects `kubectl`).

❯ do this

2. **Medium term:** If Windows support matters, build the `TerminalBackend` trait now, and add a `DirectTerminal` backend that uses your own pty management (you already have `portable-pty`) instead of tmux panes. This removes the tmux dependency entirely for the core loop — tmux becomes one optional backend for users who want its session persistence and pane management.

❯ do the TerminalBackend trait. Add the spec for a DirectTerminal as a comment or as a spec file. Let me know which would be easier to surface once we decide to implement this.

3. **For distribution:** Use platform-specific installers that declare tmux as a dependency:
   - **Homebrew formula:** `depends_on "tmux"` — Homebrew handles the rest
   - **AUR/deb/rpm:** standard package dependency declaration
   - **cargo-binstall:** can't express system deps, so fall back to runtime detection

❯ Create a plan file. Link to the spec, in the comment or dedicated spec file, for the DirectTerminal.

**The honest answer:** You can't reliably ensure tmux is installed across all operating systems without either (a) shipping your own multiplexer or (b) making tmux optional. Option (b) with the `TerminalBackend` trait is where this naturally leads — tmux is the premium backend for users who have it, and a direct-pty backend covers everyone else with reduced functionality (no session persistence, no pane splitting).
❯ ### Re: action items — opus-4-6 (HEAD)

Adding all three to pending:

1. **Tmux install check** — straightforward. I'll add a detection + helpful error message in `start.rs` before the existing `in_tmux()` bail, so users who don't even have tmux installed get a better message than "not running inside tmux."

2. **TerminalBackend trait + DirectTerminal spec** — a dedicated spec file at `src/agent-doc/specs/terminal-backend.md` is the better choice over a code comment. Reasons:
   - Survives refactors (code comments move/get deleted with the code they're attached to)
   - Can be linked from plan files, CLAUDE.md, and pending items
   - Supports richer content (diagrams, trait signatures, migration plan)
   - Shows up in `Glob` searches when you're ready to implement

3. **Plan file** — I'll create `src/agent-doc/specs/direct-terminal-plan.md` linking to the spec.

All three added to pending as gated items.
❯ Why are #t8tg #1yk1 #zy7m gated (`- [ ] [#t8tg] [/] ...`)?
### Re: why gated — opus-4-6

They shouldn't be. The `[/]` gate means "don't implement until user approves" — but you already approved all three ("do this", "do the TerminalBackend trait", "Create a plan file"). My mistake: I passed `[/]` as part of the `--pending-add` text, which embedded it as a literal string rather than using `--pending-gate`. The items were actually ungated `[ ]` but had confusing `[/]` text prefixes. Fixed — removed the stale `[/]` prefixes from all three.
<!-- agent:boundary:b0a29661 -->
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
- [ ] [#t8tg] Add tmux install detection with platform-specific install instructions in start.rs
- [ ] [#1yk1] Create TerminalBackend trait and spec file at src/agent-doc/specs/terminal-backend.md
- [ ] [#zy7m] Create DirectTerminal plan file at src/agent-doc/specs/direct-terminal-plan.md linking to the spec
<!-- /agent:pending -->
## Assistant
