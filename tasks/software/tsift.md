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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-175219.md`*

Compacted content:
- Archived 2 response topic(s): pcache-measure - gpt-5; pcache-fingerprint - gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
- Superproject pointer: `e4c047fd9` (`agent-doc: update prompt cache replay gate`).
- `src/agent-doc`: `3e6092e9` (`prompt-cache: add replay gate metadata`), pushed to `origin/main`.
- Latest GitHub Actions `CI` run on `main` was green before closeout.
- `cargo install --path .` completed from that isolated tree and installed `agent-doc 0.34.0` locally.
- `make check` passed in an isolated sibling-layout worktree containing only this three-file patch: 4,143 tests passed, 237 skipped, doctests passed, and the medium SimWorld check passed.
- `cargo test -p agent-doc-orchestration build_prompt --lib` passed.
- `cargo test -p agent-doc-orchestration prompt_cache --lib` passed.
- Documented the replay metadata invariant in `SPEC.md`.
- Added a consecutive-turn replay test proving volatile status/queue/boundary/accretion churn changes the full prompt but preserves replay metadata, while durable response-contract changes invalidate the fingerprint and provider key.
- Added dry-run diagnostics that print the replay metadata without changing agent prompt delivery.
- Added prompt-cache replay metadata: stable-prefix SHA-256, provider cache key, cache-control policy, and routing affinity.
- superproject gitlink `src/agent-doc`
- `agent-doc-orchestration/src/run.rs`
- `agent-doc-orchestration/src/prompt_cache.rs`
- `SPEC.md`

### Re: prompt caching backlog — gpt-5

> **Queue prompt:**
>
> do [#pcache-replaygate]


Added prompt-caching follow-up backlog items in priority order. Top backlog item: `#pcache-replaygate`.

No code/test run was needed because this cycle only mutates the session backlog.

### Re: pcache-missrank — gpt-5

> **Queue prompt:**
>
> do [#pcache-missrank]


Implemented `#pcache-missrank`.

- Added prompt-cache session-cost samples plus ranked miss causes for stable-prefix fingerprint, adapter state, routing affinity, cached-input loss, and creation-token spikes.
- Added a compact `render_cache_miss_ranking` output so operators see `fingerprint`, `adapter_state`, `routing_affinity`, token deltas, and `miss_rank` without reading raw JSON.
- Wired direct-run dry-run prompt-cache diagnostics to print the session-cost line with `baseline_required` until the later history task feeds previous samples.
- Documented the session-cost ranking invariant in `SPEC.md`.

Verification:

- `cargo test -p agent-doc-orchestration prompt_cache --lib`
- `cargo test -p agent-doc-orchestration build_prompt --lib`
- `make check`
- `make install`
- `gh run list --workflow CI --limit 1` showed the latest CI run completed successfully.

Commits:

- `src/agent-doc`: `02a1317a` (`prompt-cache: rank session cost misses`), pushed to `origin/main`.
- superproject: `356c21565` (`agent-doc: update prompt cache miss ranking`), ready to push with the session closeout commit.

### Re: pcache-boundary-contract — gpt-5

> **Queue prompt:**
>
> do [#pcache-boundary-contract]


Implemented `#pcache-boundary-contract`.

- Documented the prompt-cache boundary contract in `specs/05-agent-backend.md` and tightened the top-level invariant in `SPEC.md`.
- Added module-level boundary-contract guidance in `prompt_cache.rs`: stable prefix, volatile suffix, and provider key requirements.
- Added provider breakpoint/key-material coverage for the cache boundary and replay key.
- Added a direct-run prompt test proving durable response/turn contracts stay above the boundary while diff/status/queue/accretion/context content stays below it.
- Strengthened the Codex owner-continuation test so owner-pane file path and queue head stay in the volatile suffix.

Verification:

- `cargo test -p agent-doc-orchestration prompt_cache --lib`
- `cargo test -p agent-doc-orchestration prompt_cache_boundary --lib`
- `cargo test -p agent-doc-orchestration idle_queue_drain_payload_uses_owner_continuation_for_codex --lib`
- `make check`
- `make install`
- `gh run watch 27233820687 --exit-status` passed for CI on `prompt-cache: document boundary contract`.

Commits:

- `src/agent-doc`: `3570a63c` (`prompt-cache: document boundary contract`), pushed to `origin/main`.
- superproject: `86efd465b` (`agent-doc: update prompt cache boundary contract`), ready to push with the session closeout commit.

### Re: pcache-ci-history — gpt-5

> **Queue prompt:**
>
> do [#pcache-ci-history]


Implemented prompt-cache effectiveness history for the token/performance gate.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/prompt_cache.rs`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/specs/05-agent-backend.md`

What changed:
- Added `PromptCacheEffectivenessSample` JSONL persistence with provider, harness, transcript id, observation time, and the existing session-cost sample.
- Added trend-check status and thresholds so the gate reports `baseline_required`, `pass`, or `fail` for the latest matching workload history.
- Reused the existing miss-ranking path so failures include actionable cached-input deltas, creation-token spikes, stable-prefix fingerprint changes, adapter-state changes, and routing-affinity changes.
- Covered Codex/OpenAI and Claude/Anthropic real-transcript sample flows in focused tests, including baseline-required, pass, token-regression fail, and fingerprint-regression fail cases.
- Updated the prompt-cache specs to require JSONL history and operator-readable trend failures.

Verification:
- `cargo test -p agent-doc-orchestration prompt_cache --lib`
- `make check`
- `make install`
- `gh run watch 27234883466 --exit-status` passed for `9959a6e8`

Commit / push:
- `src/agent-doc`: `9959a6e8 prompt-cache: persist effectiveness history` pushed to `origin/main`
- superproject: `291f77cec agent-doc: update prompt cache ci history`

Confidence: high.
Escalation: none.
<!-- agent:boundary:6cce3a0a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
