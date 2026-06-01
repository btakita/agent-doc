---
agent_doc_session: 4602f467-ef2a-4094-a750-5eb533368988
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
v0.12.0 published to crates.io — tagpath-core split release complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260601-042332.md`*

Compacted content:
- Archived 2 response topic(s): publish 0.12.0; publish 0.12.1
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
Can tagpath benefit from using lazily-rs?

### Re: Can tagpath benefit from using lazily-rs? — gpt-5

Yes, but I would use it surgically.

The best fit is not `tagpath-core` parsing, aliasing, prose, or query normalization. Those are already pure, cheap, deterministic functions, and adding a reactive runtime there would mostly add conceptual weight. Keep the split core small.

The useful fit is the project/session layer: index freshness, incremental extraction, ontology loading, derived family maps, MCP queries, and `tagpath watch`. Those are already a dependency graph in practice:

- inputs/cells: `.naming.toml`, source file hashes/mtimes, `.naming/tags/*.md`, feature/grammar availability
- slots: resolved config, source list, per-file extraction, grouped families, ontology refs, search results, lint findings
- effects: write `.naming/index.json` / sidecar, emit watcher NDJSON, surface MCP rebuild notices

`lazily-rs` could simplify that layer by making invalidation explicit and pull-based: file changes clear dependent slots, but expensive derived values only recompute when a command, watcher event, or MCP tool actually reads them. Its batching also maps well to editor save bursts and watcher debounce windows.

The main caution is that `lazily-rs` is currently single-threaded (`RefCell`, thread-local tracking). That is fine for a request-scoped MCP query context or a single-threaded watcher event loop, but it should not become the substrate for future parallel extraction unless lazily grows a `Send`/`Sync` design or tagpath wraps it behind a single owner thread.

My recommendation: do not refactor the current indexer wholesale. If we pursue it, start with an internal `ProjectSession` prototype behind the root `tagpath` crate, not `tagpath-core`: model config/source/ontology as cells, model extracted families and indexed search as slots, then compare complexity and timings against the current `index --update` and `watch` paths. Adopt it only if it reduces the hand-rolled invalidation logic without regressing the fast sidecar/no-op path.

Proof notes: changed paths are limited to this session document; no repo code was changed. I inspected `src/tagpath/README.md`, `src/tagpath/SPEC.md`, `src/tagpath/Cargo.toml`, `src/tagpath/src/index/mod.rs`, `src/tagpath/src/watch/mod.rs`, `src/tagpath/src/index/sidecar.rs`, plus the `lazily-rs` README/SPEC/Cargo/API surface. No build or tests were run because this was an architecture answer, not an implementation change. Escalation: none.
<!-- agent:boundary:206339a8 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#p6adfmerge] Merge agent-doc feature branch 20260422/codex-agent-backend to main: the p6adf commit (845ddfd6) and prior p4*/p5* prep ride on this branch. agent-loop superproject already tracks feature-branch commits, but agent-doc main is behind and will need a sweep before any external consumer pulls.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
