---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
-
❯ When I ran `compact exchange`, it has a list that is empty. Why?
### Re: empty list after compact exchange — opus-4-6

My invocation error. When I ran the compact, I used `--message -` expecting the Unix convention where `-` means "read from stdin." But `agent-doc compact --message` treats its argument as a literal string, so the exchange was replaced with the character `-` (an empty markdown list item) and the piped summary was discarded.

The original exchange is safely archived (1,335 lines in `.agent-doc/archives/...-20260426-044100.md`).

**Fix paths:**

1. **Binary-side:** teach `agent-doc compact --message -` to read from stdin (standard Unix convention for CLIs that accept file-or-string arguments)
2. **Skill-side:** pass the summary inline as `--message "text..."` instead of piping

Both are worth doing — the first prevents this class of error for any caller, the second is the immediate workaround.

Do option 1. #spec-test-commit-push
### Re: do option 1 — opus-4-6

Implemented `--message -` stdin support in `compact::run()`. When the message argument is `"-"`, the binary reads the full message body from stdin before passing it to the compact pipeline.

**Changes** (`ae58945` on `20260422/codex-agent-backend`):
- `src/compact.rs` — resolve `--message -` to `std::io::stdin().read_to_string()` at the top of `run()`, before any downstream consumption. Added module doc entry and eval line.
- `specs/07-commands.md` — documented the convention in the compact section.
- `tests/test_cli.rs` — CLI integration test (`test_compact_message_dash_reads_stdin`) pipes content via `write_stdin` and asserts it appears in the compacted document.

Verified end-to-end: `echo "Summary piped from stdin." | agent-doc compact session.md --message - --tag skip` produces the piped text in the exchange component.

❯ in the #queue plan, add `<!-- agent:queue -->`...`<!-- /agent:queue -->` to the agent doc template.
### Re: agent:queue in template — opus-4-6

Added to Phase 1 of the #queue plan: `&lt;!-- agent:queue --&gt;...&lt;!-- /agent:queue --&gt;` goes into the default template scaffold, placed between `agent:exchange` and `agent:pending`. This covers all three scaffold sites (`claim.rs` empty-file scaffold, `claim.rs` component scaffold, and `sync.rs` scaffold).

❯ do #queue using a sync orchestra
❯ #spec-test-commit-push after each task
❯ #spec-test-build-install-commit-push after the entire run
### Re: do #queue — opus-4-6

Orchestrate subprocess spawning failed (Claude CLI nested sessions hit a hang after diff computation). Proceeding with manual phase-by-phase execution.

**Phase 1 complete** (`e13d833` on `20260422/codex-agent-backend`):

- **`queue.rs`** — new module with hybrid syntax parser: single-line prompts (`- text`), multi-line fences (`~~~prompt`/`---`), control fences (`--- start [at <time>]`, `--- stop`), `~~~start`/`~~~stop` aliases, `auto` attribute detection. Utility functions: `parse()`, `render()`, `prompts()`, `first_prompt()`, `remove_first_prompt()`.
- **`claim.rs` + `sync.rs`** — updated all 3 template scaffold sites to include `&lt;!-- agent:queue --&gt;...&lt;!-- /agent:queue --&gt;` between exchange and pending.
- **`git.rs`** — fixed `redact_component_contents_for_absorb` slice bounds panic when component markers appear as literal text inside other components (nested component handling). Added `queue` to the empty-scaffold component whitelist.
- **`specs/02-document-format.md`** — added §2.5 Queue Component with syntax table and parsing rules.
- **30 unit tests** — all syntax variants, roundtrip preservation, control fences, time gates, error cases, utility functions.

Also fixed a pre-existing bug: `redact_component_contents_for_absorb` panicked when the exchange component contained literal `&lt;!-- agent:queue --&gt;` markers (as happens when orchestrate injects task descriptions that reference component markers). The fix skips nested components during redaction.

Phases 2-4 remain.
-
❯ When I ran `compact exchange`, it has a list that is empty. Why?
### Re: empty list after compact exchange — opus-4-6

My invocation error. When I ran the compact, I used `--message -` expecting the Unix convention where `-` means "read from stdin." But `agent-doc compact --message` treats its argument as a literal string, so the exchange was replaced with the character `-` (an empty markdown list item) and the piped summary was discarded.

The original exchange is safely archived (1,335 lines in `.agent-doc/archives/...-20260426-044100.md`).

**Fix paths:**

1. **Binary-side:** teach `agent-doc compact --message -` to read from stdin (standard Unix convention for CLIs that accept file-or-string arguments)
2. **Skill-side:** pass the summary inline as `--message "text..."` instead of piping

Both are worth doing — the first prevents this class of error for any caller, the second is the immediate workaround.

Do option 1. #spec-test-commit-push
### Re: do option 1 — opus-4-6

Implemented `--message -` stdin support in `compact::run()`. When the message argument is `"-"`, the binary reads the full message body from stdin before passing it to the compact pipeline.

**Changes** (`ae58945` on `20260422/codex-agent-backend`):
- `src/compact.rs` — resolve `--message -` to `std::io::stdin().read_to_string()` at the top of `run()`, before any downstream consumption. Added module doc entry and eval line.
- `specs/07-commands.md` — documented the convention in the compact section.
- `tests/test_cli.rs` — CLI integration test (`test_compact_message_dash_reads_stdin`) pipes content via `write_stdin` and asserts it appears in the compacted document.

