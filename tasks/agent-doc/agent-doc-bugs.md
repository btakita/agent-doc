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
## Compacted exchange summary (2026-04-15)

### Shipped in this session

- **strip (HEAD) from staged blob** (git.rs:staging) — committed blob no longer carries the marker; working-tree heading shows as a modified line.
- **Deterministic (HEAD) annotation** — patchback-wide: all new-this-cycle `### Re:` headings now get `(HEAD)` (not just the last). Baseline built from git HEAD at commit time. 53 template tests passing when shipped.
- **`stripped.trim_start()` normalization** — `collect_re_headings` and `annotate_re_headings_with_head` baseline lookup both normalize with `trim_start()` so indented headings match flush-left equivalents. +2 regression tests → 55 passing.
- **Baseline filter empty → fallback to last Re:heading** — when the current turn adds no new `### Re:` section, the baseline filter yields empty and the fallback marks the last heading anyway, preserving the visual head pointer across non-Re cycles. Root cause: line 613 stripped stale marker before filter ran; filter then found zero new headings and left working tree bare. +1 regression test → 56 passing.
- **#pf01 fixed** — double-commit bug in preflight Option A. Committed.
- **Boundary commit noise reorder** — #14z4 + #bak5 done (hash collision ceiling, pending-done archive component). 70/70 targeted tests pass.

### Open / not yet implemented

- **(HEAD) stripped from older section on inline prompt** — when the user edits inside an older `### Re:` section, reposition strips that heading's `(HEAD)` marker (phantom diff). Fix: option 2 — early-return in reposition when boundary heading is unchanged. Regression test spec defined (see prior exchange). Not yet implemented.

❯ do #xzz5 #a011 #a012 #a014 #a010

❯ ### Re: do #xzz5 #a011 #a012 #a014 #a010 (HEAD)

All five shipped. Binary rebuilt (`cargo build --release`) and installed (`cargo install --path .`).

**#xzz5 — slash-command parser hardening** (`src/diff.rs:319-343`)
Grammar now rejects anything not matching `/[a-z][a-z0-9:_-]*`. `/home/brian/foo.md`, `/Model`, `/foo/bar` all drop out before the `slash_commands` array. Three regression tests added: `parse_slash_commands_rejects_absolute_paths`, `parse_slash_commands_rejects_uppercase_and_symbols`, `parse_slash_commands_accepts_namespaced_and_hyphenated`. All 11 parser tests pass.

**#a012 — `agent-doc session-check <FILE>` subcommand** (`src/session_check.rs` new, `src/main.rs` wired)
New module reads the tail of `.agent-doc/logs/ops.log`, strips the `[epoch] ` prefix, and exits `1` when the last event is `preflight_diff_start`, `0` otherwise (including empty/missing log). Six unit tests cover timestamp stripping, missing log, empty log, terminal event, and the interrupted-cycle sentinel. Smoke test on the current doc confirmed both exit codes — before this cycle's write it returned `INTERRUPTED exit=1`, after it returned `ok — last event: snapshot_save ... exit=0`.

**#a011 — end-of-cycle write invariant warning** (`src/preflight.rs:380-392`)
Piggybacks on `session_check::last_ops_event`. Every preflight start checks the prior tail; if it's still `preflight_diff_start`, preflight emits `[preflight] WARNING: previous cycle ended at preflight_diff_start without a write — interrupted cycle detected` and logs `interrupted_cycle_detected` to ops.log. The `session-check` subcommand is the external tool for the same invariant.

**#a014 — session drift auto-resync** (`src/preflight.rs:394-398`, `maybe_auto_resync_on_drift`)
State file `.agent-doc/state/drift.count` tracks consecutive drift detections. First detection increments (count=1) and logs the deferral; second consecutive detection fires `resync::run(true, None)` and resets the counter. Drift-cleared preflights delete the counter file (one false positive is tolerated). The auto-fix path logs `auto_resync_on_drift consecutive=N` for audit.

**#a010 — `fire_post_write` hook in stream.rs** (`src/stream.rs:188-203`)
After `stream_loop` + final git commit, `run_stream` now fires both `fire_post_write(file, session_id, 1)` and `fire_doc_event(file, "post_write")`. Mirrors the pattern in `write.rs:1046-1049`. Supervisors and cross-session coordinators now get notified when a stream run completes, not only when append/template writes land.

**Test matrix (my changes only):**
- `session_check::tests` — 6 pass
- `stream::tests` — 15 pass
- `parse_slash_commands` — 11 pass
- `template::tests` — 56 pass
- `preflight::tests` — 29 pass, 1 pre-existing failure (`preflight_sweep_commits_other_tracked_docs`) verified to fail on stashed baseline, unrelated to this work.

❯ do #4sh0 #tw4a #8jzg #cce5
❯ ### Re: do #4sh0 #tw4a #8jzg #cce5 (HEAD)

All four shipped. Binary rebuilt and installed (still v0.32.4).

**#4sh0 — sync.rs planned-move logging** (`src/sync.rs`)
Added `sync_log()` calls at all tmux operation points in `repair_layout` and the stash rescue path: `stash_consolidate_action/result=join-pane`, `repair_action/result=swap-window`, `repair_action/result=move-window`, `rescue_attempt/action/result=swap-pane|join-pane`. Each log entry records pane IDs, window indices, session, and outcome (ok/err). All ClaimAction→SyncLayout swaps now have a trace in `/tmp/agent-doc-sync.log`.

**#tw4a — claim cwd → nearest git repo** (`src/claim.rs`, `src/sessions.rs`)
`claim::run` now resolves cwd via `git::resolve_pane_cwd(file)` (returns submodule root when file is in a submodule) before calling `sessions::register_with_pid_and_cwd()`. New helper `register_full_with_cwd` added to `sessions.rs`. Relative-path ambiguity and cross-repo git ops are fixed.

**#8jzg — `--isolate` flag** (`src/main.rs`, `src/claim.rs`)
New boolean `--isolate` flag on `agent-doc claim`. When set, `run_isolate(file)` resolves the nearest git root, then spawns `tmux new-window -c <git_root>` running `agent-doc start <file>`. CLAUDE.md, memory, and skills now load scoped to the submodule. Mid-session `cd` does not reload context — this is the only clean fix.

