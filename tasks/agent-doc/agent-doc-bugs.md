---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: codex
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---



## Status

<!-- agent:status patch=replace -->

✅ Verified and landed the bare-path compatibility alias plus committed-historical snapshot self-heal in `src/agent-doc`. `agent-doc <FILE>` now routes to `run`, and `commit` / `session-check` repair already-committed historical `### Re:` drift when `HEAD` proves the response is no longer out-of-band. The latest `src/agent-doc` change is built, installed locally, committed, and pushed.
Remaining open work in this bug doc is still only the multi-user pending auto-create policy follow-up (`#ha3h`), but it remains gated: the linked issue explicitly says not to ship it before multi-user workspace support, and the current `src/agent-doc` tree still documents the product as single-user only.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

<!-- agent:boundary:3a8b695f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#vpm8] [recommended] Prevent template compact/write/finalize from reintroducing out-of-exchange conversation blocks: validate/reparse post-merge and post-compact writeback, auto-repair safe cases, fail closed on ambiguous exchange-boundary drift.
- [ ] [#6nj9] [recommended] Eliminate post-success working-tree dirtiness when the cycle is already committed: avoid leaving only HEAD-marker / agent-boundary churn in session docs such as src/session-share/tasks/claudescore.md.
- [ ] [#ndyy] Normalize src/agent-doc before rebasing: remove tracked .agent-doc runtime files from the index, remove the typo .gitgnore file if accidental, keep the real ignore rule, then rebase 20260422/codex-agent-backend onto origin/main.
- [ ] [#n5hs] Prevent delivered-but-uncommitted patchbacks: if response content reaches the live file/snapshot before commit, persist recoverable cycle state and require same-repo/submodule commit completion or fail closed until preflight can deterministically finish it.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

[//]: # (do #6nj9. update spec + tests. build + install for local testing. commit + push)
[//]: # Fix test suite warnings ignored tests
[//]: # (Please diagnose why audit-docs is hanging.)
