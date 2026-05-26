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
❯ re [#f14h]: The auto-update should primarily be driven in the development environment. A github action can be added but the primary driver is through actions on a development machine. Can we make this process automatic? Can it be part of the tsift cli tool call chain?
### Re: #f14h automatic graph refresh — gpt-5

Yes. I made the development-machine auto-update path part of the `tsift` CLI chain.

What changed:

- `tsift graph`, `tsift communities`, `tsift path`, and `tsift explain` now resolve the local/scoped index target and incrementally refresh missing or stale indexes before opening `index.db`.
- If another live tsift writer already owns `index.lock` and an older database exists, those graph-query commands now skip the refresh with a concise stderr note and continue against the current read-only snapshot instead of failing closed.
- Agent-doc task paths named after workspace scopes, such as `tasks/software/tsift.md`, now resolve to that scope's index for read-only graph queries too, matching the existing search behavior.
- `SPEC.md` now documents search/default autoindex behavior plus the new graph-query freshness chain.

Changed paths in `btakita/tsift`:

- `src/main.rs`
- `SPEC.md`

Verification:

- `cargo test -q autoindexes`
- `cargo test -q writer_lock_is_held`
- `cargo test -q agent_doc_task_path_infers_matching_workspace_scope`
- `cargo test -q autoindexes_missing_index_by_default`
- `cargo test -q graph_cmd_autoindexes_stale_index_by_default`
- `cargo build`
- `cargo install --path .`
- `make check`
- GitHub CI run `26461951197` passed for `btakita/tsift` with jobs `clippy + test` and `live Convex graph acceptance`. The only annotations were the existing Node.js 20 deprecation and live Convex URL note. This repo has no separate tmux workflow; `gh workflow list` shows `CI` and `Release`.

Commits:

- `btakita/tsift` pushed `6e3d6d0` (`feat: auto-refresh graph query indexes`).
- Parent repo pointer committed locally as `bd533c9b3` (`chore: update tsift graph autoindex`). I did not push parent `main` because it already has unrelated local commits ahead of `origin/main`; pushing would publish those too.
<!-- agent:boundary:91bd46d1 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
