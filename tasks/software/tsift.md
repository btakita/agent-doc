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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-143243.md`*

Compacted content:
- Archived 7 response topic(s): kx6s; qszc; qs5v; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent-doc:exchange-start -->

> **Queue prompt:**
>
> [#pcache-measure]

### Re: pcache-measure - gpt-5

Implemented the prompt-cache effectiveness measurement gate in `src/tsift` and pushed it through local and remote verification.

What changed:
- Added `fixtures/real-session-prompt-cache-effectiveness.json` with two real-session-style prompt-cache cases covering Codex warm-prefix reuse and Claude explicit breakpoint reuse.
- Added fixture/report support in `tsift-agent-doc::session_cost`, including fail-closed checks for cached-input ratio, net cached tokens, read/create regressions, empty fixtures, empty case input, missing prompt-cache plans, and missing analytics.
- Extended `tsift-cli session-cost` with `--fixture` and `--fail-under`; fixture mode rejects incompatible `--input` / `--source` usage and exits non-zero when `--fail-under` sees a failing case.
- Documented the fixture gate in `VERSIONS.md`, `specs/digests-sessions.md`, and `specs/output-formats.md`.

Verification:
- `cargo test -p tsift-agent-doc prompt_cache_effectiveness_fixture -- --nocapture`
- `cargo test -p tsift-cli cli_parses_session_cost_command -- --nocapture`
- `cargo test -p tsift-cli --test exit_code session_cost_prompt_cache_fixture_passes_fail_under -- --nocapture`
- `cargo run -p tsift-cli -- session-cost --fixture fixtures/real-session-prompt-cache-effectiveness.json --fail-under --json`
- `cargo test -p tsift-agent-doc`
- `cargo test -p tsift-cli`
- `git diff --check`
- `make check`
- GitHub Actions CI run `27214324204` passed for commit `b54bb88`.

Commits:
- `src/tsift`: `b54bb88 session-cost: gate prompt cache effectiveness`
- parent repo: `0be3766c0 tsift: advance prompt cache effectiveness gate`

Escalation: none.
Confidence: high.
<!-- agent-doc:exchange-end -->
<!-- agent:boundary:324a8449 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#pcache-measure]~~
- [#pcache-fingerprint]
- [#pcache-boundary]
- [#pcache-adapters]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#pcache-fingerprint] Prompt caching: emit per-call stable-prefix fingerprints plus provider/cache-key/breakpoint metadata so cache misses are attributable.
- [ ] [#pcache-boundary] Prompt caching: refactor agent-doc prompt assembly into stable cached prefix blocks and volatile suffix blocks, keeping queue/status/compaction churn after the cache boundary.
- [ ] [#pcache-adapters] Prompt caching: enforce provider-specific adapter behavior for Anthropic cache_control, OpenAI prompt_cache_key, and replica-local routing affinity.
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
