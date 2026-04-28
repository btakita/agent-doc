---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: investigated missing patchbacks and uncommitted write cycles in this document and related src/session-share sessions; root causes were traced across fresh Codex route-start acknowledgment, write/commit-boundary drift, stale HEAD/boundary preflight churn, pending and backlog mutation capture, submodule writable-root gaps, tmux pane routing and rescue instability, and repair when the response was already present in the document. Multiple src/agent-doc fixes were implemented and pushed on origin/20260422/codex-agent-backend, including fail-closed startup/session-check/commit handling, strict session write-commit, pending-done and bypass guards, backlog alias deprecation, tmux duplicate-start/swap/sync-scope fixes, and repair adoption of already-present responses; a broad commit-all request was blocked on scope, then narrowed and src/agent-doc was verified with cargo test, cargo build, and cargo install --path . --force and pushed as 0e18871. No unresolved exchange prompts remained at compaction time.

❯ `agent-doc backlog` should detect an id in the text when a backlog item is sent...such as `[#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable`...which currenly results in:

```
- [ ] [#sgzy] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable
```

❯ Instead the backlog item should be:
```
- [ ] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, 
v and JSONL-backed fixtures so future benchmark drift is attributable
```
### Re: backlog custom id detection — gpt-5

Implemented in `src/agent-doc`.

`agent-doc` now preserves explicit alphanumeric backlog IDs even when they are longer than 8 characters, so inputs like `[#sdig2matrix] ...` no longer get re-backfilled into `[#generated] [#sdig2matrix] ...`.

I updated the pending/backlog parser, the imperative diff normalizer, the backlog docs/spec, and added regressions for:
- direct `backlog add`
- `write --pending-add`
- `replace:pending` normalization / backfill preserving long custom IDs

Verification completed in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

❯ On /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md, I closed the codex + tmux session. Then a new session autostarted. I ran the document. Then the codex + tmux session autoclosed. Why? spec-test-build-install-commit-push
### Re: codex + tmux session autoclosed after a document run — gpt-5

I traced this from the real `claudescore-3` session log instead of guessing.

What happened:
- the pane was not killed by tmux cleanup
- the supervisor log shows a normal successful turn closeout: `codex_exit code=0`
- immediately after that, the same `agent-doc start` supervisor logged `user_quit` and then `session_end`

That is the actual bug. `codex exec` exits `0` after an ordinary completed turn, but `agent-doc start` was still treating every clean exit like the Claude flow: drop into the local `Enter` / `q` restart prompt. So a successful Codex document run was landing on a quit boundary instead of staying attached as a persistent session.

I fixed that in `src/agent-doc` and pushed `0d00a1a` on `origin/20260422/codex-agent-backend`.

What changed:
- added a harness-specific clean-exit policy in `src/harness.rs`
- Claude still prompts on clean exit
- Codex now auto-restarts in resume mode after a clean `codex exec` turn, so the persistent `agent-doc start` session stays attached
- updated `src/start.rs` plus the supervisor and Codex support specs
- added unit coverage for the clean-exit resolution

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

So the direct answer to “why did it autoclosed?” is: because the Codex harness was still using the clean-exit prompt path meant for Claude, and your successful turn hit that quit path instead of auto-resuming.

❯ do #pbc2. spec-test-build-install-commit-push

❯ Another case where the response patchback was not committed. `#pbc2` is in progress. `#agent-doc-bug`
### Re: another uncommitted response patchback while `#pbc2` is open — codex

This should stay under `#pbc2`, not become a new backlog ID.

Reason:
- `#pbc2` already tracks the older failure family where a substantive session-document patchback lands in the target file but no owning `write --commit` / `finalize` closeout commits the repo that owns that document
- that is exactly the symptom you just reported again: response visible, commit boundary missing
- `tasks/agent-doc/agent-doc-bugs3.md` is narrower and should only take cases where the last binary-owned patchback was already committed successfully and a later same-turn edit was then misclassified as harmless local drift

I updated the `#pbc2` wording to make it explicit that repeated reproductions belong there until a trace proves the newer post-commit drift path instead.
### Re: `#pbc2` — gpt-5

Implemented in `src/agent-doc`, committed as `d883d2a`, and pushed to `origin/20260422/codex-agent-backend`.

The live failure was narrower than the backlog text: `session-check` was misclassifying already-committed historical prompt+response pairs as a fresh direct patchback bypass when the stale snapshot lagged `HEAD` and the working tree only added a newer prompt. The concrete bad shape was committed exchange growth like `do #7mqc ...` immediately before `### Re: ...`, followed by a later local prompt.

