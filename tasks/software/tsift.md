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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260429-070822.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)

❯ #code-review
### Re: code review — gpt-5

Reviewed `src/tsift` with `cargo test` and `cargo clippy` and focused on indexing, search, lock handling, and summaries workflows.

#### Findings

**1. Summary extraction is not concurrency-safe and can duplicate API spend**
`cmd_summarize()` opens the writable summaries DB, checks `summary_db.is_current()`, then calls the model and only later commits via `replace_file()` (`src/tsift/src/main.rs:2605-2690`, `src/tsift/src/summarize.rs:181-191`). Two concurrent `tsift summarize --extract` runs on the same repo can both observe a miss, both call Anthropic for the same file, and then race last-writer-wins on the cache. In `--diff` mode they can also interleave `delete_by_file()` for removed paths with another extractor still rebuilding summaries. This is the highest-value workflow bug because it burns API budget and makes cache state nondeterministic.

**2. `search` still has a post-precheck read race that bypasses the resilient fallback**
`precheck_search_indexes()` validates freshness with `IndexDb::inspect_read_only()` before the main search path runs (`src/tsift/src/main.rs:3217-3233`), but `cmd_search()` then reopens the DB via raw `IndexDb::open_read_only()` for symbol hits (`src/tsift/src/main.rs:3303-3334`). If a rollback-journal lock appears after precheck but before symbol lookup, `search` can still fail with a raw SQLite lock even though the index layer already has `open_read_only_resilient()` and snapshot fallback support (`src/tsift/src/index.rs:346-395`). The existing tests cover the lock being present before precheck; they do not cover the race window after it.

**3. Summary/status read paths do not degrade the way index reads do**
`status::check_summaries()` and the summarize query/stats path call `SummaryDb::open_read_only()` directly (`src/tsift/src/status.rs:288-317`, `src/tsift/src/main.rs:2847-2855`, `src/tsift/src/summarize.rs:143-150`). Unlike the index code, summaries have no resilient read path, no snapshot fallback, and no user-facing lock diagnostics. A wedged or migrating `summaries.db` can therefore make `tsift status` and `tsift summarize --stats` or lookup queries fail even while the index side remains readable.

#### Verification

- `cargo test --manifest-path src/tsift/Cargo.toml` passed (`417` unit tests and `42` integration tests).
- `cargo clippy --manifest-path src/tsift/Cargo.toml --all-targets --all-features -- -D warnings` passed.

#### Follow-up items added to backlog.
<!-- agent:boundary:a155d62e -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#afyn] [recommended] Add a coarse summaries extraction lock or per-file lease so concurrent tsift summarize --extract runs cannot duplicate API work or race on replace/delete operations
- [ ] [#bjen] [recommended] Route tsift search symbol-hit reads through the resilient read-only helpers and add a race test where a rollback-journal lock appears after precheck
- [ ] [#8w9n] [recommended] Add resilient read-only handling for summaries.db in tsift status and summarize query paths, with degraded status/diagnostics instead of hard failure
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
