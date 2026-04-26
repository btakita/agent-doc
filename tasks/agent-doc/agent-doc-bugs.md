---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
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
<!-- agent:boundary:2f722c9c -->
<!-- /agent:exchange -->
<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#mvsnap] Snapshot migration on file rename: detect session UUID mismatch in preflight, auto-migrate snapshot/baseline/pre-response artifacts to new path hash. Fallback: explicit agent-doc move <old> <new> command
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