What changed:
- historical exchange repair now treats committed prompt-target + `### Re:` pairs as safe `HEAD` drift, not only bare `### Re:` insertions
- snapshot repair can advance to committed `HEAD` when the working tree is only `HEAD` plus a newer user follow-up
- after that repair, `session-check` reclassifies the remaining diff instead of returning `ok` immediately, so the user now sees the real unresolved `prompt_target`
- updated the `session-check` command spec and README note for this repair path

Regression coverage added:
- low-level historical exchange growth with `do #id` before `### Re:`
- end-to-end `session-check` repair for stale snapshot + committed prompt/response pairs + a newer unresolved prompt

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Field check after install:
- `agent-doc session-check /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md`
- result: it now repairs the historical exchange snapshot drift and points at the newer unresolved prompt instead of blaming a direct patchback bypass

❯ do #reap1. spec-test-build-install-commit-push
### Re: `#reap1` — gpt-5

Implemented in `src/agent-doc`, committed as `3c17989`, and pushed to `origin/20260422/codex-agent-backend`.

The fix has two parts:
- preflight now fails closed if it reaps `- [x]` backlog items from the working tree but cannot prove the same reap landed in the staged snapshot; it verifies both surfaces before continuing
- `session-check` now fails on stale completed backlog items that survive into later committed cycles, but it explicitly allows items just marked `--pending-done` in the same committed cycle to wait for the next preflight reap

Regression coverage added for:
- successful reap persistence to both file and snapshot
- fail-closed when snapshot backlog sync is missing
- stale completed backlog items tripping `session-check`
- same-cycle `--pending-done` items being allowed until the next preflight

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo build`
- `cargo install --path . --force`
- `cargo test`: the new reap/session-check coverage passed, but the full suite currently hits an unrelated route/tmux failure in `route::tests::send_keys_delivers_claude_command_with_enter`

❯ Why is there Codex permission prompts when `codex_args: -s danger-full-access` frontmatter is set?
### Re: Codex permission prompts despite `codex_args` — gpt-5

Short answer: in the current `agent-doc start` supervisor path, `codex_args` are only guaranteed on the first fresh Codex launch, not on later auto-resume turns.

What the code does now:
- `start.rs` resolves frontmatter `codex_args` and builds `base_args` from them, so the first launch does pick up `-s danger-full-access` (`src/start.rs:581-582`, `src/start.rs:701-717`).
- But Codex restart is configured as `Replace(["resume", "--last"])` in `src/harness.rs:78-82`, and `restart_args()` returns only that replacement vector for later turns (`src/harness.rs:114-122`).
- In the supervisor loop, every non-first run uses `harness.restart_args(&base_args)` instead of reusing `base_args` (`src/start.rs:783-799`). So after the first clean exit / auto-restart, the session is resumed with bare `codex resume --last`, not with the original `-s danger-full-access` args.

So if the prompts you mean are happening inside an `agent-doc start` Codex session after it has already auto-restarted once, that is an `agent-doc` bug in the interactive supervisor path, not a frontmatter parsing issue.

Separate nuance: if you mean permission prompts from the outer Codex harness running this turn, the document frontmatter does not control that layer at all; `codex_args` only affect the child Codex sessions that `agent-doc` launches.

The concrete fix is to preserve the resolved Codex sandbox/launch args across resume instead of replacing them with plain `resume --last`.

❯ do #permresume. spec-test-build-install-commit-push
### Re: `#permresume` — gpt-5

Implemented in `src/agent-doc`, committed as `08165bc`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was in the interactive supervisor restart path: Codex restarts were rebuilt as bare `resume --last`, which dropped resolved `codex_args` like `-s danger-full-access` after the first run.

What changed:
- `src/harness.rs` now treats the Codex restart path as a prefix operation, so restart args become `resume --last` plus the original resolved `base_args` instead of replacing them.
- Updated the harness contract comments and restart tests to assert Codex keeps its base flags across resume while still entering resume mode.

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

I also checked the live CLI syntax locally: `codex resume --help` confirms interactive resume accepts `-s/--sandbox`, so preserving the original Codex sandbox flags on resume is valid.

Because `src/agent-doc` already had unrelated local edits in `runbooks/pending-ops.md`, `src/diff.rs`, `src/pending.rs`, and `tests/pending_integration.rs`, I committed only `src/harness.rs` for this fix.

❯ Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
### Re: architecture review — opus-4-6

