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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-130103.md`*

Compacted content:
- Archived 1 response topic(s): post-exchange comment ownership
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The duplicate content corrupt document happened as I was typing into the comment below this agent:exchange. Did you detect this issue in the logs? Do we need to add more conditions for the full-document IPC? Should we disable the entire full-document IPC? #spec-test-build-install-commit-push

❯ The duplicate prompt issues happened as I was typing this prompt inside the agent:exchange. Please identify root causes. #spec-test-build-install-commit-push
### Re: full-document IPC duplicate prompt corruption — gpt-5

Yes. The logs show a real earlier full-document IPC hazard, but not a fresh one for this prompt.

What I found:
- `2026-05-23 18:19:11 EDT`: `compact_exchange` took the `try_ipc_full_content_socket` path (`write_mode=socket_full_content`, `patches=0`) and then snapshot/file lengths diverged (`snap_len=1970`, `file_len=2199`) before repair and drift diagnostics. That is the path that could overwrite a live prompt/scratch edit with a stale whole-document image.
- `2026-05-23 18:38:05 EDT`: route cleanup logged `post_exchange_duplicate_prompt_comment_removed`, which is consistent with recovering a duplicated prompt/comment below the exchange.
- The current prompt cycle, around `2026-05-24 09:03 EDT`, does not show a new `fullContent` write. It shows normal component IPC plus guarded compact direct-write, then user follow-up drift before preflight.

I do not think we should disable full-document IPC globally. It should remain a narrow repair/fallback primitive, but compact exchange/template response paths should not use it as their ordinary write path. The missing condition was that source-buffer proof must cover the whole visible document, including plain HTML scratch comments below `agent:exchange`; if a user types a prompt-like note there after target computation, stale full-document replacement must reject before socket/file delivery or snapshot save.

I added tests/spec coverage for that condition:
- `src/agent-doc/src/compact.rs`: compact exchange direct fallback rejects a late post-exchange scratch comment and preserves the live file/snapshot.
- `src/agent-doc/src/write.rs`: `fullContent` operator IPC rejects the same late scratch-comment drift before creating IPC patch/snapshot.
- `src/agent-doc/specs/07-closeout-commands.md`: full-content IPC remains scoped to source-buffer-proven repair/fallback and must reject post-exchange scratch drift.
- `src/agent-doc/specs/12-deterministic-simulation.md`: deterministic coverage now calls out this regression class.

Verification:
- `cargo fmt`
- `cargo test late_post_exchange_scratch_comment`
- `git diff --check`
- `make check`
- `cargo build`
- `make install`
- `agent-doc --version` => `agent-doc 0.33.16`
- `make tmux-ci`
- GitHub CI `26362377549` on `fc0c511` completed successfully: remote `make check` and remote `make tmux-ci` both passed.
- Parent repo has no workflow named `CI`.

Committed and pushed:
- `src/agent-doc`: `fc0c5119 test: cover full-document scratch comment drift` pushed to `20260422/codex-agent-backend`.
- parent `agent-loop`: `c60098968 agent-doc response: full-content scratch drift coverage` pushed to `main`.

Escalation: none.
Confidence: high.
<!-- agent:boundary:df092f90 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#1mm8]
- do [#hg0n]
- do [#te1d]
- do [#pmz2]
- do [#f4xr]
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
