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

Logged three fresh log-triage follow-ups. No open backlog items.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-012547.md`*

Compacted content:
- Archived 1 response topic(s): stale full-content IPC guard
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Why is agent-doc doing Full-content IPC replacements as I am typing in a prompt? Is there any good reason? It's causing problems that corrupt the document that cannot be fixed apparently...
❯ Note that the html comment content below this agent:exchange was once again deleted.
### Re: full-content IPC replacements — gpt-5

There is no good reason for `agent-doc` to apply a full-content editor replacement while you are typing. The only legitimate use is a recovery/re-delivery path after `agent-doc` has already repaired disk content and needs the editor buffer to catch up. That path must fail closed if the live file has drifted.

I found a real gap in the response fallback path. Operator-owned full-content IPC already used an explicit source buffer proof, but response fallback without an explicit source was proving against the buffer read immediately before delivery. If you typed after the fallback content was computed, the stale replacement could be sent with a proof for the newer live buffer, so a compliant editor plugin could still accept the stale full-content replacement.

Fixed in `src/agent-doc/src/write.rs`: response fallback full-content IPC now treats the target fallback content as the expected source when no explicit source is supplied. If the current file contains live prompt drift, the binary rejects before socket/file IPC and logs `full_content_source_buffer_rejected source=response_fallback`; the editor never receives the stale full-content payload.

I also added `response_fallback_full_content_rejects_live_drift_before_socket_delivery`, which simulates a socket listener and proves the stale response fallback is rejected before delivery, leaves the live prompt intact, saves no snapshot, and emits the rejection log. The existing preflight/comment-preservation tests still pass.

Changed paths:
- `src/agent-doc/src/write.rs`

Commands run:
- `cargo fmt`
- `cargo test response_fallback_full_content_rejects_live_drift_before_socket_delivery -- --nocapture`
- `cargo test preflight_preserves_unrelated_lines_in_mixed_post_exchange_duplicate_prompt_comment -- --nocapture`
- `cargo test preflight_waits_for_typing_before_duplicate_prompt_comment_cleanup -- --nocapture`
- `cargo test preflight_does_not_relocate_prompt_text_inside_post_exchange_html_comment -- --nocapture`
- `make check`
- `gh run view 26319677438 --json conclusion,status,jobs,url`
- `cargo install --path .`

Verification:
- `make check` passed: clippy, full non-live test suite, and medium deterministic simulator.
- Latest GitHub CI run `26319677438` is green. Its `check` job included successful `Run make check` and successful `Run make tmux-ci` steps.
- Installed local `agent-doc` binary still reports `agent-doc 0.33.16`, now built from the fixed source.

Confidence: high for the identified stale response-fallback full-content path. The source change is still uncommitted because this visible prompt did not request a repo commit or push.

Escalation: none needed.
<!-- agent:boundary:419d98d3 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
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