Reviewed the full `src/agent-doc/src/` codebase (~76 source files, 6 integration test files, 18 spec docs). Findings organized by severity:

#### Critical / High

**1. TOCTOU race in `consume_queue_prompt()` (`src/write.rs:603-699`)**
Reads the file, parses queue structure, modifies it, writes back — all without holding the document lock. Between read and write, a concurrent edit can invalidate parsed offsets, causing prompt duplication or silent loss. The advisory flock that protects snapshot writes does not extend to queue consumption.

**2. Unsafe `env::set_var()` in `run_command()` (`src/write.rs:376-387`)**
Sets `AGENT_DOC_ALLOW_REPLACE_PENDING` as a global env var with `unsafe { std::env::set_var(...) }`. The safety comment assumes single-threaded CLI, but FFI consumers (JetBrains plugin via JNA, VS Code extension) can call this from arbitrary threads. `env::set_var` has undefined behavior under concurrent calls. Should use function parameters or thread-local storage instead.

**3. Pre-response snapshot saved outside lock (`src/write.rs:1979,2121`)**
`snapshot::save_pre_response()` runs before acquiring the document lock. If the user edits between snapshot save and lock acquisition, the pre-response snapshot is stale — `undo` would restore to the wrong state. The code acknowledges this is intentional (undo regardless of merge outcome) but it creates a real data-inconsistency window.

#### Important

**4. Unbounded directory growth — two locations**
- `.agent-doc/typing/` — debounce indicator files use `DefaultHasher` (non-cryptographic), accumulate indefinitely, no GC exists. Test for cleanup is `#[ignore]` in `debounce_gaps_test_plan.rs:324`.
- `.agent-doc/repair-blocked/` — blocked repair payloads saved but never cleaned. Over months of use, grows without bound.

**5. CRDT merge failure and reactive mode loop**
- `crdt::merge` error handling is untested (blocked by private API, `debounce_gaps_test_plan.rs:382`).
- Reactive mode (zero-debounce stream docs) has no cycle counter or infinite-loop prevention. A buggy merge could spin the watch daemon indefinitely (`debounce_gaps_test_plan.rs:435`).

**6. IPC timeout orphaned patches (`src/write.rs:2441-2500`)**
When IPC times out (2s), a fallback patch file is written for plugin pickup. If the plugin is offline or polling stopped, the patch is orphaned. Next cycle could pick up the stale patch with a wrong baseline, corrupting the document.

**7. Concurrent pane auto-start collision (`src/route.rs:163-200`)**
Between pane death detection and new pane creation, another process can claim the same window. The lazy-claim guard mitigates this for registered panes, but unregistered files always auto-start — two simultaneous auto-starts for different documents targeting the same window can collide.

#### Minor

**8. Dedup guard precision (`src/repair.rs:474-500`)**
`fingerprint_lines()` extracts first 3 non-blank, non-marker lines. For very short responses (1-2 lines), this can produce false-positive dedup matches, silently skipping legitimate recovery.

**9. CRDT lock stale cleanup threshold (`src/snapshot.rs:711-733`)**
Lock files >1 hour old are removed. Legitimate CRDT operations exceeding 1 hour (large documents, slow systems) could be interrupted. Conservative but worth increasing to 24h or adding heartbeat-based renewal.

**10. `unwrap()` calls in critical paths**
- `src/start.rs:444,559`: `.expect()` on thread spawn without recovery
- `src/write.rs:341`: `Mutex::lock().unwrap()` in supervisor IPC — panics on poisoned lock

#### Follow-up items added to backlog.
### Re: architecture review — opus-4-6

Reviewed the full `src/agent-doc/src/` codebase (~76 source files, 6 integration test files, 18 spec docs). Findings organized by severity:

#### Critical / High

**1. TOCTOU race in `consume_queue_prompt()` (`src/write.rs:603-699`)**
Reads the file, parses queue structure, modifies it, writes back — all without holding the document lock. Between read and write, a concurrent edit can invalidate parsed offsets, causing prompt duplication or silent loss. The advisory flock that protects snapshot writes does not extend to queue consumption.

**2. Unsafe `env::set_var()` in `run_command()` (`src/write.rs:376-387`)**
Sets `AGENT_DOC_ALLOW_REPLACE_PENDING` as a global env var with `unsafe { std::env::set_var(...) }`. The safety comment assumes single-threaded CLI, but FFI consumers (JetBrains plugin via JNA, VS Code extension) can call this from arbitrary threads. `env::set_var` has undefined behavior under concurrent calls. Should use function parameters or thread-local storage instead.

