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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-023941.md`*

Compacted content:
- Archived 2 response topic(s): arXiv 2605.05242 implications; DCI benchmark
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#s1k1]. spec-test-build-install-commit-push
### Re: #s1k1 search workflow — gpt-5

Implemented and pushed `src/tsift` commit `1078801` (`feat: add search workflow recipe`) to `origin/main`.

What changed:
- Added `tsift workflow search`, with human, JSON, and envelope-compatible output for an ordered exact search -> semantic search -> explain -> summarize -> digest workflow.
- The recipe explicitly preserves handles across expansions: `sfam-*`, `shit-*`, `edef-*`, `ecall-*`, `eces-*`, artifact handles, touched-symbol handles, and `resume_commands[]`.
- Documented the workflow in `SPEC.md` and added it to generated `tsift init` code-navigation instructions.
- Added unit coverage for the workflow recipe and integration coverage for `tsift workflow search --json`.

Verification:
- `cargo test` passed: 556 unit tests and 90 integration tests.
- `make check` passed: `cargo clippy -- -D warnings` plus the full test suite.
- `cargo build` passed.
- `cargo install --path .` passed and replaced `/home/brian/.cargo/bin/tsift`.
- Installed smoke check passed: `tsift workflow search --json` emitted the expected ordered recipe.
- Latest GitHub Actions CI run `25774327358` is green (`clippy + test`). This repo's CI workflow has no separate tmux-test job.

I left unrelated dirty parent-repo entries untouched. The `src/tsift` submodule worktree itself is clean at the pushed commit.
<!-- agent:boundary:b512dd89 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#v6y3] Add breadth/scale guards to search envelopes: high-hit warnings, corpus-size/tool-budget signals, and concrete next-narrowing commands before agents fan out too broadly.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-12 [#686r] Add a DCI-style tsift benchmark: compare exact/chained rg, lexical BM25, and hybrid search on multi-hop code/repo tasks, tracking localization, tool calls, latency, and token budget.
- 2026-05-12 [#s1k1] Add a composable search workflow surface or documented recipes that preserve result handles across search --exact, search, explain, summarize, and digest expansion steps.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
