---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Completed: #gcommcache caches Louvain community detection under `.tsift/community-cache`, emits community diagnostics, and bounds tagpath annotation to displayed/focused communities. Local verification passed (`cargo test -q communities_json`, `cargo test -q explain_json`, `cargo build`, `cargo install --path .`, `make check`); tsift CI run 26459272826 is green. The tsift submodule commit `bb542cd` is pushed, and the root pointer commit `13d447159` is pushed.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-163522.md`*

Compacted content:
- Archived 3 response topic(s): #gcommcache; do #gcommtagctx; do [#gcommgate]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:ff08abd2 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#f14h] tsift graph auto-update: tsift index is currently stale by default (339 stale files, 5d since last manual reindex on Brian's machine). No commit hook, no nightly job. Options: (a) pre-commit hook scoped to changed scopes (cheap, deterministic, but per-clone install), (b) post-commit background reindex (zero blocking cost), (c) GitHub Actions nightly + on-push job that publishes a fresh graph.db artifact (no per-clone setup, lets graphs travel with the repo). Decide and implement. See session-share/tasks/20260526-doc-graph.md.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