**3. Pre-response snapshot saved outside lock (`src/write.rs:1979,2121`)**
`snapshot::save_pre_response()` runs before acquiring the document lock. If the user edits between snapshot save and lock acquisition, the pre-response snapshot is stale — `undo` would restore to the wrong state. The code acknowledges this is intentional (undo regardless of merge outcome) but it creates a real data-inconsistency window.

#### Important

**4. Unbounded directory growth — two locations**
- `.agent-doc/typing/` — debounce indicator files use `DefaultHasher` (non-cryptographic), accumulate indefinitely, no GC exists. Test for cleanup is `#[ignore]` in `debounce_gaps_test_plan.rs:324`.
- `.agent-doc/repair-blocked/` — blocked repair payloads saved but never cleaned. Over months of use, grows without bound.

**5. CRDT merge failure and reactive mode loop**
- `crdt::merge` error handling is untested (blocked by private API, `debounce_gaps_test_plan.rs:382`).
- Reactive mode (zero-debounce stream docs) has no cycle counter or infinite-loop prevention. A buggy merge could spin the watch daemon indefinitely (`debounce_gaps_test_plan.rs:435`).

**6. IPC timeout orphaned patches (`src/write.rs:2441-2500`)**
When IPC times out (2s), a fallback patch file is written for plugin pickup. If the plugin is offline or polling stopped, the patch is orphaned. Next cycle could pick up the stale patch with a wrong baseline, corrupting the document.

**7. Concurrent pane auto-start collision (`src/route.rs:163-200`)**
Between pane death detection and new pane creation, another process can claim the same window. The lazy-claim guard mitigates this for registered panes, but unregistered files always auto-start — two simultaneous auto-starts for different documents targeting the same window can collide.

#### Minor

**8. Dedup guard precision (`src/repair.rs:474-500`)**
`fingerprint_lines()` extracts first 3 non-blank, non-marker lines. For very short responses (1-2 lines), this can produce false-positive dedup matches, silently skipping legitimate recovery.

**9. CRDT lock stale cleanup threshold (`src/snapshot.rs:711-733`)**
Lock files >1 hour old are removed. Legitimate CRDT operations exceeding 1 hour (large documents, slow systems) could be interrupted. Conservative but worth increasing to 24h or adding heartbeat-based renewal.

**10. `unwrap()` calls in critical paths**
- `src/start.rs:444,559`: `.expect()` on thread spawn without recovery
- `src/write.rs:341`: `Mutex::lock().unwrap()` in supervisor IPC — panics on poisoned lock

#### Follow-up items added to backlog.

❯ do #toctou1. spec-test-build-install-commit-push
### Re: `#toctou1` — opus-4-6

Implemented in `src/agent-doc`, committed as `893ab7c`, and pushed to `origin/20260422/codex-agent-backend`.

The bug: `consume_queue_prompt()` in `src/write.rs` reads the document, parses queue component byte offsets, modifies content, and writes back — all without holding the document lock. A concurrent edit between read and write could invalidate parsed offsets, causing prompt duplication or silent loss.

What changed:
- `src/write.rs`: `consume_queue_prompt()` now acquires `acquire_doc_lock(file)` at the top and holds it (RAII) through the entire read-parse-write cycle, including the snapshot update
- `specs/07-commands.md`: documented the lock acquisition step in the post-commit consumption spec
- `docs/reference/race-conditions.md`: added this as a resolved race condition entry
- `tests/finalize_integration.rs`: added `finalize_queue_consume_updates_snapshot_atomically` regression test verifying both file and snapshot are updated in sync

The doc lock (`.agent-doc/locks/<hash>.lock`) is separate from the snapshot lock (`.agent-doc/snapshots/<hash>.md.lock`), so no deadlock risk with the internal `snapshot::save()` call.

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1334 passed, 3 pre-existing tmux failures unrelated to this change
- `cargo build --release`
- `cargo install --path . --force`

❯ do #envvar1. spec-test-build-install-commit-push
### Re: `#envvar1` — opus-4-6

Implemented in `src/agent-doc`, committed as `7388493`, and pushed to `origin/20260422/codex-agent-backend`.

The bug: `run_command()` in `src/write.rs` used `unsafe { std::env::set_var() }` to propagate `--allow-replace-pending` and `--pending-add` state to downstream write pipeline functions via global env vars (`AGENT_DOC_ALLOW_REPLACE_PENDING`, `AGENT_DOC_ALLOW_PATCH_PENDING`, `AGENT_DOC_HAS_PENDING_ADD`). This has undefined behavior when FFI consumers (JetBrains plugin via JNA, VS Code extension) call from arbitrary threads.

