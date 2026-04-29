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
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260429-013925.md`*

❯ #code-review
### Re: code review — gpt-5

1. High: `--federated` search still leaks lexical hits from non-federated scopes. In `cmd_search`, the federated branch filters symbol hits through `federated_symbol_search`, but it hands `root.clone()` to the sift engine instead of a filtered scope set ([src/tsift/src/main.rs:3074](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3074), [src/tsift/src/main.rs:3085](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3085)). That means the AST side respects `federation_for_scope`, while the lexical side still searches the whole workspace tree, including `Private` and `Isolated` submodules. The root cause is that federation rules are only applied to index reads, not to the underlying lexical search path.

2. Medium: `tsift lint` ignores isolation tiers and aggregates entities from every discovered index in the workspace. `cmd_lint` auto-loads entities from the resolved project root ([src/tsift/src/main.rs:3375](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:3375)), and `collect_entities_from_index_path` blindly walks every reachable `index.db` under `.tsift/indexes` ([src/tsift/src/lint.rs:201](/home/brian/work/btakita/agent-loop/src/tsift/src/lint.rs:201), [src/tsift/src/lint.rs:226](/home/brian/work/btakita/agent-loop/src/tsift/src/lint.rs:226)). In a shared workspace, that can surface private symbol names in lint suggestions for unrelated markdown docs. The root cause is filesystem discovery without any config-aware scope filtering.

3. Medium: `tsift summarize` does not resolve the workspace/project root the way the other commands do. It starts from raw `canonicalize()` and builds `.tsift/summaries.db` relative to that path ([src/tsift/src/main.rs:2419](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2419), [src/tsift/src/main.rs:2420](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2420)). From a nested directory, that points summary reads/writes, `git diff`, and `find_symbols_db_for_file` at the wrong root ([src/tsift/src/main.rs:2428](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2428), [src/tsift/src/main.rs:2472](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2472)). The result is inconsistent behavior versus `search`/`status`/`lint`, and it can create a second nested summary cache or drop workspace-scoped symbol context.

4. Medium: `tsift edit` is not actually atomic even though phase 1 is labeled `atomic batch`. Validation happens up front, but phase 2 writes files sequentially with no temp-file swap and no rollback ([src/tsift/src/main.rs:1257](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:1257), [src/tsift/src/main.rs:1268](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:1268), [src/tsift/src/main.rs:1280](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:1280)). If a later write fails after earlier ones succeeded, the command returns an error after already mutating part of the repo. The root cause is treating replacement validation as if it were a transactional commit boundary.

#### Coverage Gaps

- I do not see a regression test that proves federated lexical search excludes isolated/private scopes.
- I do not see nested-path coverage for `summarize` root resolution.
- I did not run the full test suite for this review; the findings above are from code inspection.
<!-- agent:boundary:c72d1793 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#dpeh] [recommended] Restrict federated lexical search to the same eligible scope set used for federated symbol search; add regression coverage for isolated/private scopes.
- [ ] [#618r] [recommended] Make lint entity collection config-aware so workspace linting excludes private/isolated indexes unless the caller opts in.
- [ ] [#ggft] [recommended] Resolve summarize roots through the same project/workspace helper used by search/status/lint, and add nested-path coverage.
- [ ] [#35ms] [recommended] Make tsift edit transactional or explicitly non-atomic; add a regression test for partial write failure.
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:backlog -->

<!--
-->
