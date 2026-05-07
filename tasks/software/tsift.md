---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
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
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260507-210623.md`*

Active backlog:
- [ ] [#tont] [recommended] Load tagpath ontology refs in tsift context-pack and summary-ref surfaces so stable domain tag docs can be referenced by handle/path instead of repeated prose.
- [ ] [#tfam] Replace tsift's local tag alias/query normalization helpers with tagpath v0.6.1 family/query APIs so search, explain, session-review, and context-pack share one canonical family model.
- [ ] [#tfxt] Add a tsift-side fixture-backed token-savings regression test using tagpath's fixtures/tsift-token-savings.json for search, explain, and session-review previews before expanding the contract to context-pack.

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#tont]. spec-test-build-install-commit-push
### Re: do #tont — gpt-5

Completed `#tont`.

Implemented tagpath ontology references in `tsift context-pack`: the command now loads `.naming/tags/*.md` once, attaches compact `ontology_refs` to visible context-pack symbol refs and summary refs, and emits a deduplicated top-level ontology ref list. The refs include stable handles plus tag/path/title/domain metadata and intentionally omit ontology prose so domain docs can be expanded by path only when needed.

Updated `SPEC.md` and `VERSIONS.md`, added preview-builder coverage, and extended the compiled `context-pack --json` integration test.

Verification:
- `cargo test context_pack`
- `make check`
- `cargo build`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.41`

Commits:
- `src/tsift`: `473dcfe feat: add ontology refs to context packs` pushed to `btakita/tsift main`
- parent repo submodule pointer: `1930283fa chore: update tsift submodule`

After this agent-doc closeout commit, I will push the parent repo branch so the session document and submodule pointer are on `origin/main` as well.
<!-- agent:boundary:522d5b85 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tfam] Replace tsift's local tag alias/query normalization helpers with tagpath v0.6.1 family/query APIs so search, explain, session-review, and context-pack share one canonical family model.
- [ ] [#tfxt] Add a tsift-side fixture-backed token-savings regression test using tagpath's fixtures/tsift-token-savings.json for search, explain, and session-review previews before expanding the contract to context-pack.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