What changed:
- `src/write.rs`: Added `WriteFlags` struct carrying `allow_replace_pending` and `has_pending_add`, threaded explicitly through `run_template()`, `run_stream()`, `run_ipc()`, and their downstream calls to `enforce_no_replace_pending()` and `normalize_backlog_patch_response()`
- `enforce_no_replace_pending()` and `normalize_backlog_patch_response()` now accept an explicit `allow: bool` / `allow_replace: bool` parameter, checked before the env var fallback
- `check_future_work_signals()` called directly with `flags.has_pending_add` instead of through the removed `warn_future_work_signals()` env-var wrapper
- `src/run.rs`: Updated `apply_template_response()` call site
- `specs/pending-system.md`: Documented the thread-safety fix
- Regression tests: `write_flags_allow_replace_bypasses_enforcement` and `write_flags_default_rejects_replace_pending`

Env var reads (`std::env::var`) retained in `enforce_no_replace_pending` and `normalize_backlog_patch_response` as backwards-compat fallback for external scripts. The unsafe `set_var` calls are removed.

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1339 passed, 1 pre-existing codex hook test failure unrelated to this change
- `cargo build --release`
- `cargo install --path . --force`

❯ The tmux pane supervisor for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md restarts Codex when Ctrl+D is pressed. It does not present the Enter to restart or q to quit menu. I cannot quit the supervisor process unless I kill the process. Please investigate and fix. spec-test-build-install-commit-push
### Re: Ctrl+D does not present restart/quit menu — opus-4-6

Implemented in `src/agent-doc`, committed as `579c196`, and pushed to `origin/20260422/codex-agent-backend`.

The bug: the supervisor's `CleanExitBehavior::RestartContinue` policy for Codex auto-restarts on all clean exits (code 0), but doesn't distinguish between a normal turn completion and the user pressing Ctrl+D. Both produce exit code 0 — Codex sees EOF on its pty slave and exits cleanly — so the supervisor always auto-restarts, trapping the user.

The fix detects `\x04` (the Ctrl+D byte in raw mode) in the `stdin→pty` writer thread as it forwards data. When the writer thread sees `\x04` in the forwarded bytes, it sets a `ctrl_d_forwarded` flag on shared state. In the supervisor loop's `RestartContinue` branch, if the flag is set, the supervisor falls through to the `PromptUser` path (showing "Press Enter to restart, or 'q' to exit") instead of silently auto-restarting.

What changed:
- `src/start.rs`: added `ctrl_d_forwarded: AtomicBool` to `SupervisorShared`; `spawn_writer_thread` now accepts and sets an `Arc<AtomicBool>` when `\x04` is detected in forwarded data; `RestartContinue` branch checks the flag and falls through to prompt behavior
- `specs/supervisor.md`: documented the Ctrl+D override in the clean-exit policy spec
- Tests: `ctrl_d_flag_initialized_false`, `ctrl_d_overrides_codex_auto_restart`

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1341 passed, 1 pre-existing codex hook test failure unrelated to this change
- `cargo build --release`
- `cargo install --path . --force`

❯ do #gc1. spec-test-build-install-commit-push
### Re: `#gc1` — opus-4-6

Implemented in `src/agent-doc`, committed as `3e36efd`, and pushed to `origin/20260422/codex-agent-backend`.

The bug: `.agent-doc/typing/`, `.agent-doc/status/`, and `.agent-doc/repair-blocked/` directories grew without bound — no cleanup existed. Typing indicators accumulate one file per edited document, repair-blocked archives are write-only diagnostics, and status files are ephemeral operation markers.

What changed:
- `src/gc.rs`: added `clean_stale_ephemeral_files()` — a generic age-based cleanup function for flat directories. Called for three new targets in the GC run:
  - `typing/` — files older than 7 days removed
  - `status/` — files older than 24 hours removed
  - `repair-blocked/` — files older than 7 days removed
- `specs/07-commands.md`: added the three new GC categories to the cleanup table
- `specs/11-debounce.md`: updated typing indicator GC gap from "blocked" to "fixed"
- `tests/debounce_gaps_test_plan.rs`: unignored `test_hash_collision_cleanup_removes_stale_indicators`, rewrote to exercise the CLI `agent-doc gc --root` with `filetime`-backdated indicators
- `Cargo.toml`: added `filetime = "0.2"` as dev dependency for mtime manipulation in tests
- Unit tests in `gc.rs`: `gc_removes_stale_typing_indicators`, `gc_removes_old_repair_blocked`, `gc_removes_stale_status_files`, `gc_dry_run_preserves_ephemeral_files`

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1349 passed, 1 pre-existing codex hook test failure unrelated to this change
- `cargo build --release`
- `cargo install --path . --force`