**#cce5 — model attribution in `### Re:` headers** (`src/preflight.rs`, bundled SKILL.md)
`PreflightOutput` now includes `agent_model: Option<String>` — populated from `ANTHROPIC_MODEL` env var (preferred) or frontmatter `model` field, both shortened via `short_model_name()` (strips `claude-` prefix). Bundled SKILL.md updated: when `preflight.agent_model` is set, append ` — <model>` to `### Re:` headers. Example: `### Re: do #4sh0 — sonnet-4-6`.

❯ did you add tests?
❯ ### Re: did you add tests? (HEAD)

No. The implementation commit added no tests for the 4 features:

- **#4sh0 (sync.rs logging)** — no tests. Sync.rs already has 17 tests but none cover the new log call paths.
- **#tw4a (claim cwd)** — no new tests. `register_with_pid_and_cwd` was added to `sessions.rs` without coverage.
- **#8jzg (--isolate flag)** — no tests. The `run_isolate` path goes untested.
- **#cce5 (agent_model)** — no tests. `resolve_agent_model()` added to `preflight.rs` with no unit test for the env-var path, frontmatter fallback, or `short_model_name` trimming.

The only test activity in that commit was a pre-existing clippy fix in `session_check.rs`. Want me to add tests for these now?
❯ yes

❯ I rebooted my machine. I opened JB Idea and a tmux session. From the JB Plugin, how do I start agent-doc? When I tried called `Claim for Tmux Pane`, I got a prompt "Claim failed (exit 1): error: pane %0 is in tmux session 'claude' but project session is '0'. Switch to session '0' and re-run, or use --force to override. Error: session mismatch: cross-session claim rejected".
### Re: yes (tests) — opus-4-6

Got it — I'll add tests for all 4 features. Plan:

- **#4sh0 (sync.rs logging)** — assert log output on the new call paths (capture via `tracing-test` or existing pattern in `sync.rs` tests).
- **#tw4a (claim cwd)** — unit test `register_with_pid_and_cwd` in `sessions.rs`: verify cwd is persisted and round-trips through the registry.
- **#8jzg (--isolate flag)** — integration test for `run_isolate` path (tempdir, verify isolation semantics).
- **#cce5 (agent_model)** — unit tests for `resolve_agent_model()`: env var precedence, frontmatter fallback, `None` case; and `short_model_name()` trimming (`claude-opus-4-6` → `opus-4-6`, `claude-sonnet-4-6` → `sonnet-4-6`, passthrough for already-short).

Starting on the JB blocker below first, then I'll implement these. Want me to batch into one commit or one per feature?

❯ ### Re: JB plugin claim session mismatch — opus-4-6 (HEAD)

**How to start agent-doc from the JB plugin:**

1. Open a session document (`.md` with `agent_doc_session` frontmatter) in IDEA.
2. Open a tmux pane in a terminal where you want agent-doc to run.
3. Invoke **Claim for Tmux Pane** from the plugin — this binds the active tmux pane to that document. Edits to the doc then route to that pane.
4. Agent-doc runs inside the pane (the `/agent-doc` skill handles each cycle).

**Your error — session mismatch:**

```
pane %0 is in tmux session 'claude' but project session is '0'
```

The project has a registered tmux session (`0`) from before reboot. Your new tmux session is named `claude`, so the claim is rejected as a cross-session claim (guard against commandeering panes from unrelated sessions).

**Three fixes, pick one:**

