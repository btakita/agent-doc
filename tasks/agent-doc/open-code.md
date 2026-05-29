---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260529-013114.md`*

Compacted content:
- Archived 12 response topic(s): do #e130; do #77v6; do #ysv9; 9 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...

### Re: queue tidy → #bz6s Wave 5 complete (extraction done) — opus-4-7

You pruned the struck queue items, leaving `do [#bz6s]` at the head of the active auto queue. The big-bang (increment 6) was already in; the only `#bz6s` work left was Wave 5, so I ran it.

#### Wave 5 — prune CLI-shell deps + measure

- **Removed 12 now-transitive-only heavy deps** from the CLI shell's `[dependencies]`: `alacritty_terminal`, `fs2`, `hex`, `htmd`, `interprocess`, `portable-pty`, `regex`, `rusqlite`, `serde_yaml`, `signal-hook`, `tagpath`, `yrs`. They reach `agent-doc` only transitively through orchestration now. (`hex` moved to `[dev-dependencies]` — integration tests still use it.)
- **CLI shell direct deps: 36 → 24**, confirmed by `cargo tree -p agent-doc -e no-dev --depth 1` (none of the heavy crates appear).
- **Build-time payoff measured:** shell-only incremental rebuild is **~1.5s** (orchestration is a separate cached 340-dep compilation unit), vs recompiling within the old ~154K-LOC / 266-crate monolith. Recorded in `COMPILE_BASELINE.md`.

#### `#bz6s` acceptance criteria — all met

1. ✅ `agent-doc-orchestration` compiles standalone on `agent-doc-core`.
2. ✅ CLI shell builds without the heavy deps as **direct** deps.
3. ✅ CLI-shell rebuild measured + recorded (`COMPILE_BASELINE.md`).
4. ✅ `make check` green across all three crates (modulo the pre-existing live-tmux `codex_bare_run` test).
5. ✅ `nm -D libagent_doc.so` — full 39-symbol FFI surface intact.

Commits: submodule `53416ebc`, superproject pointer `5ce8cd69d`. Marking `#bz6s` **done**.

#### What's left

- The `pub use` shims in `agent-doc` are still in place (they're cheap and correct). The plan's optional final polish — rewriting call sites from `crate::X` to `agent_doc_orchestration::X` and deleting the shims — is **not** required by the acceptance criteria; flag it as a separate low-priority cleanup if you ever want it.
- Queue still has `do [#yybc]` (FFI-relocation pattern tracking) and `do [#adcr]` (note: `agent-doc-core` is already extracted/published — `#adcr` looks already-satisfied; worth verifying before running).
<!-- agent:boundary:91b34c89 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#bz6s]~
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#yybc] #k9e1-ffi-mem POC follow-up: track the proven pattern (pub use agent_doc_core::ffi::* + force_link_core_ffi_symbols reference function in main) for future pure FFI relocations. POC moved agent_doc_free_string + agent_doc_free_state; nm verification confirmed symbols still in libagent_doc.so export table.
- [ ] [#adcr] Extract agent-doc-core crate (~17K lines) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