Verified end-to-end: `echo "Summary piped from stdin." | agent-doc compact session.md --message - --tag skip` produces the piped text in the exchange component.

❯ in the #queue plan, add `<!-- agent:queue -->`...`<!-- /agent:queue -->` to the agent doc template.
### Re: agent:queue in template — opus-4-6

Added to Phase 1 of the #queue plan: `&lt;!-- agent:queue --&gt;...&lt;!-- /agent:queue --&gt;` goes into the default template scaffold, placed between `agent:exchange` and `agent:pending`. This covers all three scaffold sites (`claim.rs` empty-file scaffold, `claim.rs` component scaffold, and `sync.rs` scaffold).

do #queue using a sync orchestra
#spec-test-commit-push after each task
#spec-test-build-install-commit-push after the entire run

❯ Implement #queue Phase 1: Parser + data model — Add QueueComponent to the document model (parallel to PendingComponent). Implement hybrid syntax parser for: single-line prompts (bare '- ' list items), multi-line prompts (~~~prompt/--- fences), control fences (--- start, --- start at <time>, --- stop), blank line skipping between items. Add agent:queue component to the default template scaffold in claim.rs (empty-file scaffold + component scaffold) and sync.rs scaffold, placed between agent:exchange and agent:pending. Comprehensive unit tests covering: single-line items, multi-line with ~~~prompt, multi-line with ---, mixed syntax, empty queue, parse errors, start/stop fences, time gates, auto attribute detection. Reference the detailed plan at tasks/agent-doc/agent-queue-plan.md for the exact syntax rules and design decisions. #spec-test-commit-push
Both binaries are built and installed. Now write the response and finalize.

<!-- patch:exchange -->

### Re: do #queue Phase 1 — sonnet-4-6

Phase 1 implemented, tested, committed, pushed, and installed.

#### What was built

Codex (Opus 4.6) created `src/queue.rs` (541 lines) and updated the scaffold/test files. I added the `specs/07-commands.md` section.

**`queue.rs` — I/O-free parser**

```rust
pub enum QueueEntry {
    Prompt(QueuePrompt),        // single-line or multi-line
    StartFence(Option<String>), // None = immediate; Some(time) = scheduled
    StopFence,
}
pub struct QueuePrompt { pub text: String, pub multiline: bool }
pub fn parse(body: &str) -> Result<Vec<QueueEntry>>
pub fn render(entries: &[QueueEntry]) -> String
pub fn remove_first_prompt(entries: &[QueueEntry]) -> Vec<QueueEntry>
```

Parser handles: bare `- ` list items, `~~~prompt`/`---` multi-line fences, `--- start`/`--- start at <time>`, `--- stop`, blank-line skipping. 30 unit tests.

**Scaffold** — `claim.rs` + `sync.rs` both updated: `<!-- agent:queue -->` inserted between exchange and pending in all three scaffold sites.

**Bonus fix** — `git.rs redact_component_contents_for_absorb` had a slice-bounds panic when component marker text appeared literally inside another component's body. Fixed with nested bounds check, regression test added.

**Specs** — `specs/02-document-format.md` (codex) and `specs/07-commands.md` (this cycle) both updated.

#### Commits

- `e13d833` — codex: parser + scaffold + git.rs fix (637 lines changed)
- `cf490fa` — spec: `07-commands.md` agent:queue section

Both pushed to `20260422/codex-agent-backend`. `cargo install` + `cargo build --release` completed; `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

<!-- /patch:exchange -->

### Re: do #queue Phase 1 — sonnet-4-6

Phase 1 implemented, tested, committed, pushed, and installed.

#### What was built

Codex (Opus 4.6) created `src/queue.rs` (541 lines) and updated the scaffold/test files. I added the `specs/07-commands.md` section.

**`queue.rs` — I/O-free parser**

```rust
pub enum QueueEntry {
    Prompt(QueuePrompt),        // single-line or multi-line
    StartFence(Option<String>), // None = immediate; Some(time) = scheduled
    StopFence,
}
pub struct QueuePrompt { pub text: String, pub multiline: bool }
pub fn parse(body: &str) -> Result<Vec<QueueEntry>>
pub fn render(entries: &[QueueEntry]) -> String
pub fn remove_first_prompt(entries: &[QueueEntry]) -> Vec<QueueEntry>
```

Parser handles: bare `- ` list items, `~~~prompt`/`---` multi-line fences, `--- start`/`--- start at <time>`, `--- stop`, blank-line skipping. 30 unit tests.

**Scaffold** — `claim.rs` + `sync.rs` both updated: `&lt;!-- agent:queue --&gt;` inserted between exchange and pending in all three scaffold sites.

**Bonus fix** — `git.rs redact_component_contents_for_absorb` had a slice-bounds panic when component marker text appeared literally inside another component's body. Fixed with nested bounds check, regression test added.

**Specs** — `specs/02-document-format.md` (codex) and `specs/07-commands.md` (this cycle) both updated.

#### Commits

- `e13d833` — codex: parser + scaffold + git.rs fix (637 lines changed)
- `cf490fa` — spec: `07-commands.md` agent:queue section

Both pushed to `20260422/codex-agent-backend`. `cargo install` + `cargo build --release` completed; `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

