---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.15 released. Transfer auto-init (template default), write silent-drop warnings, investigate-behavior runbook.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. JetBrains perf: GC 3.9GB blob, VCS refresh debounce 500ms, plugin timing instrumentation v0.2.42, state diagnostics logging. `agent-doc clean --archive`. Plugin version check in make precommit. Boundary (HEAD) bug: transfer now commits target so transferred headings are in git HEAD. All genuinely new headings get (HEAD). Perf issue was ephemeral (restart fixed).*

When transferring from tasks/recruit/robert-ross to tasks/dreamable/build-party.md the default element template was not added to the file content. Why?
### Re: Transfer doesn't apply default template

The `transfer` command in `extract.rs` requires the target file to already exist:

```rust
if !target.exists() {
    anyhow::bail!("target file not found: {}", target.display());
}
```

It never calls `init_file()` to scaffold a new document. The default template (frontmatter + `&lt;!-- agent:exchange --&gt;` component structure) only lives in `init.rs` and is only invoked by `agent-doc init`.

So when you transferred to `tasks/dreamable/build-party.md`, either:
1. The file didn't exist and transfer failed with an error, or
2. The file existed but was a bare file without template components — transfer appended content to EOF without wrapping it in `&lt;!-- agent:exchange --&gt;` tags

The fix would be: if the target doesn't exist, auto-create it via `init_file()` before proceeding with the transfer. This would give it proper frontmatter and component structure.

See the content of tasks/dreamable/build-party.md and the logs to see what happened.
### Re: What happened during the transfer

Checked both files:

**`tasks/dreamable/build-party.md`** — append-mode document (no `agent_doc_format: template`, no `&lt;!-- agent:exchange --&gt;` component). Just has frontmatter + `# Session: Untitled Session` + `## User`.

**`tasks/recruit/robert-ross.md`** — template-mode document with full component structure.

So the transfer found the target file (no bail), but the target had no `exchange` component. Per `extract.rs` line 186-191, when the target component doesn't exist, content gets appended to EOF as raw text — no component tags wrapping it.

The bug is: transfer doesn't check whether the target has the required component structure, and doesn't scaffold it if missing. Two possible fixes:

1. **Auto-init target**: If target lacks the component, run `init_file()` to convert it to template mode first
2. **Auto-create component**: Insert `&lt;!-- agent:exchange --&gt;...&lt;!-- /agent:exchange --&gt;` around the transferred content if the target doesn't have that component

Which approach do you prefer, or should it just error with a clear message saying the target needs template components?

The file did not exist. It seems that the file was created and added + committed to git.
Please look at the agent-doc logs and the git logs for more info to what happened.
Please add a runbook for any agent-doc behavior investigation.
- look at the agent-doc logs
- look at the git logs
- look at the affected files
### Re: Investigation results

**ops.log timeline (timestamps are epoch):**

1. `1775417417` — `write_origin file=tasks/dreamable/build-party.md origin=skill` — the robert-ross skill session wrote to build-party.md
2. `1775417417` — `ipc_write_attempt ... patches=0 unmatched_len=1766` — **0 patches matched, 1766 bytes silently dropped**
3. `1775417420` — `commit_staging ... snap_len=0 file_len=117` — file was auto-created with 117 bytes (default append-mode frontmatter only)
4. `1775417420` — `commit_success` — committed the empty shell

**git log confirms:** Single commit `2d24da2f` at 15:30:20 created build-party.md with 8 lines (just frontmatter + `# Session: Untitled Session` + `## User`).

**git diff on robert-ross.md** shows the user wrote `Transfer the livekit repo evaluation to tasks/dreamable/build-party.md` inline. The skill session then used `agent-doc write` with template `<!-- patch:exchange -->` blocks targeting a non-existent file. `write` auto-created the file in **append mode** (no `&lt;!-- agent:exchange --&gt;` component), so the patches had nothing to match. 1766 bytes of content were dropped.

**Root cause:** `agent-doc write` auto-creates files in append mode. Template patch blocks require `&lt;!-- agent:exchange --&gt;` tags. The skill should have used `agent-doc init` first (to scaffold template components), then `agent-doc transfer` or `agent-doc write`.

**Runbook created:** Added `runbooks/investigate-behavior.md` with the checklist:
1. Check ops.log (write_origin, ipc_write_attempt, drift_warning)
2. Check git history (commits, diffs, file creation)
3. Check affected files (content, snapshot, frontmatter, components)
4. Common failure patterns table
5. Report template

**Bug to fix:** `agent-doc write` should either (a) error when template patches target a file without matching components, or (b) auto-init the file in template mode when it receives template patches. Currently it silently drops unmatched content.

do
- [ ] Fix: `agent-doc write` silently drops template patches when target has no components
- [ ] Fix: transfer/write should auto-init target in template mode if it doesn't exist
### Re: Both fixes implemented

**Fix 1: `transfer()` auto-inits target** (`extract.rs`)
- When target doesn't exist, auto-creates it matching source format (template or inline): we should default to template format
- Reads source frontmatter to detect template mode; creates parent dirs if needed
- Generates fresh UUID session, copies agent name from source
- Logs: `[transfer] Auto-created <path> (template|inline)`: template is the default

**Fix 2: Write warns on no-component targets** (`write.rs`)
- `run_stream`: After parsing patches, if no patch blocks found but unmatched content exists, checks whether file has template components. If none: warns with actionable message (`agent-doc init --mode template`)
- `try_ipc`: After `build_ipc_patches_json`, if `ipc_patches` is empty but unmatched content is non-empty, emits WARNING + ops.log entry (`ipc_unmatched_content_dropped`)
- Also improved ops.log to include `ipc_patches` count (synthesized patches) alongside original `patches` count

**Verification:** `make check` passes — all 697 tests + clippy clean.

**Files changed:**
- `src/agent-doc/src/extract.rs` — auto-init in `transfer()`, updated module spec + contracts
- `src/agent-doc/src/write.rs` — warnings in `run_stream` and `try_ipc`, improved logging

build and install for testing
### Re: Default to template + built and installed

Updated `transfer()` to always default to template format when auto-creating target (removed source-format matching logic). Simplified the code — no more `is_template` detection.

Built, tested (all 697 pass), and installed:
- `cargo build --release` — release binary at `src/agent-doc/target/release/agent-doc`
- `cargo install --path .` — updated `~/.cargo/bin/agent-doc`

Ready for testing. Try transferring to a non-existent target file.
release
### Re: v0.31.15 released (HEAD)

Published to all targets:
- **crates.io:** `cargo publish` — agent-doc v0.31.15
- **PyPI:** manylinux2014 wheel uploaded
- **GitHub Release:** https://github.com/btakita/agent-doc/releases/tag/v0.31.15
- **Binary:** `cargo install --path .` — `~/.cargo/bin/agent-doc` updated
<!-- agent:boundary:7fb7b19c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Add `git::commit(target)` to transfer command
- [x] Fix: `agent-doc write` silently drops template patches when target has no components
- [x] Fix: transfer/write should auto-init target in template mode if it doesn't exist
- [x] Release v0.31.15
<!-- /agent:pending -->
