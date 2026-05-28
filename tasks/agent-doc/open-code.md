---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-001745.md`*

Compacted content:
- Archived 5 response topic(s): Clear Session Context error; Run Agent Doc dispatch proof timeout; do option 2; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
<!-- agent:boundary:f3cbfa78 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->

<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#n89s] #agent-doc-bug Fix `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness (routed_reopen.rs:319) — interim fix until tracker is built
- [ ] [#xd6k] #agent-doc-bug Add model name validation in `canonical_model_name` or probe builder for opencode harness — reject bare names without provider prefix (model_tier.rs:256, codex.rs:534)
- [ ] [#adcr] Extract `agent-doc-core` crate (~17K lines: template, diff, component, frontmatter, crdt, pending, model_tier + leaf modules) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
- [ ] [#fr8q] [recommended] Compact this session (116 exchange lines, block-level accretion) before next implementation turn
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
