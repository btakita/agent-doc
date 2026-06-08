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
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-212555.md`*

Compacted content:
- Archived 1 response topic(s): closeout repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: prompt caching — gpt-5

> **Queue prompt:**
>
> Can tsift implement Prompt Caching? See https://viktor.com/blog/how-we-built-viktor-around-prompt-caching


Yes. tsift can implement prompt caching as a deterministic planning and diagnostics layer now, while leaving live provider API mutation to whichever agent runtime calls the model. I added `prompt_cache_plan` to `tsift session-cost`: it appears when cache reads/creation are observed or when uncached prompt volume is large enough to justify cache setup.

The plan captures the Viktor-style cache shape from the linked article: stable context first, append-only thread history until an intentional compaction boundary, no volatile timestamps or random ids inside the cached prefix, compaction against the live warm prefix, and provider-specific adapter requirements for Anthropic `cache_control` breakpoints, OpenAI `prompt_cache_key`, and self-hosted/edge replica affinity.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`

Commits:
- tsift submodule: `505506f` (`add session prompt cache plan`), pushed to `origin/main`.
- parent repo: `src/tsift` already records `505506f` at `HEAD`; no separate pointer commit was needed before this agent-doc closeout.

Verification:
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`: passed, artifact `tart-02a5c5c8a8`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`: passed, artifact `tart-ffffe139d0`.
- `tsift --envelope digest-runner --kind log --path . --shell-command 'cargo install --path .'`: passed, artifact `tart-99633b041d`.
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest CI was completed/success for `test semantic ast edit edge cases` at 2026-06-08T19:08:05Z.

No follow-up backlog item was created for this cycle; the implemented `prompt_cache_plan` is the tsift-owned surface, and live provider adapter calls belong to the runtime that invokes the model.

Escalation: none.

Confidence: high.
<!-- agent:boundary:0a782a4a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- Please add analytics to the prompt cache. I want to be able to review effective it is over time.
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
