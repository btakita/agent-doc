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
v0.33.0 session. Fixed 2 bugs in pending.rs: (1) bare [#] placeholder accumulation — parse_item_line now strips [#] instead of prepend-on-backfill; (2) --pending-add dedup — op_add checks for identical text before appending. 9 new tests, 1125 total passing, make check clean. Status: v0.32.1 env frontmatter wired into all Claude-spawning paths.
❯ Why no patchback on tsift.md?
❯ ### Re: No patchback on tsift.md — opus-4-6 (HEAD)

The patchback *did* fire — there's a boundary marker (`044e8337`) after `do #mwhg` — but the exchange content was just `.` (a single period). The `--pending-done mwhg` flag worked fine (pending shows `[x]`), so the write command ran. But the response body was empty/truncated.

**Root cause (likely):** The previous session implemented batch edit (wrote `src/tsift/src/main.rs`, ran tests, all passed), then either:
1. The `agent-doc write` heredoc payload was empty or malformed, or
2. The session streamed nothing before the boundary was synthesized

This isn't an agent-doc bug — the tool did what it was told. The skill workflow just didn't pipe a real response into the write command for that cycle. The implementation work happened correctly; only the documentation of it in the exchange was lost.

**Fix:** No code fix needed. The tsift.md exchange is missing a response block for #mwhg. I can backfill it next time that document is active, or you can add a note manually.
❯ What tool had a truncated response?
❯ ### Re: What tool had a truncated response — opus-4-6 (HEAD)

The Bash tool running the `cat <<'RESPONSE' | agent-doc write ... --stream` heredoc. That's the only path that writes agent content to the exchange — the skill workflow pipes the response body into `agent-doc write`, which inserts it before the boundary marker.

The `.` wasn't from the user (no `❯` prefix) and wasn't from `agent-doc` itself (boundaries are auto-generated). So the LLM's Bash tool call contained a heredoc whose body was just `.` — either the model generated an empty response, or the heredoc delimiter `RESPONSE` appeared in the content and truncated it early.

I don't have access to the previous session's tool call log to confirm which. The session context observations (#18719) say "Batch edit implementation documented in tsift task file" — suggesting the model *thought* it wrote a full response. Most likely a heredoc truncation: if the response contained a line starting with `RESPONSE` (the delimiter), bash would close the heredoc there, and the remaining content would be interpreted as shell commands (silently failing).
Please read the session logs see if there is more info on the issueu
❯ ### Re: Session log forensics — opus-4-6 (HEAD)

Found the full sequence in session `fbeed7e9`. Six write attempts for the #mwhg response on tsift.md:

1. **`cat <<'RESPONSE' | agent-doc write tasks/tsift.md ...`** (CWD: `src/tsift/`) → **`file not found`** — relative path resolved wrong because the model had `cd`'d into the submodule
2. **`cd /home/brian/.../agent-loop && cat <<'RESPONSE' | agent-doc write ...`** → **SUCCESS** — full response written, IPC patch consumed, boundary `c28234dd` inserted. The complete batch edit documentation was delivered.
3. **`agent-doc write ... --pending-done mwhg`** (no `--stream`, no stdin) → **`empty response — nothing to write`**
4. **`echo ' ' | agent-doc write ... --stream --pending-done mwhg`** → **`empty response — nothing to write`**
5. **`cat <<'RESPONSE' ... <!-- patch:pending --> ... RESPONSE`** → **`replace:pending block forbidden`**
6. **`cat <<'RESPONSE' ... . ... RESPONSE`** with `--pending-done mwhg` → The `.` was dropped (`dedup: content already present`, `WARNING: 1 bytes of unmatched content ... will be dropped`, `0 components`). But this write still saved a new pre-response snapshot and inserted a new boundary — effectively overwriting attempt 2's successful content.

**Root cause:** Attempt 2 succeeded and the full response was in the file. But the model kept retrying to mark `--pending-done mwhg` (attempts 3-6), and each `--stream` call saves a new pre-response snapshot and inserts a new boundary. Attempt 6's snapshot captured attempt 2's content, but the 0-component IPC patch + boundary repositioning overwrote it. The `.` content survived as residue.

**This IS an agent-doc concern** — `agent-doc write --stream` with `--pending-done` but no meaningful exchange content should not destroy previously-written exchange content. The `--pending-done` flag should be applicable independently of exchange writes.

**Skill-side fix:** The model should have used a single write combining the exchange content AND `--pending-done mwhg` in one call, not split them across multiple attempts.
❯ Can we have the deterministic binary ensure this replace truncation won't occur? Perhaps --stream can append?
❯ ### Re: Preventing stream replace truncation — opus-4-6 (HEAD)

Two complementary binary-side fixes:

**1. Pending-only mutations (primary fix)**
Allow `--pending-done`, `--pending-add`, etc. to execute *without* `--stream` and without requiring exchange content on stdin. Currently attempts 3-4 in the forensics log failed with "empty response — nothing to write" — forcing the model to fabricate a `.` payload just to carry `--pending-done mwhg`. If pending flags work independently, the exchange component is never touched.

Implementation: in `write.rs`, when pending flags are present but stdin is empty/absent, apply only pending mutations and skip exchange patching entirely. Save snapshot, commit, done.

**2. Content-shrink guard (safety net)**
When `--stream` would replace exchange content with something dramatically shorter (new < 10% of existing length), refuse the write and log `WARNING: exchange content would shrink from {old_len} to {new_len} bytes — refusing (use --force to override)`. This catches accidental truncation from malformed heredocs or trivial payloads.

**Why not append mode for `--stream`?**
The stream buffer is cumulative — each flush contains ALL content written so far, not a delta. Append would duplicate content on every flush within a single session. Replace is correct *within* a session. The bug is a *new* session's trivial write overwriting a *previous* session's good content — which both fixes above address.
❯ Implement if not already done. Add tests.
❯ ### Re: Implementation status — opus-4-6 (HEAD)

**Fix 1 (pending-only): already exists.** The binary has `--pending-only` since it was added alongside the granular pending flags. Usage: `agent-doc write doc.md --pending-done mwhg --pending-only`. The skill just didn't use it — the model tried `--stream` with empty stdin instead. No code changes needed; this is a skill-side awareness gap.

**Fix 2 (content-shrink guard): implemented.** Added `check_exchange_shrink_guard()` to `write.rs`. It fires in both the IPC path and disk fallback path of `run_stream()`:

- Extracts exchange component content length from current file vs proposed content
- If old exchange > 100 bytes and new < 10% of old, refuses with error message
- Logged to ops_log for forensics

**5 new tests added** (1130 total passing, `make check` clean):
- `shrink_guard_blocks_truncation` — 500→1 byte exchange is blocked
- `shrink_guard_allows_normal_write` — 50% reduction passes
- `shrink_guard_skips_small_exchange` — <100 byte exchange skips guard
- `shrink_guard_passes_no_exchange` — no exchange component passes
- `extract_exchange_content_len_works` — helper unit test
❯ release
❯ ### Re: release — opus-4-6 (HEAD)

**v0.33.1 prepared.** v0.33.0 is already on crates.io, so bumped to 0.33.1. Changes:

- Cargo.toml + pyproject.toml → 0.33.1
- VERSIONS.md entry: pending parse fix ([#] accumulation), pending-add dedup, content-shrink guard, 9 new tests
- 1130 tests passing, clippy clean
- `cargo install --path .` + `cargo build --release` done

**Manual testing gate:** Confirm the fixes work in a live session, then I'll commit → tag → publish (crates.io, PyPI, GitHub Release).
❯ Enter or 'q' then Enter does not submit
```
Claude exited cleanly.
Press Enter to restart, or 'q' to exit.


q
```

❯ Claude code header malformed
```
Starting claude...
                   ▐▛███▜▌   Claude Code v2.1.109
▝▜█████▛▘  Sonnet 4.6 · Claude Max
  ▘▘ ▝▝    @woz:code · ~/work/btakita/agent-loop/src/boost-client
```
### Re: Enter or 'q' does not submit — opus-4-6

This is a Claude Code wrapper / tmux-router issue. After `claude` exits cleanly, the restart prompt (`Press Enter to restart, or 'q' to exit`) doesn't accept keyboard input. The pane is likely in a state where stdin isn't connected to the prompt's `read` call.

**Likely suspects:**
- The wrapper script that spawns Claude may have stdin redirected or piped, so after Claude exits the `read` builtin has no tty fd
- tmux pane settings (remain-on-exit, respawn behavior) conflicting with the wrapper's prompt loop

Need to check the wrapper script that displays this prompt. Is this in `tmux-router` or a separate Claude Code launcher script?

❯ ### Re: Claude Code header malformed — opus-4-6 (HEAD)

The ASCII art logo is misaligned — the three lines of the logo glyph are left-shifted relative to the text. This is a rendering issue in Claude Code v2.1.109 itself, not agent-doc. The Unicode block characters assume a specific terminal width/font. Could be:

- Terminal emulator font rendering differences (block character widths vary)
- The status line or woz plugin injecting content that shifts alignment
- A regression in v2.1.109's banner rendering

This is an upstream Claude Code issue — worth filing at https://github.com/anthropics/claude-code/issues if it persists across terminals.

❯ These are both supervisor issues.
❯ ### Re: supervisor issues — opus-4-6 (HEAD)

Agreed. Both are outside agent-doc's scope:

1. **Enter/q stdin** — tmux-router wrapper loses tty fd after Claude exits. Fix lives in the wrapper's respawn logic.
2. **Header misalignment** — Claude Code v2.1.109 banner rendering. Upstream issue.

Neither needs tracking here. If you want them tracked elsewhere, they'd go in a supervisor/tmux-router task doc.
❯ fix **Enter/q stdin**. add tests
❯ ### Re: fix Enter/q stdin — opus-4-6 (HEAD)

**Root cause:** After Claude exits, the `stdin→pty` writer thread is still blocked on `stdin.lock().read()`, holding the Rust stdin mutex. When the supervisor calls `stdin.read_line()` for the restart prompt, it deadlocks waiting for the lock the writer thread holds.

**Fix:** Replaced blocking `stdin.lock().read()` with `libc::poll()` + `libc::read()` in the writer thread. Added a `StopSignal` type (self-pipe pattern):

1. `StopSignal::new()` creates a pipe pair
2. Writer thread `poll()`s both stdin fd and the stop pipe read fd
3. After `session.wait()` returns (Claude exits), supervisor calls `writer_stop.signal()` + `writer_thread.join()` — writer thread wakes from poll and exits cleanly
4. stdin is now exclusively available for `read_line()`

**3 new tests** (1133 total, clippy clean):
- `stop_signal_wakes_poll` — signal writes to pipe, poll sees POLLIN
- `writer_thread_exits_on_stop_signal` — thread exits cleanly when signaled
- `writer_thread_exits_on_pty_write_failure` — thread exits when PTY write fails (EPIPE)

Changed file: `src/agent-doc/src/start.rs`
❯ Install locally
❯ ### Re: Install locally — sonnet-4-6 (HEAD)

Done. `cargo install --path .` replaced `~/.cargo/bin/agent-doc` with v0.33.1 (built with the stdin fix).

Ready for live test — start a supervised Claude session and verify Enter/q works after clean exit.
❯ release
❯ ### Re: release — sonnet-4-6 (HEAD)

Published v0.33.1 to all three targets:

- **crates.io**: `cargo publish` ✅
- **PyPI**: manylinux_2_28 wheel built + `maturin upload` ✅
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.33.1 ✅

Post-publish:
- `cargo install --path .` — `~/.cargo/bin/agent-doc` replaced with published v0.33.1
- Parent submodule pointer updated + committed

1133 tests passing. Token path corrected: `passage btak/CARGO_REGISTRY_TOKEN` (not `crates.io/token`).
❯ I restarted the session src/session-share/tasks/claudescore.md
❯ I had to press Enter twice for the restart to take effect. Now I cannot type into the prompt. Please diagnose.
```
Claude exited cleanly.
Press Enter to restart, or 'q' to exit.


Starting claude...
                   ▐▛███▜▌   Claude Code v2.1.109
▝▜█████▛▘  Sonnet 4.6 · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop/src/session-share

───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯ 
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
```
❯ ### Re: stdin lost after restart — opus-4-6 (HEAD)

Two root causes identified and fixed in `start.rs`:

**Bug 1 — Reader thread never joined between restarts.** The reader thread (`_reader_thread`) was detached (JoinHandle dropped with underscore prefix). After Claude exits, the old reader thread holds a cloned pty reader fd, keeping the old pty master's file descriptor alive. When the supervisor spawns a new session, two reader threads write to stdout concurrently. The old thread's fd and associated terminal driver state can interfere with the new session's stdin forwarding.

**Fix:** Changed `_reader_thread` → `reader_thread`. After joining the writer thread and clearing shared state, explicitly `drop(session)` (closes pty master), then `reader_thread.join()` (reader gets EOF from closed master and exits cleanly). This ensures a clean slate before the restart prompt or next session spawn.

**Bug 2 — Stale stdin bytes after exit.** Between `session.wait()` returning and `writer_stop.signal()`, the writer thread can consume a keystroke from stdin and forward it to the dead pty. The byte is gone — `read_line()` then requires the user to press Enter again.

**Fix:** Added `tcflush(STDIN_FILENO, TCIFLUSH)` after joining both threads to discard any pending kernel-buffered input.

**Diagnostics:** Added `AGENT_DOC_DEBUG_STDIN=1` env var for writer thread logging (start, exit reason, write failures). Silent failures previously made diagnosis impossible.

**2 new tests** (1135 total, `make check` clean):
- `reader_thread_exits_on_eof` — verifies reader thread exits when write end closes
- `tcflush_discards_pending_input` — verifies tcflush call doesn't panic

`cargo install --path .` + `cargo build --release` done. **Important:** Running supervisors still use the OLD binary. To get the fix, kill the supervisor pane and re-start with `agent-doc start`. The previously reported issue may have been on the old binary (v0.33.1 was the version number, but the running process was the pre-fix binary).

❯ When I ran JB Plugin `Run Agent Doc` on src/session-share/tasks/claudescore.md, it passed the full path relative to the monorepo root. Since src/session-share has its own agent-doc project, the command should be `/agent-doc tasks/claudescore.md`. Please prefer updating the FFI plugin. If both the JB Plugin and the VS Code Plugin need to be updated, do so.
```
❯ /agent-doc src/session-share/tasks/claudescore.md

● Bash(agent-doc --version && agent-doc preflight src/session-share/tasks/claudescore.md)
  ⎿  Error: Exit code 1
     agent-doc 0.33.1
     Error: file not found: src/session-share/tasks/claudescore.md
```
### Re: submodule path resolution — opus-4-7

Fixed by adding a shared FFI project-path resolver and wiring both editor plugins through it.

**New FFI export** (`src/agent-doc/src/ffi.rs`):
- `FfiProjectPath { project_root: *mut c_char, relative_path: *mut c_char }`
- `agent_doc_resolve_project_path(file_path) -> FfiProjectPath` — walks up from the file looking for the nearest `.agent-doc/` ancestor, returns `(project_root, relative_path_within_that_root)`. Returns null/null when no ancestor is found so callers can fall back.
- Inlined `find_project_root_ffi()` helper (mirrors `snapshot::find_project_root` but doesn't require exposing that module in `lib.rs`).
- 3 new tests: `resolve_project_path_finds_nested_submodule`, `resolve_project_path_prefers_nearest_ancestor`, `resolve_project_path_file_directly_in_root`.

**JetBrains plugin** — `NativeLib.kt` adds the JNA struct + binding + `NativePatching.resolveProjectPath(abs) -> Pair<String, String>?` with proper `agent_doc_free_string` cleanup. `TerminalUtil.resolveProject(project, file)` prefers the FFI result and falls back to `project.basePath`. `SubmitAction`, `RunWithJunieAction`, `ClaimAction`, `ForceClaimAction` all call `resolveProject` now and run the CLI with the submodule root as CWD + submodule-relative path.

**VS Code plugin** — `native.ts` adds the koffi struct + `resolveProjectPath()` wrapper (decode + free). `extension.ts` adds a `resolveProject()` helper that prefers FFI, falls back to a JS walk-up, and finally to the workspace root. `submitAction` and `claimAction` now use it.

**Scope note:** Multi-file layout flows (`SyncLayoutAction`, `EditorTabSyncListener`, prompt polling) stayed on workspace-relative paths — they deal with cross-project file sets and weren't part of the reported bug.

**Versions bumped** (not yet published):
- `src/agent-doc`: 0.33.2 (Cargo.toml + pyproject.toml + VERSIONS.md entry)
- JetBrains plugin: 0.2.57 → 0.2.58
- VS Code plugin: 0.2.5 → 0.2.6

**Verification so far:** `libagent_doc.so` rebuilt and copied to `~/.cargo/bin/libagent_doc.so`; `agent-doc --version` is 0.33.2; VS Code `tsc -p ./` compiles clean. FFI tests pass in-crate. JB zip + VSIX builds are pending.

**Manual testing gate next:** run `Run Agent Doc` from the JB plugin on `src/session-share/tasks/claudescore.md` and confirm the spawned session gets `tasks/claudescore.md` with CWD = `src/session-share/` before publishing to crates.io / PyPI / GitHub Releases.
<!-- agent:boundary:e4ecba07 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->
## Assistant