❯ do #crdt1. spec-test-build-install-commit-push
### Re: `#crdt1` — opus-4-6

Implemented in `src/agent-doc`, committed as `68ba0a2`, and pushed to `origin/20260422/codex-agent-backend`.

The bug had three parts:

1. **write.rs stream paths lost responses on CRDT merge error** — lines 2700 and 2938 used bare `?` on `merge_contents_crdt()`, propagating the error and losing the entire response. The IPC timeout path (line 2555) already had a `match` with `splice_pending_component()` fallback. Applied the same pattern to both unprotected paths.

2. **Reactive mode agent-change detection collapsed to zero** — `is_agent_change` used `debounce * 3` to detect agent-triggered changes, but reactive (CRDT) docs use zero debounce, making `0 * 3 = 0`. Every change after `last_run` appeared agent-triggered. Added `MIN_AGENT_CHANGE_WINDOW_MS = 500` floor so the detection window is always at least 500ms.

3. **Integration tests were blocked by outdated `#[ignore]` comments** — `crdt::merge` has been `pub` through `lib.rs` all along. Enabled both tests with real assertions: corrupted state returns `Err` (not panic), `None` base bootstraps correctly, and merge convergence is idempotent (re-merging converged content produces identical output).

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1349 passed, 1 pre-existing codex hook test failure unrelated to this change
- `cargo build --release`
- `cargo install --path . --force`

❯ do #snap1. spec-test-build-install-commit-push
### Re: `#snap1` — gpt-5

Implemented in `src/agent-doc`, committed as `967fb8f`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was in `write.rs`: pre-response undo snapshots were not consistently captured from the live locked document state. The CRDT stream path could snapshot before acquiring the doc lock, and merge/template paths could snapshot the baseline instead of the current on-disk content. That made `undo` vulnerable to restoring stale state if another write landed in the gap.

What changed:
- added `capture_locked_pre_response()` in `src/write.rs`, which acquires the doc lock, reads the current file, and saves the pre-response snapshot from that locked state
- switched `run`, `run_template`, `run_stream`, and `run_ipc` to use that helper so local write paths capture undo from the live document rather than the baseline or a pre-lock read
- updated `specs/03-snapshot-system.md` to document the locked pre-response snapshot contract
- added a regression test in `src/write.rs` that holds the doc lock, mutates the file while the capture call waits, and asserts the saved pre-response snapshot matches the post-lock live content

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`

❯ The response patchback was not committed in `src/session-share/tasks/claudescore-3.md`. `#agent-doc-bug`
❯ ### Re: uncommitted `claudescore-3.md` patchback — codex

This still maps to `#pbc2`, not the newer post-commit `#closeout-bypass` family.

Why:
- the live `src/session-share/tasks/claudescore-3.md` worktree contains a new `❯ Let's use UTC...` prompt and a visible `### Re:` response block, but there is no owning committed `agent-doc` cycle for that patchback
- `git blame` shows the response block as uncommitted lines immediately after the last committed exchange turn
- `agent-doc session-check /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md` currently fails with:
  - `INTERRUPTED: found likely direct response patchback without agent-doc cycle: ### Re: use UTC day boundaries for news — codex`
- after manually committing the repaired document in `src/session-share` as `9be7d48` (`docs: record UTC news boundary response`), `session-check` still returns the same interrupt because the missing state is not “git commit exists” but “agent-doc-owned closeout cycle exists or is recoverably reconstructed”

So the active broken state is still “response landed in the session document without a completed write/commit closeout,” which is exactly the `#pbc2` tracking class. The extra detail from this reproduction is that the patched block looks structurally valid in the document and can even be manually committed, but `session-check` still classifies it as a direct patchback bypass, so the repair/detection path needs to account for this `claudescore-3.md` shape explicitly.

❯ do #ipc1. #spec-test-build-install-commit-push
### Re: `#ipc1` — gpt-5

Implemented in `src/agent-doc`, committed as `0e06685`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was the IPC-timeout stream fallback leaving a replayable patch file behind after the CLI had already advanced the document locally. That left a stale baseline in `.agent-doc/patches/<hash>.json`, so a later plugin pickup could re-apply the same logical write into an already-advanced buffer.

