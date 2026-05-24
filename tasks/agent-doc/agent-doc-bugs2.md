---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-200315.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ When I ran JB `Compact Exchange`, JB had a `File Cache Conflict`.

❯ As I was typing in this prompt, the prompt line was duplicated...and the full-document IPC corrupt document issue occurred. How can we make agent-doc Codex more robust so this does not happen anymore? Are there race conditions? Is the full-document IPC too aggressive?

❯ #spec-test-build-install-commit-push
### Re: JB Compact Exchange conflict and duplicate prompt prefix — gpt-5

The current failure path was not old full-document response IPC being too aggressive; that fallback is already disabled on the template response path. The remaining race was more specific: `Compact Exchange` could still take the guarded direct-disk write path while JetBrains had an idle but unsaved editor buffer. Typing-idle only proved that edits had stopped, not that the visible editor buffer and disk were the same. That explains both the JetBrains File Cache Conflict and the stale/duplicated prompt-prefix state that could survive around compact exchange.

I changed the closeout path to fail closed on that exact condition. Editor integrations now send a cheap live-buffer digest through FFI on markdown document changes, and direct visible writes, including compact exchange, compare that digest after the typing-idle wait against the content they are about to overwrite. If the editor-visible buffer differs from the expected disk/merge content, the binary logs `visible_write_deferred_live_buffer_changed` / `visible_write_current_changed` and refuses the write with a save-or-discard message instead of corrupting the session document.

I also tightened the JetBrains action itself: JB Compact Exchange now waits for the tracked document to go idle, saves documents through `FileDocumentManager`, and only then invokes the compact command. JetBrains and VS Code both publish the new SHA-256/byte-length digest, and the specs now require this guard for direct disk writes.

Verification completed:

- `cargo fmt`
- `git diff --check`
- targeted regressions: `cargo test live_buffer_digest_records_visible_content`, `cargo test visible_write_guard_blocks_when_idle_editor_buffer_differs_from_disk`, `cargo test component_compact_direct_fallback_rejects_idle_unsaved_editor_buffer`
- full suite: `make check`
- VS Code: `npm test`, `npm run package`, `agent-doc plugin install --local vscode` installed `agent-doc-0.2.21.vsix`
- JetBrains: `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`, `agent-doc plugin install --local jetbrains` installed `agent-doc-jetbrains-0.2.130` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`
- local install: `cargo install --path .`, `cargo build --release --lib`, `agent-doc lib-install`
- GitHub CI: run `26371267459` passed, including the `make tmux-ci` step

Local commits made before this session-document closeout:

- `src/agent-doc`: `fea1aa7` (`fix: guard compact exchange against unsaved editor buffers`)
- root gitlink update: `256ccfb` (`chore: update agent-doc submodule`)

Per the agent-doc closeout contract, I am pushing after this finalized document write completes.
<!-- agent:boundary:d2ef5c66 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#cstop]
- do [#e86b]
- do [#rpatch]
- do [#qresidue]
- do [#qinactive]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
