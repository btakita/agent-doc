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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-153847.md`*

Compacted content:
- Archived 5 response topic(s): #next-steps for SQLite graph + tagpath community search; #gprojrows; closeout repair; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #gcommcache — gpt-5

Implemented and pushed `#gcommcache`.

- Added cached community detection keyed by cache version, root/scope, index graph watermark, and tagpath state/hash; cache hits skip `db.all_edges()` and Louvain.
- Added `community_diagnostics` to `tsift communities --json` and `tsift explain --json` with cache hit, edge count, iterations, tagpath state, and annotation counts.
- Bounded tagpath member annotation to displayed communities for `communities --limit N` and the focused symbol community for `explain`.
- Updated `SPEC.md` and integration coverage for cache diagnostics plus bounded annotation.

Commits:
- tsift `bb542cd` (`feat: cache community detection`) pushed to `btakita/tsift main`
- root pointer `13d447159` (`chore: update tsift community cache`) pushed to `btakita/agent-loop main`

Verification:
- `cargo test -q communities_json`
- `cargo test -q explain_json`
- `cargo build`
- `cargo install --path .`
- `make check`
- GitHub Actions CI run `26459272826`: success for `clippy + test` and `live Convex graph acceptance`

CI notes: this repo's CI workflow has no separate `tmux-test` job. The run completed with non-blocking annotations for GitHub's Node.js 20 action deprecation and the existing live Convex configuration note.
<!-- agent:boundary:209f2e80 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gprojrows]~
- ~do [#gsqlwrite]~
- ~do [#gcommcache]~
- do [#gcommtagctx]
- do [#gcommgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gcommtagctx] [recommended] Improve community search quality by carrying file/ref context into CommunityMember and resolving tagpath handles with community or edge evidence instead of name-only fallback; cover duplicate-name, skipped-directory, scoped, and federated fixtures with handle coverage and ambiguous-member diagnostics.
- [ ] [#gcommgate] [recommended] Add a community search quality and performance gate to metric-digest and fixtures: track communities runtime, handle coverage, stale/no-tagpath behavior, duplicate-name precision, and top-community stability across real and synthetic multi-module workloads.
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
