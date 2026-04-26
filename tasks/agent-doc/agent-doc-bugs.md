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
<!-- agent:boundary:9630439c -->
<!-- /agent:exchange -->
<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#queue] Implement agent:queue component with hybrid syntax (- list + ~~~prompt/--- fences), sequential dispatch, and CLI scaffolding — tasks/agent-doc/agent-queue-plan.md
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