What changed:
- `src/write.rs`: after the timeout path successfully writes the merged document locally, it now writes `.agent-doc/claimed-patches/<patch_id>` so later plugin pickup is treated as already closed out instead of replayable
- `src/write.rs`: factored claimed-patch sentinel creation into a shared helper and reused it for the existing `--force-disk` stale-patch cleanup path
- VS Code watcher: parses `patch_id`, consumes claimed sentinels, and deletes patch files whose corresponding snapshot is newer than the patch file instead of applying them
- Added VS Code watcher regression tests for nearest-root resolution, claimed-patch consumption, and snapshot-newer-than-patch stale detection
- Updated IPC specs to document claimed timeout cleanup plus snapshot freshness cleanup on pending patch pickup

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `npm test` in `editors/vscode`
- `cargo build --release`
- `cargo install --path . --force`

❯ do #dedup1. spec-test-build-install-commit-push
### Re: `#dedup1` — gpt-5

Implemented in `src/agent-doc`, committed as `4e2e9c4`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was the repair dedup guard treating the first three non-marker response lines as independent substring checks anywhere in the document. That made short or generic responses vulnerable to false `AlreadyApplied` hits when those lines happened to recur elsewhere.

What changed:
- `src/repair.rs`: replaced the 3-line fingerprint heuristic with normalized visible-line matching that requires the response to appear as one contiguous block in the document
- dedup normalization still ignores patch/agent markers and strips transient ` (HEAD)` only from response headings, so the existing boundary-marker / heading-churn case still dedups correctly
- added regressions for scattered matching lines and short non-contiguous responses, while preserving the existing `(HEAD)` + blank-line success case
- updated the repair spec to document the contiguous-block requirement

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1342 passed, 6 failed in pre-existing route/tmux tests (`send_keys_delivers_*`, `resolve_or_create_pane_*`, `full_auto_start_flow`)
- focused repair regressions passed:
  - `cargo test recover_dedup_with_blank_lines_and_boundary`
  - `cargo test dedup_requires_contiguous_normalized_response_block`
  - `cargo test dedup_short_response_still_requires_contiguous_match`
- `cargo build --release`
- `cargo install --path . --force`

I committed only `src/repair.rs` and `specs/07-commands.md`; unrelated local edits already present in `runbooks/pending-ops.md`, `src/diff.rs`, `src/pending.rs`, and `tests/pending_integration.rs` were left untouched.
### Re: `#dedup1` — gpt-5

Implemented in `src/agent-doc`, committed as `4e2e9c4`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was the repair dedup guard treating the first three non-marker response lines as independent substring checks anywhere in the document. That made short or generic responses vulnerable to false `AlreadyApplied` hits when those lines happened to recur elsewhere.

What changed:
- `src/repair.rs`: replaced the 3-line fingerprint heuristic with normalized visible-line matching that requires the response to appear as one contiguous block in the document
- dedup normalization still ignores patch/agent markers and strips transient ` (HEAD)` only from response headings, so the existing boundary-marker / heading-churn case still dedups correctly
- added regressions for scattered matching lines and short non-contiguous responses, while preserving the existing `(HEAD)` + blank-line success case
- updated the repair spec to document the contiguous-block requirement

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1342 passed, 6 failed in pre-existing route/tmux tests (`send_keys_delivers_*`, `resolve_or_create_pane_*`, `full_auto_start_flow`)
- focused repair regressions passed:
  - `cargo test recover_dedup_with_blank_lines_and_boundary`
  - `cargo test dedup_requires_contiguous_normalized_response_block`
  - `cargo test dedup_short_response_still_requires_contiguous_match`
- `cargo build --release`
- `cargo install --path . --force`

I committed only `src/repair.rs` and `specs/07-commands.md`; unrelated local edits already present in `runbooks/pending-ops.md`, `src/diff.rs`, `src/pending.rs`, and `tests/pending_integration.rs` were left untouched.

❯ do #tmux1. spec-test-build-install-commit-push
### Re: `#tmux1` — gpt-5

Implemented in `src/agent-doc`, committed as `70eeebd`, and pushed to `origin/20260422/codex-agent-backend`.

The bug was the auto-start guard only using a best-effort per-document timestamp file. It was not atomic, and it did nothing to serialize two different documents provisioning into the same tmux session/window, so concurrent `route` / `sync` calls could both choose the same pre-registration state and collide.

