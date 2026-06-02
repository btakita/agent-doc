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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-185822.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #mdastapi — gpt-5

> **Queue prompt:**
>
> [#mdparsecache]


> **Queue prompt:**
>
> [#mdastapi]


Completed `#mdastapi`.

Implemented `tsift markdown-ast` as a first-class Markdown JSON/envelope projection with stable `mdast-*` node handles plus `span-*` bridge handles, hierarchy and section paths, byte/body spans, heading/list/fence metadata, and node/source/edit expansion commands. Wired Markdown `source-read` and Markdown `symbol-read` to surface `markdown-ast` follow-ups, including `symbol-read` to `markdown-ast --node <span-*>`.

Updated `SPEC.md` and added compiled CLI coverage for the new command, source-read expansion, and symbol-read span expansion.

Commits pushed:
- `btakita/tsift@1b141b6` `feat: add markdown ast projection`
- `btakita/agent-loop@7e82414e1` `chore: update tsift submodule`

Verification:
- `cargo check -p tsift-cli` passed.
- `cargo test -p tsift-cli markdown` passed.
- `make check` passed after the final edit.
- `cargo build --workspace` passed.
- `cargo install --path . --force` passed.
- Installed smoke test passed: `tsift --envelope markdown-ast SPEC.md --path . --budget small --json`.
- GitHub Actions CI run `26842414251` passed. The latest CI workflow has no tmux-named job to inspect, so there was no tmux leg to rerun.

### Re: #mdtokens — gpt-5

Completed `#mdparsecache` and `#mdtokens`.

Implemented a reusable in-process Markdown projection cache keyed by file/content hash, then routed Markdown section/block edit lookups through it so repeated edit-intents planning/apply paths reuse one extracted tree. The Markdown projection now reports cache evidence, node counts, and phase timings for parse/extract and outline projection.

Added token-budgeted Markdown projections: `markdown-ast` now exposes outline-first mode, selected-node mode via `--node <mdast-*|span-*>`, and selected-node expansion handles; `source-read` now includes a bounded Markdown window outline for visible nodes. The real-session token-savings fixture now measures raw session Markdown body replay against compact outline plus selected-node expansion handles.

Commits pushed:
- `btakita/tsift@ebf9395` `feat: cache markdown projections`
- `btakita/agent-loop@5baf64498` `chore: update tsift submodule`

Verification:
- `cargo check -p tsift-cli` passed.
- `cargo test -p tsift-cli markdown_ast` passed.
- `cargo test -p tsift-cli token_savings` passed.
- `cargo test -p tsift-cli markdown` passed.
- `make check` passed after the final edit.
- `cargo build --workspace` passed.
- `cargo install --path . --force` passed.
- Installed smoke checks passed for `markdown-ast` outline/phase fields and `source-read` Markdown window outlines.
- GitHub Actions CI run `26843503622` passed. The tsift CI workflow has no tmux-named job to inspect, so there was no tmux leg to rerun.
<!-- agent:boundary:06a24c84 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#mdastapi]~
- ~[#mdparsecache]~
- [#mdtokens]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [x] [#mdparsecache] Cache and reuse per-file Markdown parse/extract results across edit-intents planning/apply and source-read enrichment; add large-document benchmarks and phase timings for section/block lookup hot paths.
- [x] [#mdtokens] Add token-budgeted Markdown projections: outline-first section/block previews, selected-node expansion handles, and measured token-savings fixtures for session-review/context-pack instead of replaying raw markdown bodies.
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