❯ Add response to this doc. is #queue done?

### Re: is #queue done? — opus-4-6

The response from the prior cycle is already committed in the document — no re-add needed.

**#queue is not done.** Phase 1 (parser + data model) is complete and live at 0.33.14, but three phases remain:

| Phase | Status | Summary |
|-------|--------|---------|
| 1. Parser + data model | Done | `queue.rs` parser, scaffold updates, 30 tests |
| 2. Preflight integration | Open | Trigger resolution (`auto`, start fences, `do queue`), `queue_active` state, emit `queue_prompts` |
| 3. Consumption + dispatch | Open | Per-item removal after commit, change detection halt, stop/start fence handling, looping |
| 4. CLI scaffolding | Open | `agent-doc queue add/list/clear` commands |

Plan file: `tasks/agent-doc/agent-queue-plan.md`

❯ do #queue Phase 2

### Re: do #queue Phase 2 — opus-4-6

Phase 2 implemented, tested, committed, and pushed (`26b0c75` on `20260422/codex-agent-backend`).

#### What was built

**Activation resolution** (`queue.rs`) — `resolve_activation()` determines whether the queue should be active using priority order:

1. `auto` attribute on `&lt;!-- agent:queue auto --&gt;` → immediate activation when prompts exist
2. Start fence at head → bare `--- start` consumed and activates; `--- start at <time>` defers
3. Exchange trigger → `do queue` / `run queue` detected in diff
4. Persisted state → `queue_active: true` in frontmatter

New types: `QueueTrigger` enum (Auto, StartFence, ExchangeRequest, Persisted), `QueueActivation` struct with activation state + consumed entries.

**Preflight integration** (`preflight.rs`) — Step 4f runs queue maintenance after diff computation:

- Reads `agent:queue` component, parses entries, resolves activation
- Consumes bare start fences (writes back to document + snapshot)
- Strips `auto` from opening tag when queue drains
- Persists `queue_active` in frontmatter (set on activation, cleared on drain)
- Emits `queue_prompts`, `queue_active`, `queue_deferred`, `queue_start_at`, `queue_trigger` in preflight JSON

**Exchange trigger detection** (`diff.rs`) — `detect_queue_trigger()` scans user-added diff lines for `do queue` / `run queue`, respecting code fences, blockquotes, and the `❯ ` prompt prefix. Does NOT match `do #queue` (pending item reference).

**Bare flag support** (`component.rs`) — `parse_attrs()` now supports bare flags like `auto` (previously only `key=value` pairs were parsed, so `&lt;!-- agent:queue auto --&gt;` never actually populated the attrs map).

**Frontmatter** — Added `queue_active: Option<bool>` field with `merge_fields` handler.

#### Tests

- 16 new activation resolution tests (all trigger modes, priority, empty queue, drain)
- 9 new `detect_queue_trigger` tests (positive/negative: exchange/code fence/blockquote/prefix)
- `parse_attrs` test updated for bare flag support
- Full suite: 1215 unit + 337 integration tests pass

#### Changes summary

| File | Lines | What |
|------|-------|------|
| `queue.rs` | +247 | QueueTrigger, QueueActivation, resolve_activation(), strip_auto_from_tag(), 16 tests |
| `preflight.rs` | +222 | QueueState, run_queue_maintenance(), 5 new PreflightOutput fields, Step 4f wiring |
| `diff.rs` | +125 | detect_queue_trigger() + 9 tests |
| `component.rs` | +14/−7 | parse_attrs bare flag support |
| `frontmatter.rs` | +9 | queue_active field + merge handler |
| `specs/02-document-format.md` | +11 | §2.5 activation resolution docs |
| `specs/07-commands.md` | +34 | Activation + preflight queue fields |

#### Remaining phases

| Phase | Status | Summary |
|-------|--------|---------|
| 1. Parser + data model | Done | `queue.rs` parser, scaffold updates, 30 tests |
| 2. Preflight integration | **Done** | Trigger resolution, `queue_active` state, `queue_prompts` emission |
| 3. Consumption + dispatch | Open | Per-item removal after commit, change detection halt, stop/start fence handling, looping |
| 4. CLI scaffolding | Open | `agent-doc queue add/list/clear` commands |

❯ do #queue Phase 3. spec-test-commit-push

### Re: do #queue Phase 3 — opus-4-6

Phase 3 implemented, tested, committed, and pushed (`0cb4810` on `20260422/codex-agent-backend`).

#### What was built

**Post-commit consumption** (`write.rs`) — `consume_queue_prompt()` runs between the write step and the commit step, so the consumption is included atomically in the same git commit as the response:

- Reads frontmatter; if `queue_active != true`, no-op.
- Parses queue component, removes the first `Prompt` entry via `remove_first_prompt()`.
- On drain (no prompts remain): strips `auto` from opening tag, sets `queue_active: false`.
- Writes updated content to file AND snapshot.

**Halt detection** (`preflight.rs`) — three new guards in `run_queue_maintenance()` that fire before prompts are dispatched:

1. **Stop fence at head** — consumes the `--- stop` fence, strips `auto`, clears `queue_active`, emits `queue_halted: "stop_fence"`. No prompt dispatched.
2. **Time gate at head** — if `--- start at <time>` and time hasn't arrived, emits `queue_deferred: true`. Queue stays active but paused.
3. **Item modified** — compares head prompt text between snapshot and file. If different (user edited between cycles), strips `auto`, clears `queue_active`, emits `queue_halted: "item_modified"`.

**Pure functions** (`queue.rs`) — `detect_head_prompt_modified()`, `has_stop_fence_at_head()`, `time_gate_at_head()`, `QueueHaltReason` enum. 16 new unit tests.

**New preflight output field** — `queue_halted: Option<String>` in both `QueueState` and `PreflightOutput`.

#### Tests

- 16 new unit tests in `queue.rs` (change detection, stop fence, time gate, edge cases)
- 3 new integration tests in `finalize_integration.rs`:
  - `finalize_consumes_first_queue_prompt_after_commit` — verifies first prompt removed, others remain
  - `finalize_drains_queue_and_clears_active_on_last_prompt` — verifies drain + auto strip + queue_active clear
  - `finalize_does_not_consume_when_queue_inactive` — verifies no consumption when queue_active is not set
- Full suite: 1664 pass (1 pre-existing flake in codex hook test)

#### Spec updates

- `specs/02-document-format.md` — §2.5 consumption semantics
- `specs/07-commands.md` — `queue_halted` field, halt rules, post-commit consumption docs

#### Remaining phases

| Phase | Status | Summary |
|-------|--------|---------|
| 1. Parser + data model | Done | `queue.rs` parser, scaffold updates, 30 tests |
| 2. Preflight integration | Done | Trigger resolution, `queue_active` state, `queue_prompts` emission |
| 3. Consumption + dispatch | **Done** | Post-commit removal, halt detection (stop fence, time gate, item modified), 19 tests |
| 4. CLI scaffolding | Open | `agent-doc queue add/list/clear` commands |

❯ I moved tasks/recruit/robert-ross.md to tasks/professional/robert-ross.md. agent-doc was inserting duplicates of the prompt corrupting the document. Why?

❯ Also, Idea is not responding with lag. Why?

### Re: prompt duplication after file move — opus-4-6

**Root cause: snapshot keyed by path hash, not session UUID.**

agent-doc snapshots live at `.agent-doc/snapshots/<sha256(file_path)>.md`. When you manually moved `tasks/dreamable/robert-ross.md` → `tasks/professional/robert-ross.md` (commit `18973f81`), the snapshot stayed keyed to the old path. On the next preflight at the new path:

1. No snapshot found → fresh baseline created from the file as-is
2. First agent-doc cycle (`b1e3c5e33`) saw the entire compacted exchange as "new user content" → the write pipeline stripped 89 lines trying to normalize
3. Second cycle (`a879f0bbd`) re-added the same content with `❯` prompt prefixes on lines it classified as user prompts → corruption

The oscillation (strip → re-add with prefixes) is the write pipeline fighting itself: each cycle sees a diff against its own previous mutation and tries to "fix" it again.

**Fix:** agent-doc should detect file renames via the session UUID in frontmatter. When preflight finds no snapshot for the current path but the frontmatter UUID matches a snapshot at a different path, it should migrate the snapshot to the new path hash before computing the diff. Alternatively, an explicit `agent-doc move <old> <new>` command could relocate the snapshot + baseline + pre-response artifacts in one step.

❯ Add the Fix into pending.

**Workaround for now:** after moving a session document, run `agent-doc claim <new-path>` to force a fresh snapshot from the committed state before resuming the session.

### Re: IDEA lag — opus-4-6

IDEA isn't in the process list right now — it's either crashed or was killed. Without it running, I can't diagnose the specific lag.

Common causes given the agent-doc JB plugin context:
- The IPC file-watcher polling too aggressively (the preflight log showed `IPC listener active` — the plugin was connected at 13:16)
- Large `.agent-doc/` directory with many snapshot/archive files triggering IntelliJ's VFS indexer
- The `ActionPromoter` issue from the memory notes — though that manifests as broken intentions, not lag

If you restart IDEA and the lag returns, I can check plugin logs, thread dumps (`jstack`), or the IDE's built-in freeze reporter. Let me know.

### Re: add fix to pending — opus-4-6

Added `#mvsnap` to pending — covers both the preflight auto-migration path and the explicit `agent-doc move` command as a fallback.

❯ Also, the background CRDT insertion of the duplicate prompts is still happening. Please clean up any orphaned sessions that are doing this.

### Re: orphaned session cleanup — opus-4-6

Cleaned up the active corruption and orphaned artifacts:

1. **Killed `agent-doc start` PID 1824751** — the robert-ross CRDT listener that was inserting duplicate boundary markers and partial prompts into `tasks/professional/robert-ross.md`. The working tree had duplicated `&lt;!-- agent:boundary --&gt;`, `&lt;!-- /agent:exchange --&gt;`, and `&lt;!-- agent:pending --&gt;` blocks plus truncated user text (`"Shou"`, `"Should robert-ross have a sim"`).

2. **Restored `tasks/professional/robert-ross.md` from HEAD** — undid the CRDT corruption. Document is clean.