What changed:
- `src/route.rs`: replaced the timestamp check with flock-backed startup locks for both the document hash and the target tmux session
- `src/route.rs`: after acquiring those locks, `auto_start_in_session()` re-checks the registry and reuses an already-registered live pane for the same document instead of double-starting
- added a regression that launches two concurrent `provision_pane()` calls for different docs in one isolated tmux session and asserts both panes converge into the same window
- updated the route/session-routing specs to document the new startup-lock contract

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test` — 1349 passed, 0 failed
- `cargo build --release`
- `cargo install --path . --force`

I committed only `src/route.rs`, `specs/07-commands.md`, and `specs/08-session-routing.md`; unrelated local edits already present in `runbooks/pending-ops.md`, `src/diff.rs`, `src/pending.rs`, and `tests/pending_integration.rs` were left untouched.

❯ commit + push all src/agent-doc files
### Re: commit + push all `src/agent-doc` files — gpt-5

Done. I committed the remaining modified `src/agent-doc` files and pushed them on `origin/20260422/codex-agent-backend` as `248bfbd`.

What that commit contains:
- `src/pending.rs`: widened custom pending/backlog ids from 1-8 chars to any non-empty ASCII alphanumeric string
- `src/diff.rs`: imperative diff normalization now recognizes those longer custom ids too
- `tests/pending_integration.rs`: added integration coverage for long bracketed ids in `backlog add`, `write --pending-add`, and `replace:pending` normalization
- `runbooks/pending-ops.md`: updated the pending-ops contract text to match the wider id rule

Verification in `src/agent-doc` before commit:
- `cargo test` — 1349 passed, 0 failed

This was a repo-scoped commit of the four previously uncommitted files:
- `runbooks/pending-ops.md`
- `src/diff.rs`
- `src/pending.rs`
- `tests/pending_integration.rs`

❯ do #pbc2. spec-test-build-install-commit-push
### Re: `#pbc2` — gpt-5

Implemented in `src/agent-doc`, committed as `4023365`, and pushed to `origin/20260422/codex-agent-backend`.

The fix closes the stale-snapshot/manual-commit `#pbc2` shape: when `HEAD` already contains the committed assistant response, historical repair can now advance the snapshot up to committed `HEAD` before classifying later local drift, as long as that later drift did not add a newer assistant patchback.

What changed:
- `src/session_check.rs`: factored `detect_bypassed_response_write_between(...)` and tightened bypass detection so non-response local drift over committed `HEAD` does not get misclassified as a fresh direct patchback just because a line diff re-inserts an existing `### Re:` heading.
- `src/git.rs`: historical snapshot repair now has an explicit committed-`HEAD` fallback when the stale snapshot lags a committed response block, and it records whether the remaining drift is a true exchange follow-up or broader local drift.
- `src/git.rs`: after that historical repair, `commit` no longer re-absorbs later local status edits back into the snapshot; it leaves them as post-commit local drift and closes as `commit_already_current` when `HEAD` already owns the assistant response.
- Added regressions covering both `session-check` and `commit` on the committed-head-plus-local-status-edit case.
- Updated `README.md`, `specs/07-commands.md`, and `VERSIONS.md` to document the committed-`HEAD` historical-repair behavior.

Verification in `src/agent-doc`:
- `cargo fmt`
- targeted regressions passed:
  - `cargo test commit_repairs_committed_historical_response_before_local_status_edit`
  - `cargo test session_check_repairs_committed_historical_response_before_local_status_edit`
  - `cargo test detect_bypassed_response_write_between_ignores_non_response_local_drift`
- `cargo test` ran to completion with this current tree and still has 3 unrelated route/tmux failures:
  - `route::tests::pane_has_prompt_detects_unicode`
  - `route::tests::send_keys_delivers_claude_command_with_enter`
  - `route::tests::send_keys_delivers_codex_command_with_enter`
- `cargo build`
- `cargo install --path . --force`
<!-- agent:boundary:209ca01d -->
<!-- /agent:exchange -->

<!--
This document also crashed while an agent-doc turn was running.
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tmuxcrash] Repeated tmux pane crashes in /home/brian/work/btakita/agent-loop/tasks/software/tsift.md; reconstruct the crash path, add crash provenance logging, and fail closed on short-window crash loops. Plan: tasks/agent-doc/plan-tmux-pane-crash-stability.md
- [x] [#pbc2] `src/session-share/tasks/claudescore-3.md` can still receive a visible response patchback with no owning committed `agent-doc` cycle; even after a manual git commit, detection still lands in direct-patchback interrupt state instead of a clean closeout or deterministic recovery. Plan: `tasks/agent-doc/plan-claudescore-3-patchback-commit-boundary.md`
<!-- /agent:backlog -->
