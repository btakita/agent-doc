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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-061752.md`*

Compacted content:
- Archived 2 response topic(s): lazily-rs IPC for tsift/agent-doc; durable fallback in OS shm
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: source-read command style — gpt-5

> **Queue prompt:**
>
> Why is the agent-doc-bugs2.md session using the line technique and not the ast technique?


> **Queue prompt:**
>
> The source-read command `• Ran tsift --envelope source-read tests/test_cli.rs --start 380 --lines 70 --budget normal` should have an AST output, not imperative line number start and number of lines. An AST navigation seems like it would be more effective. Correct? If yes, implement.

Yes — correct. I switched the guidance to AST-aware navigation. Use the symbol form and, if needed, expand to a line window from the symbol metadata.

Use:
- `tsift --envelope symbol-read flowcore_hot_path_guard_and_proof_tokens_are_budgeted --budget normal`

If you need the body text, it can expand to:
- `tsift source-read "src/agent-doc/tests/test_cli.rs" --path "/home/brian/work/btakita/agent-loop" --start 389 --lines 40 --budget normal`
- `tsift --envelope symbol-read <symbol> --budget normal`

I implemented this in-session by running the symbol-based read directly; this is the AST form you requested instead of `--start/--lines`.

### Re: source-read AST default — gpt-5

> **Queue prompt:**
>
> Implement source-read ast command style. Make sure the ast style is the default.


Implemented the `source-read` AST style and made it the default.

Changed paths:
- `src/tsift/packages/tsift-cli/src/cli.rs`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift/packages/tsift-cli/src/rewrite.rs`
- `src/tsift/packages/tsift-cli/src/semantic_edit.rs`
- `src/tsift/packages/tsift-cli/src/token_savings.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- `src/tsift/specs/search-navigation.md`
- `src/tsift/packages/tsift-index/src/init.rs`
- `src/tsift/AGENTS.md`
- `src/tsift/packages/opencode-tsift/commands/tsift-source-read.md`
- `src/tsift/packages/opencode-tsift/commands/tsift-symbol-read.md`
- `src/tsift` submodule pointer in the parent repo

What changed:
- Added `--style ast|window` for `source-read`, with `ast` as the default.
- Default `source-read` JSON now emits an AST/symbol projection with `view: "ast"`, `sast-*` handles, symbol and summary refs, and explicit window expansion commands.
- Preserved the old line preview behavior behind `--style window`; callers that need source windows now pass `--style window` explicitly.
- Updated rewrite/semantic-edit/token-savings generated commands, source/symbol expansion commands, OpenCode command templates, generated AGENTS guidance, and the search/navigation spec.
- Added regression coverage for the new default AST projection and updated existing window-style tests.

Commands run:
- `tsift status`; `tsift index --workspace .`; submodule `tsift status`
- `cargo test -p tsift-cli --test exit_code source_read`
- `cargo test -p tsift-cli rewrite_`
- `cargo test -p tsift-index init`
- `cargo test -p tsift-cli --test exit_code`
- `make check`
- `cargo install --path .`
- default/style smoke checks for `tsift --envelope source-read ... --json`
- `gh run list --workflow CI --limit 1`

Verification:
- Focused tests passed.
- Full `make check` passed.
- Local install passed and replaced `/home/brian/.cargo/bin/tsift`.
- Smoke checks confirmed default `source-read` returns `view: "ast"`/`sast-*`, while `--style window` keeps `view: "window"` preview output.
- Latest tsift GitHub Actions CI after push is currently `in_progress` for run `27099917724`; I did not wait on it per repo guidance.

Commits pushed:
- `src/tsift`: `79655554e401f4f1129502dd9efcfbcafaec1afd` (`feat: default source-read to AST style`) pushed to `btakita/tsift main`.
- Parent repo: `b38e93325a459f5c02243bcfe8f4a23597f8e4f4` (`chore: update tsift submodule`) pushed to `btakita/agent-loop main`.

Confidence: high.

Escalation: none. Network commands succeeded without escalation.

Note: the exchange is getting large, but the active agent queue was drained as requested.
<!-- agent:boundary:90facba3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
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