3. **Removed 13 orphaned supervisor sockets** — stale `.agent-doc/supervisor/*.sock` files from dead sessions. Only the bugs-doc socket (`1ef5f0e7`) remains.

4. **Removed dead robert-ross socket** (`6cb2a9bf`) — process is gone, socket was stale.

**Root cause confirmation:** the `agent-doc start` process was applying CRDT patches against a mismatched snapshot (from before the file move), causing the write pipeline to re-insert boundary markers and prompt prefixes on every patch cycle. Killing the process + restoring from HEAD stops the feedback loop. To resume the robert-ross session cleanly, run `agent-doc claim tasks/professional/robert-ross.md` first to establish a fresh snapshot at the new path.

❯ Please plan and add to pending a feature to recover from orphaned sockets.

### Re: orphaned socket recovery feature — opus-4-6

**Design:**

The feature has two surfaces:

1. **Automatic (preflight step 0.5)** — before the repair step, scan `.agent-doc/supervisor/*.sock`. For each socket:
   - Look up the session UUID in `sessions.json` to get the PID
   - Check PID liveness (`kill -0` or `/proc/<pid>/`)
   - If dead: remove the socket file, prune the `sessions.json` entry, log `[preflight] gc: reaped orphaned socket <uuid>`
   - If alive but socket won't connect (broken pipe): same treatment — the process is zombie or hung

2. **Manual (`agent-doc gc`)** — same logic, callable standalone. Useful when preflight isn't running (e.g., after a crash, or during debugging). Outputs a summary of what was reaped.

