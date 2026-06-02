---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-183027.md`*

Compacted content:
- Archived 6 response topic(s): #mdastcontract; #mdsectionspans; #mdsectionedits deferred; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps for the Markdown AST API, performance, and token efficiency?

### Re: #next-steps — gpt-5

Current state: the Markdown semantic edit contract is already in place. `edit-intents` recognizes Markdown heading, section, list-item, and code-fence intents, and the current implementation has stable section/block spans plus guarded apply/verify coverage.

The next layer should be:

1. `#mdastapi` — promote Markdown AST data from internal span helpers into a first-class JSON/envelope API that returns stable node handles, hierarchy/section paths, byte/body spans, list depth, fence language, and expansion commands. This is the top backlog item because it gives read, write, editor, and digest flows one shared contract.
2. `#mdparsecache` — reduce repeated Markdown parse/extract work. The current section/block helpers re-derive symbols during planning; cache one parsed/extracted view per current buffer and add large-session benchmarks with phase timings.
3. `#mdtokens` — make Markdown output token-budgeted by default: outline-first previews, selected section/block expansion handles, and fixtures proving savings for `session-review` / `context-pack` against raw session or spec bodies.

Added those backlog items in priority order. Top backlog item: `#mdastapi`.

Verification: no code changes were made. I ran `agent-doc preflight`, `agent-doc plan`, `agent-doc orchestrate --plan`, `agent-doc read`, refreshed the `tsift` index, and used bounded `tsift` search/source-read/session-review context before closeout.
<!-- agent:boundary:fa30c490 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#mdastapi] Promote Markdown AST to a first-class JSON/envelope API: stable node handles, hierarchy/section path, byte and body spans, block kind metadata, list/fence attributes, and expansion commands for source-read/symbol-read/edit-intents consumers.
- [ ] [#mdparsecache] Cache and reuse per-file Markdown parse/extract results across edit-intents planning/apply and source-read enrichment; add large-document benchmarks and phase timings for section/block lookup hot paths.
- [ ] [#mdtokens] Add token-budgeted Markdown projections: outline-first section/block previews, selected-node expansion handles, and measured token-savings fixtures for session-review/context-pack instead of replaying raw markdown bodies.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
