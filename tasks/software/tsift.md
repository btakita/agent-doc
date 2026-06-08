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
> Please add analytics to the prompt cache. I want to be able to review effective it is over time.


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
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest CI was completed/success for `add session prompt cache plan` at 2026-06-08T21:25:43Z.
- `tsift status` in `src/tsift`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `net_cached_input_tokens` gives a coarse effectiveness signal: cache reads minus creation tokens. Negative or small positive values are a warning sign even when cache reads are present.
- `total_cache_creation_tokens` and `cache_read_to_creation_ratio` expose rewrite churn. High creation with low reads means the runtime is repeatedly creating cache entries without getting enough warm reuse.
- `timeline` shows which turns had a ratio drop, so the next step is to inspect what changed at that boundary: prefix text, tools, cache key, compaction, or routing.
- `average_cached_input_ratio`, first/last ratios, and `cached_input_ratio_delta` show whether reuse is holding, improving, or degrading.
- Self-hosted or edge caches may be replica-local. Without routing affinity for the same cache key, a byte-identical prompt can still miss because it lands on a different replica.
- Compaction should preserve the live warm prefix and rewrite only the transcript tail. A cold standalone summarizer prompt effectively starts a new cache shape.
- Volatile values inside the cached prefix, such as timestamps, generated IDs, transient diagnostics, or one-off compaction instructions, should be treated as accidental invalidation sources.
- Thread or session key changes should invalidate OpenAI-style keyed reuse because the same byte-identical prefix is no longer associated with the same `prompt_cache_key`.
- Tool definition changes should invalidate the tool-prefix segment. For Anthropic-style explicit breakpoints, edits before a `cache_control` breakpoint invalidate that block and downstream cached material.
- A stable system/developer prefix change should invalidate the cache for that prefix because the bytes are no longer identical.
- `git diff --check`: passed.
- `tsift --envelope digest-runner --kind log --path . --shell-command 'cargo install --path .'`: passed, artifact `tart-99633b041d`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`: passed, artifact `tart-35d22ae8c2`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`: passed, artifact `tart-02a5c5c8a8`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-agent-doc prompt_cache_plan_summarizes_effectiveness_over_time -- --nocapture'`: passed, artifact `tart-e2cfb2ea68`.
- parent repo: `220cbf472` (`update tsift prompt cache analytics`), pushed to `origin/main`.
- tsift submodule: `7b9da33` (`add prompt cache analytics`), pushed to `origin/main`.
- parent pointer: `src/tsift`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/VERSIONS.md`
- Added library and compiled CLI regression coverage for improving, stable, and candidate uncached prompt-cache analytics.
- Updated the session digest spec and unreleased changelog entry.
- Added compact and human text output for the analytics so cache effectiveness can be reviewed without raw JSON.
- Added bounded `prompt_cache_plan.analytics` with sample count, effective flag, trend (`single_sample`, `stable`, `improving`, `declining`, `insufficient_data`), average/first/last cached-input ratios, ratio delta, net cached-read minus cache-creation tokens, optional read/create ratio, timeline truncation flag, and a first-plus-latest per-turn cache timeline.

No follow-up backlog item was created for this cycle; the implemented `prompt_cache_plan` is the tsift-owned surface, and live provider adapter calls belong to the runtime that invokes the model.

Escalation: none.

Confidence: high.

### Re: prompt cache invalidation — gpt-5

The prompt cache system should treat invalidation as a stable-prefix contract, not as an explicit `clear cache` operation owned by tsift.

For the current `session-cost` surface, tsift observes cache effectiveness from transcript usage. The live cache itself belongs to the model provider or runtime adapter. That means a cache miss usually means the runtime sent a different cache key, different routing affinity, or different prefix bytes than the provider saw on the previous warm call.

Practical invalidation rules:
- A stable system/developer prefix change should invalidate the cache for that prefix because the bytes are no longer identical.
- Tool definition changes should invalidate the tool-prefix segment. For Anthropic-style explicit breakpoints, edits before a `cache_control` breakpoint invalidate that block and downstream cached material.
- Thread or session key changes should invalidate OpenAI-style keyed reuse because the same byte-identical prefix is no longer associated with the same `prompt_cache_key`.
- Volatile values inside the cached prefix, such as timestamps, generated IDs, transient diagnostics, or one-off compaction instructions, should be treated as accidental invalidation sources.
- Compaction should preserve the live warm prefix and rewrite only the transcript tail. A cold standalone summarizer prompt effectively starts a new cache shape.
- Self-hosted or edge caches may be replica-local. Without routing affinity for the same cache key, a byte-identical prompt can still miss because it lands on a different replica.

How the new analytics help review invalidation over time:
- `average_cached_input_ratio`, first/last ratios, and `cached_input_ratio_delta` show whether reuse is holding, improving, or degrading.
- `timeline` shows which turns had a ratio drop, so the next step is to inspect what changed at that boundary: prefix text, tools, cache key, compaction, or routing.
- `total_cache_creation_tokens` and `cache_read_to_creation_ratio` expose rewrite churn. High creation with low reads means the runtime is repeatedly creating cache entries without getting enough warm reuse.
- `net_cached_input_tokens` gives a coarse effectiveness signal: cache reads minus creation tokens. Negative or small positive values are a warning sign even when cache reads are present.

The recommended backlog item `#1zax` now tracks explicit invalidation diagnostics on top of the analytics, so tsift can eventually classify likely causes instead of only reporting symptoms.

Changed paths: none in this explanation cycle.

Commands:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `tsift status` in `src/tsift`

Verification: no code changed in this cycle, so no build/test suite was rerun.

Escalation: none.

Confidence: high.
<!-- agent:boundary:035a5367 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- Please describe how cache invalidation works for our prompt cache system.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#1zax] [recommended] Add prompt-cache invalidation diagnostics that classify likely prefix/key/replica churn from analytics timeline drops, creation-token spikes, and read/create regressions.
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