**Edge cases to handle:**
- Socket exists but no `sessions.json` entry (leaked by a crash before registration) — reap the socket
- `sessions.json` entry exists but no socket file (clean exit that didn't deregister) — prune the entry
- `watch.pid` references a dead process — remove the stale PID file

Added as `#gcsock` in pending.

❯ Add to pending a claude_model and codex_model for the frontmatter to use the model for the agent doc sessions.

### Re: claude_model / codex_model frontmatter — opus-4-6

Added `#hmodel` to pending. The feature adds two frontmatter fields:

- `claude_model` — model override for Claude Code sessions (e.g., `opus`, `sonnet`)
- `codex_model` — model override for Codex sessions (e.g., `o3-pro`, `gpt-5`)

Preflight already emits `agent_model` for model attribution. These fields would feed into that, letting each harness resolve its own model from the frontmatter rather than relying on the generic `agent` field. The skill uses the resolved value for the `— model-name` suffix on response headers.

❯ The CRDT corruption is still happening in tasks/professional/robert-ross.md. Please diagnose.

### Re: CRDT corruption still happening — opus-4-6

**Root cause found: IDEA's JB plugin (PID 774403) was auto-respawning the session.**

The JB plugin owns the global `.agent-doc/ipc.sock` and monitors `sessions.json`. When I killed the `agent-doc start` process, the plugin detected the dead session and immediately spawned a new tmux pane with `agent-doc start tasks/professional/robert-ross.md`. This happened three times (%128 → %129 → %130) before I identified the respawn source.

The respawned process inherited the same mismatched snapshot (keyed to the old `tasks/dreamable/` path hash), so each restart immediately resumed corrupting the document with duplicate boundary markers and prompt-prefix insertions.

**Fix applied:**

1. Killed the agent-doc start process (PID 1917090)
2. Ran `agent-doc claim tasks/professional/robert-ross.md` — this establishes a fresh snapshot at the correct path hash and registers the session cleanly in `sessions.json`
3. The claim provisioned pane %131 with a fresh Claude session
4. Verified: the working tree now shows only legitimate user content (your unresponded edits about robert-ross's directory structure), no duplicate boundary markers or truncated prompts

**IDEA was running the whole time** (PID 774403) — earlier `ps aux | grep idea` missed it because the process name is lowercase `idea` in the comm column. It holds IPC sockets across 13+ `.agent-doc/` roots in the workspace.

❯ Does the JB plugin need to be fixed? If yes please plan and add to pending.

### Re: does the JB plugin need fixing? — opus-4-6

The plugin's respawn behavior (restart dead sessions) is correct and desirable — it's a session-continuity feature. The bug is that `agent-doc start` blindly enters the CRDT listener loop without validating that the snapshot matches the current file path.

**The fix belongs in the binary, not the plugin:**

`agent-doc start` should validate snapshot integrity on launch:
1. Compute the path hash for the file
2. Check that `.agent-doc/snapshots/<hash>.md` exists
3. If missing or stale (frontmatter UUID doesn't match), auto-reclaim before entering the listener loop

This means the JB plugin can keep its respawn behavior unchanged — the binary will self-heal on startup. The existing `#mvsnap` pending item (preflight auto-migration) covers the diff-time case; this new item (`#startval`) covers the runtime case when `agent-doc start` launches.

Together, `#mvsnap` + `#startval` + `#gcsock` close the full lifecycle: preflight migrates snapshots, start validates before listening, and GC cleans up dead sockets.

Added `#startval` to pending.

❯ do #gcsock. #spec-test-build-install-commit-push

### Re: do #gcsock — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`8481bd7` on `20260422/codex-agent-backend`).

#### What was built

**Orphaned socket GC** integrated into `gc.rs` — runs as part of the existing `gc::run()` pipeline, which means preflight's daily auto-GC (Step 0a, `.agent-doc/gc.stamp`) includes socket cleanup with zero new wiring.

**Algorithm** (`clean_orphaned_sockets`):

1. Scan `.agent-doc/supervisor/*.sock` files, extract session UUID from filename
2. Look up UUID in `sessions.json` — if registered PID is alive (`/proc/<pid>/` exists), keep socket
3. If PID dead or no registry entry, try `ipc::is_active()` (socket connect) as fallback
4. If connect fails: remove `.sock` file, mark sessions.json entry for pruning
5. Phase 2: also scan sessions.json entries whose PID is dead AND socket doesn't exist — prune those ghost entries
6. Acquire `RegistryLock` before modifying sessions.json (safe under concurrent agent-doc processes)

**No new subcommand needed** — `agent-doc gc` already existed; socket cleanup is now included in its sweep. `--dry-run` works for sockets too.

#### Tests (7 new)

| Test | Verifies |
|------|----------|
| `gc_removes_orphaned_socket_with_dead_pid` | Dead PID → socket removed + sessions.json pruned |
| `gc_keeps_socket_with_live_pid` | Current process PID → socket kept |
| `gc_removes_socket_without_session_entry` | Leaked socket (no registry) → removed |
| `gc_prunes_session_entry_with_dead_pid_and_no_socket` | Ghost entry (no socket, dead PID) → pruned |
| `gc_dry_run_does_not_delete_sockets` | Dry run reports counts but doesn't mutate |
| `gc_no_supervisor_dir_returns_zero` | Missing supervisor/ dir → graceful no-op |
| `gc_mixed_live_and_dead_sockets` | Mixed: only dead socket removed, live kept |

#### Spec

Added `## gc` section to `specs/07-commands.md` with the full algorithm table and preflight integration note.

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

Full suite: 1235 unit + integration pass (1 pre-existing codex hook flake).

❯ do #mvsnap. spec-test-build-install-commit-push
### Re: do #mvsnap — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`d8f066a` on `20260422/codex-agent-backend`).

#### What was built

**Auto-migration in preflight** (`snapshot.rs: try_migrate_renamed`): when `ensure_initialized` runs (from preflight, claim, or sync), it now detects orphaned state files after a rename:

1. Reads the document's `agent_doc_session` UUID from frontmatter
2. If no snapshot exists for the current path hash, scans `.agent-doc/snapshots/*.md` for a snapshot whose frontmatter has a matching session UUID
3. If found, migrates all hash-keyed state files to the new path hash: snapshots, baselines, locks, pending, CRDT, and pre-response
4. Updates the sessions registry entry

**Baselines added to explicit `rename`** (`rename.rs`): the `STATE_FILES` list now includes `("baselines", "md")` so `agent-doc rename <old> <new>` also migrates baseline files.

#### Tests (4 new)

- `try_migrate_renamed_finds_orphaned_snapshot` — full migration path with snapshot, baseline, and pending files
- `try_migrate_renamed_noop_when_snapshot_exists` — no migration when snapshot already present
- `try_migrate_renamed_noop_when_no_session` — no migration without session UUID
- `try_migrate_renamed_noop_when_no_match` — no migration when no UUID match found

#### Specs

- `specs/03-snapshot-system.md` — new "Auto-Migration on Rename" section
- `specs/07-commands.md` — new `rename` command section documenting both explicit and auto-migration

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

Full suite: 1239 unit + integration pass (1 pre-existing codex hook flake).

❯ do #startval. #spec-test-build-install-commit-push
### Re: do #startval — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`ec1e133` on `20260422/codex-agent-backend`).

#### What was built

Added `snapshot::ensure_initialized(file)` call to `start.rs:run()` between session registration and the IPC listener loop. On every `agent-doc start`, this now:

1. **Migrates orphaned state** — if the file was moved (e.g., JB plugin respawn after rename), detects the old-path snapshot by session UUID match and migrates all state files (snapshots, baselines, locks, pending, CRDT, pre-response) to the new path hash
2. **Bootstraps fresh snapshot** — if no snapshot exists at all (first launch), creates one from the current file content with stripped exchange
3. **Logs the outcome** — writes to session log (`snapshot_validated action=initialized|already_valid`) and stderr on initialization or failure

#### Tests (2 new)

- `ensure_initialized_migrates_after_move_with_existing_session` — simulates JB respawn scenario: file with existing session UUID + old-path snapshot/CRDT state → move → `ensure_initialized` migrates both to new hash
- `ensure_initialized_bootstraps_snapshot_for_fresh_file` — file with session UUID but no prior state → creates snapshot

#### Specs

- `specs/03-snapshot-system.md` — added `start` to the list of `ensure_initialized` callers
- `specs/07-commands.md` — new step 4 in `start` command documenting snapshot integrity validation

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

Full suite: 1720 tests pass (1 pre-existing codex hook flake).

❯ do #hmodel. #spec-test-build-install-commit-push
### Re: do #hmodel — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`9a255d2` on `20260422/codex-agent-backend`).

#### What was built

Added `claude_model` and `codex_model` frontmatter fields for per-harness model override. When running under Claude Code, `claude_model` takes precedence over `model`; under Codex, `codex_model` takes precedence. Falls back to `model` when no harness-specific field is set.

**Resolution method:** `Frontmatter::resolve_harness_model(&self, harness: &str)` — called by preflight (for `agent_model` attribution), `run`, `stream`, `orchestrate`, `hooks`, and `capture`.

**Example frontmatter:**
```yaml
model: gpt-5
claude_model: opus
codex_model: o3-pro
```

**Files changed:** `frontmatter.rs` (struct + method + merge_fields + 11 tests), `preflight.rs`, `run.rs`, `stream.rs`, `orchestrate.rs`, `hooks.rs`, `capture.rs`, `start.rs`, `specs/02-document-format.md`.

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

Full suite: 1729 tests pass (1 pre-existing codex hook flake).

❯ Add to pending changing `agent:pending` to `agent:backlog`. `agent:pending` will alias to `agent:backlog`...but `agent:backlog` is the canonical element name. Please update the agent document template to use `agent:backlog`.

❯ Please add `agent:icebox` as another element. This is the project icebox not in the active backlog. Please add `agent:icebox` to the template.
### Re: backlog rename + icebox element — opus-4-6

Added both to pending:

- **#hbacklog** — Rename `agent:pending` to `agent:backlog`. The existing `agent:pending` marker becomes an alias so old documents keep working, but `agent:backlog` is the canonical element name going forward. Scaffold sites in `claim.rs` and `sync.rs`, the document-format spec, and the SKILL.md instructions all need updating.

- **#hicebox** — New `agent:icebox` component for parking items outside the active backlog. Placed after `agent:backlog` in the template. Needs scaffold, spec, parser, and component handling support.

Also noted the `claude_model: claude-opus-4-6` frontmatter addition.

❯ do #hbacklog. #spec-test-build-install-commit-push
### Re: do #hbacklog — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`58f0fcb` on `20260422/codex-agent-backend`).

#### What was built

**`component.rs`** — new public helper `is_backlog_component(name)` that returns true for both `"backlog"` (canonical) and `"pending"` (legacy alias). Also exports `BACKLOG_COMPONENT` and `BACKLOG_ALIAS` constants.

**15 source files updated** — all component lookups (`c.name == "pending"`) replaced with `is_backlog_component(&c.name)` across compact.rs, extract.rs, git.rs, main.rs, notify.rs, pending_cmd.rs, plan.rs, preflight.rs, session_check.rs, template.rs, write.rs.

**Scaffolds** — claim.rs, sync.rs, notify.rs now emit `<!-- agent:backlog patch=replace -->` / `<!-- /agent:backlog -->` with a `## Backlog` heading for new documents.

**Backward compatibility** — existing documents with `<!-- agent:pending -->` markers continue to work unchanged. The parser still extracts `"pending"` as the component name; all runtime lookups accept both names. Test fixtures retained `agent:pending` markers to verify this.

**Specs/docs** — SKILL.md (both source and installed), document-format.md, pending-ops.md, pending-system.md, 07-commands.md, PLUGIN-SPEC.md, examples/README.md all updated to reference `agent:backlog` as canonical with `agent:pending` as legacy alias.

#### Tests

3 new tests in component.rs (`is_backlog_component_accepts_both_names`, `backlog_component_parsed_from_new_marker`, `legacy_pending_marker_still_parsed`). 1 test added in write.rs (`!is_append_mode_component("backlog")`). 3 test assertions updated for changed error messages/scaffolds.

Full suite: 1241 unit tests pass (1 pre-existing codex hook flake).

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

❯ `patch=replace` is redundant in `agent:backlog` since the agent-doc binary has a subcommand and essentially owns how agent:backlog is managed. Please deprecate the patch attribute. #spec-test-build-install-commit-push
### Re: deprecate patch=replace on agent:backlog — opus-4-6

Implemented, tested, built, installed, committed, and pushed (`731992a` on `20260422/codex-agent-backend`).

#### What changed

`strip_backlog_patch_attr()` in `component.rs` strips `patch=...` and `mode=...` from `agent:backlog` / `agent:pending` opening tags with a deprecation warning to stderr. Applied automatically via `normalize_template_structure_or_fail()` in the write path so all template writes strip the attribute.

- **Scaffolding** (`claim.rs`, `sync.rs`, `notify.rs`): new documents emit `&lt;!-- agent:backlog --&gt;` without the attribute
- **git.rs**: component comparisons tolerate patch attr mismatch on backlog during the transition (old snapshots with `patch=replace`, new files without)
- **Specs/runbooks**: `pending-system.md` and `document-format.md` updated with deprecation note and cleaned examples

#### Tests

6 new unit tests for the stripping function (strips `patch=replace`, `mode=replace`, preserves other attrs, no-ops on exchange/no-attr). Full suite: 1241 unit tests pass (1 pre-existing codex hook flake).

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14.

❯ Sanity check: `agent:exchange` `patch` attribute is still active. Make sure `patch` is not also deprecated on `agent:exchange`.
### Re: sanity check — agent:exchange patch attribute — opus-4-6

Confirmed safe. `strip_backlog_patch_attr()` iterates only over `BACKLOG_COMPONENT` (`"backlog"`) and `BACKLOG_ALIAS` (`"pending"`) — `agent:exchange` is never matched.

`agent:exchange patch=append` remains active and meaningful: `template::default_mode("exchange")` returns `"append"` (the built-in default), and the template engine uses it to decide append-vs-replace behavior in `apply_patches_with_overrides()`. The attribute is also explicitly documented in `specs/02-document-format.md` as part of the inline-attr precedence chain.

❯ do #hicebox. #spec-test-build-install-commit-push
### Re: do #hicebox — opus-4-6

Added `agent:icebox` component to the template system.

#### Changes

**component.rs** — `ICEBOX_COMPONENT = "icebox"` constant, `is_icebox_component()` helper, plus two unit tests (name detection + parser roundtrip).

**Scaffolds** — claim.rs (both empty-file and component scaffold), sync.rs now emit `## Icebox` / `&lt;!-- agent:icebox --&gt;` / `&lt;!-- /agent:icebox --&gt;` after backlog. Test assertions updated from four to five components.

**Specs/docs** — `02-document-format.md` standard component table includes icebox (`replace` default). `runbooks/document-format.md` example block and conventional-names list updated.

**No special handling needed** — icebox is a simple content component. It doesn't need `strip_backlog_patch_attr` equivalent, template.rs rejection logic, or `--pending-*` CLI flags. Users manage icebox content by editing the document directly.

#### Build

`cargo install --path .` + `cargo build --release` completed. Both `.bin/agent-doc` and `~/.cargo/bin/agent-doc` are live at 0.33.14. Committed and pushed as `69843ee` on `20260422/codex-agent-backend`.

❯ How can we automatically migrate existing agent docs to use `agent:backlog` and `agent:icebox`?
### Re: automatic migration to backlog/icebox — opus-4-6

Short answer: you don't strictly need to migrate. The alias system already handles it transparently.

#### Why migration isn't required

`is_backlog_component()` in `component.rs` accepts both `"backlog"` and `"pending"` at parse time. Every read path — template patching, compaction, pending ops, strip-attr — goes through this helper. Existing `<!-- agent:pending -->` documents work identically to `<!-- agent:backlog -->` ones with zero behavioral difference.

New operations (e.g., `agent-doc notify --pending-add`) already emit the canonical `<!-- agent:backlog -->` form when auto-creating the component. So documents naturally migrate forward as they're used — the next compaction or auto-create produces `backlog`, not `pending`.

#### If you want cosmetic consistency

A batch rename for existing docs could be done with sed:

```bash
find . -name '*.md' -path '*/.agent-doc' -prune -o \
  -name '*.md' -exec grep -l 'agent:pending' {} + | \
  xargs sed -i \
    -e 's/<!-- agent:pending/<!-- agent:backlog/g' \
    -e 's/<!-- \/agent:pending/<!-- \/agent:backlog/g'
```

Or as a dedicated CLI subcommand (`agent-doc migrate <FILE|DIR>`) that:
1. Parses with `component::parse()`
2. Renames `pending` → `backlog` markers in-place
3. Optionally adds an empty `<!-- agent:icebox -->` section after backlog
4. Updates snapshot + commits

#### Icebox is opt-in

`agent:icebox` is a passive content component — no CLI flags, no special handling. Users add it when they want to park items out of the active backlog. There's no reason to inject it into every existing document automatically. Better to let users add it when they first need it, same as `agent:status`.

#### Recommendation

The alias system is sufficient for backward compat. A CLI `migrate` command would be a nice-to-have for repos that want to standardize their markup, but it's low priority since behavior is identical either way. The sed one-liner covers the 90% case today.

❯ Add the migrate command. Remove deprecated attributes such as `agent:backlog patch` or `agent:pending patch`->`agent:backlog patch`.
❯ #spec-test-build-install-commit-push
### Re: migrate command — opus-4-6

Added `agent-doc migrate` with three modes:

- **Positional files:** `agent-doc migrate tasks/plan.md tasks/bugs.md`
- **Project scan:** `agent-doc migrate --all` — walks the project root for `.md` files containing `<!-- agent:pending` markers
- **Preview:** `agent-doc migrate --dry-run` — reports which files would change without writing

#### What it migrates

1. `<!-- agent:pending ... -->` → `&lt;!-- agent:backlog ... --&gt;` (open tags)
2. `&lt;!-- /agent:pending --&gt;` → `&lt;!-- /agent:backlog --&gt;` (close tags)
3. Strips deprecated `patch=`/`mode=` attributes from backlog component tags (both newly-renamed and already-canonical)

Markers inside fenced code blocks and inline code spans are left untouched. Idempotent — running twice produces identical output.

#### Implementation

- New module: `src/migrate.rs` with 7 unit tests (pending→backlog rename, attr stripping, code-block immunity, idempotency, etc.)
- Wired as `Commands::Migrate` in `main.rs`
- Added to `commands.rs` for editor autocomplete
- Spec entry in `specs/07-commands.md`

#### Build

`cargo install --path .` + `cargo build --release` completed. Both binaries are live at 0.33.14. Committed and pushed as `5850228` on `20260422/codex-agent-backend`.
<!-- agent:boundary:98698f9a -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