1. **Attach to the old session name** — if session `0` still exists post-reboot (unlikely after reboot), `tmux attach -t 0`.
2. **Force the claim** (quickest if you've verified `%0` is really yours):
   ```
   agent-doc claim tasks/agent-doc/agent-doc-bugs.md --force
   ```
   The plugin should expose a `--force` toggle; if not, run from a terminal. This overwrites the registered session to `claude`.
3. **Reset the project session registry** — clear the stale session binding so the next claim picks up `claude` cleanly:
   ```
   agent-doc sessions reset   # or: rm .agent-doc/sessions.json (check exact path)
   ```
   Then re-run `Claim for Tmux Pane`.

**Which I'd use:** option 2 (`--force`) — reboot invalidated session `0`, and forcing is the intended escape hatch. If the plugin's Claim action doesn't expose `--force`, that's itself a bug (file under agent-doc-bugs).

**Follow-up bug candidate:** after reboot, stale session bindings in the registry should probably be garbage-collected (detect that session `0` no longer exists in `tmux list-sessions` and auto-clear). Want me to add that as a pending item?
❯ I want the Claim action to expose `--force`. The FFI plugin should detect if the session does not exist and use `--force`.
❯ ### Re: expose --force + auto-detect stale session — opus-4-6 (HEAD)

Got it. Two-part change:

**1. CLI: `agent-doc claim --force` already exists** (the error message mentions it) — I'll verify and, if missing, add the flag to `claim` subcommand in `cli.rs` / `claim.rs`.

**2. FFI: auto-detect stale session and retry with force.** In `ffi.rs` `claim_for_pane` (or wherever the JB plugin calls into), on `session mismatch: cross-session claim rejected`:
- Parse the registered session name from the error (or read it from the registry directly).
- Call `tmux has-session -t <registered>` — if exit != 0, the session is stale.
- Retry the claim with `force=true`.
- If `tmux has-session` succeeds, return the original error (real cross-session conflict — user should decide).

**3. JB plugin UI:** also expose a manual `--force` toggle on the `Claim for Tmux Pane` action (checkbox in the dialog, or a separate `Force Claim for Tmux Pane` action) so the user can override even when auto-detect is wrong.

Added to pending. I'll implement after the tests from #4sh0/#tw4a/#8jzg/#cce5 unless you want this first — it's the active blocker, so I lean toward **this first, then tests**. Confirm?
❯ confirm
❯ ### Re: confirm — opus-4-6 (HEAD)

Implemented in `src/agent-doc/src/claim.rs`. Scope narrowed after reading the code.

**Finding:** `--force` already exists end-to-end (`main.rs:294`, `claim.rs:104`). No CLI plumbing changes needed — only the gate logic.

**Change:** CLI now auto-accepts the claim when the configured project session is **dead** — no `--force` required. Decision logic extracted into a pure function `cross_session_decision()` so it is unit-testable without a live tmux server.

**Decision table** (`claim.rs`):

| pane_session | configured | configured_alive | force | result |
|---|---|---|---|---|
| `X` | `X` | — | — | `Accept` |
| `A` | `B` | `false` | any | `AcceptStale` ← **new path** |
| `A` | `B` | `true` | `false` | `Reject` (unchanged) |
| `A` | `B` | `true` | `true` | `AcceptForce` (unchanged) |

Stale takes precedence over `--force` so the warning reflects reality (the configured session literally doesn't exist — nothing to "force over").

**Tests added** (5 new, all passing): `cross_session_accept_when_pane_matches_configured`, `cross_session_accept_stale_when_configured_dead`, `cross_session_accept_stale_takes_precedence_over_force`, `cross_session_reject_when_configured_alive_and_no_force`, `cross_session_accept_force_when_configured_alive_and_force`.

`cargo test --bins claim::` → 13 passed, 0 failed. `cargo clippy --bin agent-doc -- -D warnings` → clean on the modified file.

**Live proof:** While writing this response, the agent-doc write failed with a stale-pane error (`TMUX_PANE=%2` from a pre-reboot env, pane no longer exists, current pane is `%10` in session `claude`). I reclaimed with `--force` and it worked — the exact scenario this fix addresses. After the next release, the stale case will not need `--force` at all.

**FFI / JB plugin side — deliberately not touched yet.** Rationale:

1. ClaimAction.kt calls the CLI subprocess (thin wrapper), so it inherits the CLI fix automatically on release.
2. The CLI-level fix solves the post-reboot case without needing error-string parsing or retry logic in the plugin.
3. A manual `Force Claim for Tmux Pane` action in the JB plugin is still valuable as an escape hatch for real cross-session conflicts (both sessions alive). ~15 lines of Kotlin. Say the word.

**Next?**

1. Release agent-doc locally (`cargo build --release`) so you can test the fix in a live session.
2. Optional: add `Force Claim for Tmux Pane` action in the JB plugin.
3. Then: tests for #4sh0/#tw4a/#8jzg/#cce5 (still in pending).

What order?
❯ 1, 2, 3
❯ ### Re: 1, 2, 3 — opus-4-6 (HEAD)

On it. Starting step 1: `cargo build --release` in `src/agent-doc/`.
❯ Please readd the patchback. I rejected the external change while writing the following.
❯ When I try to auto-claim src/session-share/tasks/harness-research.md, there seems to be an issue with the recently added `cd src/session-share` for submodules change
```
/home/brian/.cargo/bin/agent-doc start src/session-share/tasks/harness-research.md
~/work/btakita/agent-loop/src/session-share main ⇡40 !4 ?1                                        agent-loop 11:59:38
❯ /home/brian/.cargo/bin/agent-doc start src/session-share/tasks/harness-research.md
Error: file not found: src/session-share/tasks/harness-research.md
```

**Step 1 done.** Release build: `agent-doc 0.32.4` at `src/agent-doc/target/release/agent-doc` (48s). `.bin/agent-doc` symlink points to it — live in tmux-spawned sessions.

Test in a live session before I move to step 2 (JB plugin `Force Claim` action)? Or should I proceed straight to step 2 while you test?
❯
❯ ### Re: auto-claim submodule path bug + step 1 — opus-4-6 (HEAD)

**Readding step 1 result:** `cargo build --release` in `src/agent-doc/` completed (48s). `agent-doc 0.32.4` now live at `src/agent-doc/target/release/agent-doc`; `.bin/agent-doc` symlink points to it.

**Diagnosis of the new bug (auto-claim on `src/session-share/tasks/harness-research.md`):**

Root cause is in `route.rs:736` + `route.rs:648`:

```rust
let cwd = crate::git::resolve_pane_cwd(file);   // → .../src/session-share
...
let start_cmd = format!("{} start {}", agent_doc_bin, file_path);
tmux.send_keys(&new_pane, &start_cmd)?;
```

`file_path` is passed through from the caller as the super-root-relative string (`src/session-share/tasks/harness-research.md`). The new pane is spawned with `cwd = <submodule root>`, then `agent-doc start src/session-share/tasks/harness-research.md` runs from inside the submodule — the super-root-relative path does not resolve, hence `Error: file not found`.

The cwd narrowing was added (correctly) in `#tw4a`/`#8jzg` so submodule docs spawn inside the submodule; the start command was not updated to match.

**Fix:** rewrite `file_path` to be relative to the spawned pane's `cwd` before composing `start_cmd`. Proposed logic:

```rust
let start_path = std::fs::canonicalize(file)
    .ok()
    .and_then(|abs| abs.strip_prefix(&cwd).ok().map(|p| p.to_path_buf()))
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_else(|| file_path.to_string());
let start_cmd = format!("{} start {}", agent_doc_bin, start_path);
```

Fallback behavior: if canonicalize fails or the file isn't under `cwd`, use the original `file_path` (matches current behavior — no regression for non-submodule docs).

**Impact:** affects every auto-started submodule pane. `provision_pane` path too (same `auto_start_in_session`).

**Want me to:**
1. Land the fix in `route.rs` now (tiny diff, plus a unit test if I can isolate `cwd` narrowing), rebuild locally, and then you re-try the auto-claim?
2. Then resume the step 2 → step 3 sequence we agreed on?

This bug is the active blocker for your other session, so I'd lean toward fixing it immediately.
❯ Yes. We were discussing the supervisor to fix this issue. Should we implement the supervisor managing the `cd` or have a quick fix before that?
❯ ### Re: supervisor vs quick fix — opus-4-6 (HEAD)

**Quick fix first.** The supervisor is not a prerequisite for this bug, and the quick fix stays correct even after the supervisor lands.

**Why the supervisor doesn't fix this on its own:**

The bug is not in *how* the pane is spawned — it's in the `start` argument string. `route.rs` narrows `cwd` to the submodule but still passes `file_path` as super-root-relative. No matter who spawns the child (current `tmux send-keys`, future supervisor PTY), `agent-doc start src/session-share/tasks/harness-research.md` run from `src/session-share/` will still fail. The supervisor would inherit the same broken invariant unless we also teach it "rewrite path relative to resolved cwd" — which *is* the quick fix.

**Layering:**

1. **Now (quick fix, ~10 LOC in `route.rs`):** canonicalize `file`, strip `cwd` prefix, fall back to original. Unit-testable without tmux. Unblocks your other session immediately.
2. **Later (#jg0d/#b486/#40ct/#vnp0/#6ae3/#zp02):** supervisor phase 1 wire-up. When `start.rs` moves to `PtySpawnConfig`, `PtySpawnConfig.args` receives the already-rewritten `start_path` — the quick fix becomes the canonical path-normalization step, not throwaway work.
3. **Supervisor smoke test (#f7d5)** can then assert the CWD-invariant + path-relative contract in one place.

**Risk of doing supervisor first:** 6 pending items, real restart/state/ipc/pty/env wiring, cross-platform concerns (Windows ConPTY stub in #jg0d). Your other session stays blocked the entire time. And you'd still need this rewrite anyway.

**Plan:**

1. Land `route.rs` rewrite + unit test (`cwd_narrowing_rewrites_start_path_for_submodule_doc`).
2. Also patch `provision_pane` path (same `auto_start_in_session` call site).
3. `cargo test --bins route::` + `cargo build --release`.
4. You retry auto-claim on `src/session-share/tasks/harness-research.md` in a live session.
5. Resume step 2 (JB `Force Claim` action) → step 3 (tests for #4sh0/#tw4a/#8jzg/#cce5).
6. Supervisor work stays queued as its own track.

Confirm and I'll ship the fix.
❯ confirm
❯ ### Re: confirm — opus-4-6 (HEAD)

**Landed and rebuilt.** Quick fix shipped in `src/agent-doc/src/route.rs`.

**Change:**

1. New pure helper `rewrite_start_path(file, cwd, original) -> String` — canonicalizes both `file` and `cwd`, strips the cwd prefix, falls back to `original` on any failure. Keeps non-submodule docs on the pre-existing code path.
2. Call site in `auto_start_in_session` (single funnel for both `auto_start` and `provision_pane`) now computes `start_path` once and uses it in both:
   - `start_cmd = format!("{} start {}", agent_doc_bin, start_path)` (the `send_keys` invocation that was failing)
   - `send_command(tmux, &new_pane, &start_path)` (the subsequent `/agent-doc <path>` slash command — same cwd, same bug class)

**Tests (4 new, all passing):**

- `rewrite_start_path_narrows_to_submodule_relative` — super-root-relative `src/sub/tasks/foo.md` + cwd `super/src/sub` → `tasks/foo.md`. The exact failure mode from your other session.
- `rewrite_start_path_no_op_when_file_under_cwd_with_same_prefix` — non-submodule case (cwd = super root) stays byte-identical.
- `rewrite_start_path_falls_back_when_canonicalize_fails` — ghost path → original string.
- `rewrite_start_path_falls_back_when_file_not_under_cwd` — real file outside cwd → original string.

Plus full `route::` suite: **43 passed, 0 failed**. `cargo clippy --bin agent-doc -- -D warnings`: clean.

**Release build:** `cargo build --release` → 47.5s. `agent-doc 0.32.4` at `src/agent-doc/target/release/agent-doc`. `.bin/agent-doc` symlink resolves and reports `agent-doc 0.32.4` — live for tmux-spawned sessions.

**Manual testing gate (per release runbook):** please retry auto-claim on `src/session-share/tasks/harness-research.md` in a live session. Expected: pane spawns inside `src/session-share/`, `agent-doc start tasks/harness-research.md` resolves, Claude boots, `/agent-doc tasks/harness-research.md` lands without "file not found". Report back and I'll proceed to step 2 (JB `Force Claim` action) → step 3 (tests for #4sh0/#tw4a/#8jzg/#cce5).

**Not touched (deliberate):**

- No supervisor wire-up. Stays queued in #jg0d/#b486/#40ct/#vnp0/#6ae3/#zp02/#f7d5 as its own track. When phase 1 lands, the same `rewrite_start_path` helper feeds `PtySpawnConfig.args` — this fix is forward-compatible, not throwaway.
- No `cargo install --path .` yet — waiting on your manual-test confirmation before I push to `~/.cargo/bin/`, then the publish gate for v0.32.5.
❯ When I switch editor panes where the bound tmux pane is closed...causing an auto start on harness-research.md...I get the following error
```
/home/brian/.cargo/bin/agent-doc start src/session-share/tasks/harness-research.md
~/work/btakita/agent-loop/src/session-share main ⇡40 !4 ?1                                        agent-loop 12:44:13
❯ /home/brian/.cargo/bin/agent-doc start src/session-share/tasks/harness-research.md
Error: file not found: src/session-share/tasks/harness-research.md
```

❯ Also, patchbacks to this file are resulting with a line containing `❯` right above the agent:boundary. Please fix this as well
❯ ### Re: still failing + ❯ line — opus-4-6 (HEAD)

**Root cause of the still-failing auto-start: I skipped `cargo install --path .` last cycle.**

Your error shows `/home/brian/.cargo/bin/agent-doc` — the stale binary. My previous cycle rebuilt `src/agent-doc/target/release/agent-doc` (0.32.4) and updated the `.bin/agent-doc` symlink, but the cargo-bin copy still contained the pre-fix code. That's the binary your sessions launch from (`~/.cargo/bin/` is on `$PATH`; `.bin/` is only on `$PATH` for tmux-spawned sessions inside the repo). So the old `auto_start_in_session` ran and passed the unrewritten super-root-relative path.

**Fix applied:**

```
cargo install --path src/agent-doc
→ Replacing /home/brian/.cargo/bin/agent-doc
→ Replaced package `agent-doc v0.32.4`
```

Verified: `/home/brian/.cargo/bin/agent-doc --version` → `agent-doc 0.32.4`. The `rewrite_start_path` helper is now live in both the cargo-bin and the repo `.bin` copies.

**Memory update:** I already treat `cargo install --path .` as mandatory post-publish; I also need to treat it as mandatory whenever I rebuild `src/agent-doc` locally and expect the fix to reach live sessions — `.bin/agent-doc` only helps tmux-spawned sessions inside the repo, not `~/.cargo/bin/`-launched ones. The release runbook (`src/agent-doc/CLAUDE.md` step 4) already lists this before the manual-testing gate; I elided it last cycle and will not again.

**Please retry the auto-claim** on `src/session-share/tasks/harness-research.md` — the cargo-bin fix is now in place. If it still reproduces, it's a distinct bug and I'll diagnose.

---

**On the `❯` line above the boundary:**

My fault — I've been ending exchange responses with a bare `❯` on its own line as a pseudo-prompt affordance. The binary then inserts `<!-- agent:boundary:... -->` right after my content, so the `❯` lands directly above the boundary marker on every cycle. Visual clutter, no semantic value.

**Fix (skill-side, no binary change):** saved a new auto-memory rule `feedback_no_trailing_prompt_glyph.md` — "Never end an agent-doc exchange response with a bare `❯` line" — and indexed it in `MEMORY.md` under the agent-doc Skill section. Going forward, responses end with the last real content line. If you want a submit affordance, you type `❯` yourself (you already do — `❯ confirm` and `❯ ` blocks are all user-authored).

This response is the first one following the new rule — note no trailing `❯` below.
❯ Make sure this skill fix will be released in an agent-doc install. Please confirm that the skill fixes are to both my local .claude and agent-doc/.claude/SKILL.md. Use runbooks where appropriate
❯ ### Re: release skill fix in agent-doc install — opus-4-6 (HEAD)

**Both SKILL.md files carry the rule and v0.32.5 is installed.**

**Where the rule was added (canonical source → release path):**

1. `src/agent-doc/SKILL.md:135` — bundled source. This file is compiled into the binary via `include_str!("../SKILL.md")` in `src/agent-doc/src/skill.rs:38`, so every future `cargo build` / `cargo install` / `cargo publish` picks it up automatically. New paragraph sits right after "Model attribution" and before "Streaming checkpoints":

   > **Never end a response with a bare `❯` line:**
   > Do NOT append `\n❯\n` … The binary inserts `<!-- agent:boundary:... -->` immediately after your content, so a trailing `❯` always lands as a dangling prompt-glyph row directly above the boundary marker on every cycle. `❯` is the user's submit indicator that they type themselves when they want to prompt back; the agent must not pre-seed it. End responses with the last real content line (or a period).

❯ Why do we need to specify this in the SKILL? I'm concerned about skill bloat.

2. `.claude/skills/agent-doc/SKILL.md:135` — project-local install that Claude Code actually reads at `/agent-doc` invocation. Synced via `agent-doc skill install`:
   ```
   Installed skill v0.32.5 → .claude/skills/agent-doc/SKILL.md
   ```
   `grep -n "bare .❯. line"` confirms both files carry the rule at line 135 with identical text (byte-equivalent to the `include_str!`-bundled source).

**Release prep completed (per `.agent/runbooks/prerelease.md` common steps 1-3, 8):**

- **Version bumped (step 2):** `Cargo.toml` 0.32.4 → **0.32.5**, `pyproject.toml` 0.32.4 → **0.32.5**, kept in sync.
- **VERSIONS.md updated (step 3):** new `## 0.32.5` entry with two bullets — the `rewrite_start_path` route.rs fix (with full test list + supervisor forward-compat note) and the SKILL.md "no trailing `❯`" rule (including why it ships bundled via `include_str!`).
- **Manual testing gate prep (step 8):** `cargo build --release` (47.8s) + `cargo install --path .` completed. `/home/brian/.cargo/bin/agent-doc --version` → `agent-doc 0.32.5`. Both the path fix and the SKILL.md rule are now live in the cargo-bin copy that your sessions launch from.
- **`make check`:** `cargo test --bin agent-doc route::` → 43 passed (my scoped target). Full `make check` has 45 tmux tests failing in parallel with `std::env::current_dir() → NotFound`, which is a pre-existing IsolatedTmux race (each test creates + deletes a tempdir; under high parallelism some races trip `current_dir`). Not caused by this change — same pattern fails on HEAD without my edits. Flagging as a known-flaky condition, not a regression gate.

**Not yet done (requires your confirmation — runbook step 8 is a hard gate):**

- No `cargo publish` (crates.io)
- No `maturin publish` (PyPI)
- No `git tag v0.32.5 && git push origin v0.32.5` (GitHub Release via CI)

**Two things to test in a live session:**

1. **Auto-claim / auto-start on the submodule doc:** retry `src/session-share/tasks/harness-research.md` — pane should spawn inside `src/session-share/`, `agent-doc start tasks/harness-research.md` should resolve, Claude should boot without "file not found".
2. **No phantom `❯` above boundary:** this very response is generated under the new rule (no trailing `❯` line below). Confirm the boundary marker sits directly under the last real content line in your editor.

If both verify, give the word and I'll run the publish gate (cargo + maturin + tag push) under `.agent/runbooks/prerelease.md` target sections.

**Auto-memory:** The project-local `.claude/projects/.../memory/feedback_no_trailing_prompt_glyph.md` entry stays as a belt-and-suspenders backup for my session state — it's user-scoped (not shipped), while the SKILL.md rule at `src/agent-doc/SKILL.md:135` is what every `agent-doc skill install` downstream gets.

❯ Do we need `.claude/projects/.../memory/feedback_no_trailing_prompt_glyph.md`? I don't want context bloat
❯ ### Re: skill bloat + memory bloat — opus-4-6 (HEAD)

**You're right on both. The elegant fix is in the binary, not docs.**

**Proposal: strip trailing `❯` lines in `agent-doc write` itself.**

- Location: `src/agent-doc/src/write.rs` (or wherever exchange content is normalized pre-append). Single regex/trim pass on inbound content: strip trailing `\n❯\s*\n?` before boundary insertion.
- Scope: exchange-component writes only (don't touch `agent:pending` or other components where `❯` could be legitimate content).
- Effect: enforced at the layer where it matters. Agents can't produce the bug even if the SKILL forgets to say so.

**Then both bloat concerns go away:**

1. **Remove the SKILL paragraph** (`src/agent-doc/SKILL.md:135` and the mirrored project-local copy). The binary enforces it; no need to teach every agent.
2. **Remove `feedback_no_trailing_prompt_glyph.md`** and its `MEMORY.md` index line. Same reason — binary-enforced, no local rule needed.

Net delta: ~+10 lines in `write.rs`, −15 lines across SKILL + memory. Behavior rule becomes a code invariant.

**If you want this:** say "do binary fix" and I'll implement under `.agent/runbooks/prerelease.md` (code → test → bump → publish → verify). If you'd rather keep the SKILL rule as a soft belt (in case the binary fix ever regresses), say "keep SKILL only" and I'll just delete the memory file.

❯ Could you do an audit of the agent-doc SKILL. Prune unnecessary content. Make sure we delegate to runbooks if we can.
❯ ### Re: SKILL audit + pruning plan — opus-4-6 (HEAD)

**Current:** 293 lines. **Target:** ~140 lines (~52% cut). Delegating to runbooks wherever the content isn't hot-path on every cycle.

**Audit by section:**

| # | Section | Lines | Action | Rationale |
|---|---------|-------|--------|-----------|
| 1 | Preflight JSON schema block (44–68) | 25 | **Cut** — keep 1-line field list | Agent parses real output; schema dupe is dead weight |
| 2 | `0c. Model tier gate` (94–112) | 19 | **Move** → `runbooks/model-tier-gate.md` | Rarely triggers; 2-line pointer suffices |
| 3 | `Streaming checkpoints` (138–154) | 17 | **Move** → `runbooks/streaming-checkpoints.md` | Procedural detail, not every-cycle |
| 4 | `Never end with ❯` (135–136) | 2 | **Delete** — becomes binary fix (prior exchange) | Code-enforced, not agent-enforced |
| 5 | `### 2a Append mode` (180–195) | 16 | **Collapse** to 3 lines | All modern docs are template mode; append is legacy |
| 6 | `### 2b Template mode` duplicates (216–226) | 11 | **Cut** — step 1 already covers `--stream`/baseline | Pure duplication |
| 7 | `Baseline file` paragraph (237–243) | 7 | **Cut** — covered in preflight section | Duplication |
| 8 | `Snapshot Storage` (273–278) | 6 | **Delete** | Internal detail; agent never touches snapshot files directly |
| 9 | `Document Format` frontmatter example (255–271) | 17 | **Move** → `runbooks/document-format.md` | Reference, not workflow |
| 10 | `### 3 Git integration` gutter explanation (247–253) | 7 | **Collapse** to: "After write, run `agent-doc commit <FILE>`." | Visual-gutter detail is operator-facing, not agent-facing |
| 11 | `Success Criteria` (288–293) | 6 | **Keep** — orients new invocations | Small and load-bearing |

**What stays verbatim (hot path on every cycle):**
- Invocation + claim/compact detection (§0)
- `agent-doc preflight` call + `no_changes` / `claims` / `baseline_file` handling
- `0b` slash-command dispatch (Skill tool semantics are non-obvious)
- `### Re:` header rule + model attribution (§1) — structural requirement for `(HEAD)` gutter
- `1b` pending ops 3-line summary + runbook pointer
- `--stream` write-back + `agent-doc commit` immediately after
- `IMPORTANT: Do NOT use Edit tool` (one line, load-bearing)

**New runbooks to create:**

1. `runbooks/model-tier-gate.md` — precedence chain, `required_tier` gate behavior, `model_switch` ack
2. `runbooks/streaming-checkpoints.md` — when/how to flush, baseline re-save pattern, code sample
3. `runbooks/document-format.md` — frontmatter fields, append vs template mode, `agent:name` component conventions

**Runbooks already present that absorb content:** `compact-exchange.md`, `pending-ops.md`, `transfer-extract.md`, `code-enforced-directives.md` (supports the "move ❯ rule to binary" decision).

**Execution plan (if you approve):**

1. Create the 3 new runbook files with extracted content.
2. Edit `src/agent-doc/SKILL.md` (bundled source — shipped via `include_str!`).
3. Run `agent-doc skill install` to sync `.claude/skills/agent-doc/SKILL.md`.
4. Verify: `wc -l` on both files, `grep` spot-checks for deleted anchors.
5. No version bump needed (SKILL content change only, no binary behavior change) — **unless** you also want the binary `❯`-stripping fix bundled, in which case it's a patch release under `.agent/runbooks/prerelease.md`.

**Question before I execute:** Should I bundle this with the binary `❯`-stripping fix (one release) or ship the SKILL prune now and the binary fix separately? Bundling is cleaner; separating is lower-risk.
❯ bundle this
### Re: bundle — executing — opus-4-6

Bundled. Starting execution now. Plan:

1. **Binary fix** — add trailing-`❯` strip to `agent-doc write` exchange writes (single normalization pass, exchange component only).
2. **Tests** — positive (trailing `❯` stripped) + negative (`❯` mid-content preserved, `❯` in pending not touched).
3. **SKILL prune** — edit `src/agent-doc/SKILL.md` per audit table; create 3 new runbooks; `agent-doc skill install` to sync.
4. **Memory cleanup** — delete `feedback_no_trailing_prompt_glyph.md` + its `MEMORY.md` line.
5. **Release** — follow `.agent/runbooks/prerelease.md` (VERSIONS.md, Cargo.toml, pyproject.toml sync, `make check`, cargo publish, maturin, `cargo install --path .`, tag push).
6. **Verify live** — confirm response from installed binary produces no trailing `❯`.

Will post progress after each completed step. Starting with exploration of `write.rs` and test layout.
❯ ### Re: bundled changes built + installed — manual gate — opus-4-6 (HEAD)

**Status:** all changes done, release build installed, **awaiting manual verification** before publishing.

**Built:**

1. **Binary fix** — `template::strip_trailing_caret_lines` + call sites in `apply_patches_with_overrides` (patch loop + unmatched routing + auto-created-exchange path). Only fires for exchange-bound content; `❯` in other components is preserved.
2. **Tests** — 8 new unit tests in `template.rs`, all passing; full `template::` suite: 64 passing. Lib suite: 292 passing. (45 pre-existing tmux-dependent `resync::`/`route::` test failures are unrelated — all panic with `Os { code: 2, kind: NotFound }` from tmux subprocess spawning; they fail identically on the pre-change baseline.)
3. **SKILL prune** — `src/agent-doc/SKILL.md`: **293 → 112 lines (~62% cut).**
4. **Runbooks created + bundled** via `include_str!` in `src/skill.rs::BUNDLED_RUNBOOKS`:
   - `runbooks/model-tier-gate.md`
   - `runbooks/streaming-checkpoints.md`
   - `runbooks/document-format.md`
   - `runbooks/code-enforced-directives.md` (promoted from project-local)
5. **Memory cleanup** — deleted `~/.claude/projects/-home-brian-work-btakita-agent-loop/memory/feedback_no_trailing_prompt_glyph.md` + removed its `MEMORY.md` index line.
6. **VERSIONS.md** — 0.32.5 entry updated: replaced the old "SKILL rule" bullet with two new bullets (binary fix + SKILL prune), preserved the existing `rewrite_start_path` bullet untouched.
7. **Release artifacts built** — `make build-release` produced `src/agent-doc/target/release/agent-doc`, `.bin/agent-doc` symlink is live, `cargo install --path .` replaced `~/.cargo/bin/agent-doc` with v0.32.5 (so tmux-spawned Claude sessions pick it up too).

**This response is the first write through the new binary.** If the binary fix is correct, the boundary marker below will sit directly under the last real content line — no phantom `❯` row. I'm deliberately ending this response with a period, not `❯`, because the point is that the agent *can* forget and the binary still catches it.

**Manual testing gate — please verify:**

1. **Boundary sanity:** After this write, open the doc in your editor and confirm `<!-- agent:boundary:... -->` sits directly under the last real line (no bare `❯` row above it).
2. **Mid-content `❯` preserved:** Scroll up — all the `❯ <user prompt>` lines from this session's history should still be intact. They're user content, not trailing prompt glyphs, and the binary must not have touched them.
3. **Optional stress test:** Manually edit this exchange to append a line containing just `❯` at the very end, save, then run `/agent-doc` again. The next preflight cycle should strip it server-side and no phantom row should appear.

**Not yet done (publish gate):**

- `cargo publish` to crates.io
- `maturin upload` to PyPI
- `git tag v0.32.5 && git push origin v0.32.5`
- `gh release create v0.32.5` (or let CI handle it after tag push)

Per the memory rule **"Manual testing gate before publish"** and the prerelease runbook §8, none of those fire until you confirm. Say `publish` (or `ship it` / `release`) and I'll run the publish gate.
❯ release
❯ ### Re: released — opus-4-6 (HEAD)

**agent-doc v0.32.5 shipped.**

| Target | Status |
|--------|--------|
| crates.io | Published (`agent-doc v0.32.5`) |
| PyPI | Uploaded (`agent_doc-0.32.5-py3-none-manylinux_2_28_x86_64.whl`) |
| GitHub Release | https://github.com/btakita/agent-doc/releases/tag/v0.32.5 |
| `~/.cargo/bin/agent-doc` | v0.32.5 (installed) |
| `.bin/agent-doc` symlink | v0.32.5 (release build) |

**What shipped:**

- Binary strips trailing bare `❯` from exchange writes — code-enforced invariant.
- SKILL.md: 293 → 112 lines. 4 new runbooks bundled (`model-tier-gate`, `streaming-checkpoints`, `document-format`, `code-enforced-directives`).
- Pending gated-state `[/]` + submodule auto-start fix also included (pre-existing in-flight work).

**Memory and context are clean:** `feedback_no_trailing_prompt_glyph.md` deleted and de-indexed. The rule is now enforced at the binary layer — agents can forget it and the behavior stays correct.

❯ In tasks/recruit/filevine/filevine-interview.md, I'm using Opus, but the header says haiku `### Re: docker container note — haiku (HEAD)` Why?
❯ ### Re: header says haiku but I'm on Opus — opus-4-6 (HEAD)

**Diagnosis.** `resolve_agent_model()` in `src/agent-doc/src/preflight.rs:230` resolves attribution in this priority order:

1. `ANTHROPIC_MODEL` env var
2. Frontmatter `model` field

Your shell has `ANTHROPIC_MODEL=haiku` exported (verified just now with `env | grep -i model`). Preflight reads that env var and stamps `agent_model: haiku` into its JSON, regardless of what Claude Code is actually running. The skill then appends `— haiku` to every `### Re:` header.

**Why Claude Code doesn't correct it.** Claude Code's `/model` selection is internal to the harness — it does **not** export `ANTHROPIC_MODEL` to the child shell. So the env var is stale (probably set in your zshrc or a prior session) and agent-doc trusts it blindly.

**Evidence in the current preflight for this doc:**

```
"effective_tier": "high",
"agent_model": "haiku"
```

Tier is `high` (Opus-class), but model attribution is `haiku`. Those two shouldn't disagree — that's the smell.

**Proposed fixes** (ranked):

1. **Remove `ANTHROPIC_MODEL=haiku` from your shell env.** Fastest fix. Check `~/.zshrc`, `~/.zshenv`, `~/.config/zsh/*`, and any direnv `.envrc`. Once unset, `resolve_agent_model` falls through to frontmatter (none set here) and returns `None`, so headers render as plain `### Re: topic` with no suffix.
2. **Make agent-doc consistency-check the env var against `effective_tier`.** If `ANTHROPIC_MODEL` maps to a tier below `effective_tier`, log a warning and ignore the env var. Prevents silent mislabeling if someone deliberately sets `ANTHROPIC_MODEL=haiku` for cost reasons but then Claude Code escalates to Opus.
3. **Drop env-var priority entirely.** Frontmatter-only. Harness-agnostic and predictable, but loses the "auto-detect current model" convenience.

Want me to (a) hunt down where `ANTHROPIC_MODEL=haiku` is being set in your shell config, (b) add the tier-consistency check to `resolve_agent_model`, or both? I'd lean toward doing (a) first since it's the immediate unblocker, then (b) so this class of bug can't silently recur.

Also worth adding to `tasks/agent-doc/agent-doc-bugs.md` as a tracked item — this is a real correctness bug, not just a cosmetic quirk, because the attribution is visible in archived exchanges and will mislead anyone reading the history.

❯ I'm getting an error with `Run Agent Doc` at src/boost-client/tasks/monsterrodholders.md. Need this fixed asap
```
Starting claude...
 ▐▛███▜▌   Claude Code v2.1.109
▝▜█████▛▘  Opus 4.6 · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop/src/boost-client

❯ Unknown command: /agent-doc

● Args from unknown skill: src/boost-client/tasks/monsterrodholders.md
```
❯ ### Re: /agent-doc unknown in boost-client submodule — opus-4-6 (HEAD)

**Root cause.** Claude Code only discovers skills from two places: `<cwd>/.claude/skills/` and `~/.claude/skills/`. It does **not** walk up the directory tree to find parent-repo skills.

When `agent-doc start` spawns `claude` at `src/agent-doc/src/start.rs:171`, it doesn't set `current_dir`, so Claude inherits the tmux pane's cwd. For a doc in `src/boost-client/tasks/monsterrodholders.md`, that's `~/work/btakita/agent-loop/src/boost-client`.

Verified:

- `src/boost-client/.claude/` — **does not exist**
- `~/.claude/skills/agent-doc/` — **does not exist**
- `agent-loop/.claude/skills/agent-doc/SKILL.md` — exists, but invisible to a Claude process rooted in the submodule

Claude has no `/agent-doc` skill to dispatch → "Unknown command".

**Immediate unblocker (30 seconds, no code change).** Symlink the skill into your user-level skills dir so it's available from any cwd:

```bash
mkdir -p ~/.claude/skills
ln -s ~/work/btakita/agent-loop/.claude/skills/agent-doc ~/.claude/skills/agent-doc
```

After that, `/agent-doc` works from any submodule shell — including fresh `agent-doc start` spawns inside `src/boost-client`, `src/presence`, `src/corky`, etc. Updates to the agent-loop copy flow through automatically because it's a symlink.

**Proper fix (agent-doc binary).** Two complementary changes:

1. **`agent-doc skill install --user`** — add a `--user` (or `--global`) flag that installs to `~/.claude/skills/agent-doc/` instead of `<cwd>/.claude/skills/agent-doc/`. Prefer symlink-to-source when the binary was invoked from a repo checkout; fall back to file copy for installed binaries. `skill::install()` at `src/agent-doc/src/skill.rs` currently hardcodes the `.claude/skills/` path relative to cwd.

2. **`agent-doc start` should chdir up to the outermost repo containing `.claude/skills/agent-doc/`.** Walk up from the document's parent until either (a) we find a `.claude/skills/agent-doc/SKILL.md`, or (b) we reach `$HOME`. If found, set the child `claude` process's `current_dir` to that ancestor. This keeps the skill discoverable even in nested submodules without requiring a user-level install. The tradeoff: Claude's cwd is no longer the submodule, so relative file ops from Claude target agent-loop root instead. Mitigation: pass the doc path as absolute, and Claude's `Read`/`Edit` tools already handle absolute paths fine.

**Recommendation.** Do #1 now (symlink + `--user` flag). Skip #2 — the chdir behavior is surprising and will break assumptions about cwd in `agent-doc start` sub-processes. The user-level install is the clean separation: "install once, works from anywhere."

Want me to:

- **(a)** Apply the symlink immediately so you're unblocked on boost-client?
- **(b)** Implement `agent-doc skill install --user` in the binary (small change — ~30 lines in `skill.rs` + a new flag in `main.rs`)?
- **(c)** Both — symlink now for the unblock, then PR the `--user` flag?

❯ Do option a first. The check back in

I'd do **(c)**. The symlink takes 5 seconds; the `--user` flag is the real fix so this doesn't bite anyone else (or future-you after an `rm -rf ~/.claude`). Flag it `[bug]` in this doc if you want it tracked.

**Note on `— opus-4-6` in this header:** preflight still reports `agent_model: haiku` (env var not yet unset), but I'm overriding to `opus-4-6` manually since we diagnosed the env-var bug last cycle and you confirmed you're on Opus. The attribution will auto-correct once `ANTHROPIC_MODEL` is cleared from your shell.
❯ ### Re: symlink applied — opus-4-6 (HEAD)

Done.

```
/home/brian/.claude/skills/agent-doc -> /home/brian/work/btakita/agent-loop/.claude/skills/agent-doc
```

**To verify the fix in boost-client:**

1. Go to the broken tmux pane (the one that showed `Unknown command: /agent-doc`).
2. Type `/clear` then Enter — Claude Code reloads its skill registry on a fresh conversation. (Restart the pane if `/clear` doesn't pick up the new skill; skill discovery happens at session start, not per-message.)
3. Try `/agent-doc src/boost-client/tasks/monsterrodholders.md` again.

If it still errors, the fallback is to fully quit that `claude` process in the pane and let `agent-doc start` respawn it — the new Claude instance will rescan `~/.claude/skills/` at startup and find the symlink.

Tell me what happens (works / still broken / new error) and I'll either queue up option (b) — the `agent-doc skill install --user` binary change — or dig into whatever's still wrong.
❯ It worked. do **`agent-doc start` should chdir up to the outermost repo containing `.claude/skills/agent-doc/`.**
<!-- patch:exchange -->
### Re: skill chdir-up — opus-4-6

Overriding my earlier recommendation against option #2. The symlink is a workaround; the real fix is making `agent-doc start` find the skill automatically.

**Implementation:** Modify `resolve_pane_cwd()` in `git.rs:279`. After finding the superproject root, walk up looking for `.claude/skills/agent-doc/SKILL.md`. If an ancestor contains it, use that ancestor as the cwd.

Current flow: superproject → narrow to submodule → return submodule root.
New flow: superproject → walk up for skill dir → if found, return that ancestor; else fall through to existing submodule/superproject logic.

Tradeoff accepted: Claude's cwd will be agent-loop root (not the submodule), but doc paths are absolute and Read/Edit handle them fine.

Adding to pending and implementing now.
<!-- agent:boundary:06bdb00c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#a013] SPEC.md breakup — flat `specs/*.md` mechanical extraction.
- [ ] [#a004] Drop Option B (skill-side submodule workaround) — A handles it deterministically; B redundant.
- [ ] [#jg0d] supervisor/resize.rs: SIGWINCH unix path via signal-hook; Windows ConPTY stub for phase 1
- [ ] [#b486] supervisor/state.rs: crash classifier, ring buffer, Healthy->Transient->Flapping->Halted state machine
- [ ] [#40ct] supervisor/ipc.rs: Unix-domain socket accept loop, reuse length-prefixed JSON frame from ipc_socket.rs
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
- [ ] [#2g09] FFI claim: detect missing tmux session and auto-retry with --force; expose --force on JB Claim action as manual override
- [ ] [#gbq8] `agent-doc start` chdir up to outermost repo containing `.claude/skills/agent-doc/` — modify resolve_pane_cwd() in git.rs to walk up from superproject root
<!-- /agent:pending -->
## Assistant
