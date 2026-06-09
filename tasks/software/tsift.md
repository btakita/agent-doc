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
> [#pcache-fingerprint]


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
<!-- agent-doc:exchange-start -->
### Re: pcache-fingerprint - gpt-5

Implemented prompt-cache attribution metadata in `src/tsift` and pushed it through local and remote verification.

What changed:
- Added `prompt_cache_metadata` to session-cost usage samples and prompt-cache timeline entries. Each parsed Claude/Codex call now carries provider, optional cache key, stable-prefix fingerprint, explicit breakpoint paths, and optional routing affinity.
- Added deterministic, dependency-free stable-prefix fingerprints derived from explicit fingerprint fields when present, otherwise provider/cache-key/stable-prefix/breakpoint metadata.
- Extended compact and human `session-cost` output with `prompt-cache-call` / cache metadata lines so cache misses can be compared without requiring JSON parsing.
- Enriched `fixtures/real-session-prompt-cache-effectiveness.json` with representative provider/cache-key/routing/breakpoint fields so the existing fail-under fixture also exercises attribution metadata.
- Documented the metadata surface in `VERSIONS.md`, `specs/digests-sessions.md`, and `specs/output-formats.md`.

Verification:
- `cargo test -p tsift-agent-doc prompt_cache_timeline_emits_attribution_metadata -- --nocapture`
- `cargo test -p tsift-cli --test exit_code session_cost_reads_codex_token_counts_from_stdin -- --nocapture`
- `cargo test -p tsift-cli --test exit_code session_cost_reports_prompt_cache_invalidation_diagnostics -- --nocapture`
- `cargo run -p tsift-cli -- session-cost --fixture fixtures/real-session-prompt-cache-effectiveness.json --fail-under --json`
- `cargo test -p tsift-agent-doc`
- `cargo test -p tsift-cli`
- `git diff --check`
- `make check`
- GitHub Actions CI run `27217314211` passed for commit `17544ea`.

Commits:
- `src/tsift`: `17544ea session-cost: emit prompt cache fingerprints`
- parent repo: `db0246ea5 tsift: advance prompt cache fingerprinting`

Escalation: none.
Confidence: high.
<!-- agent-doc:exchange-end -->
Implemented `#pcache-boundary`.

> **Queue prompt:**
>
> [#pcache-boundary]


I added a shared `prompt_cache` renderer in `src/agent-doc` that emits a stable prefix plus an explicit `<agent_doc_prompt_cache_boundary ... />` marker before volatile turn data. The direct `run` prompt now keeps durable response contracts above that marker and moves diffs, document excerpts, prompt-target summaries, status/accretion context, and compaction guidance below it.

I also routed the Codex owner-pane queue continuation payload through the same boundary helper, so the file path and active queue head no longer occupy the first bytes of the prompt. Tests now assert those volatile queue fields, plus direct-run prompt targets and accretion diagnostics, remain after the boundary.

Commits:
- `src/agent-doc`: `486913a7` (`agent-doc: add prompt cache boundary`)
- parent: `79a024587` (`tsift: advance prompt cache boundary`)

Verification:
- `cargo test -p agent-doc-orchestration prompt_cache`
- `cargo test -p agent-doc-orchestration build_prompt_places_turn_churn_after_cache_boundary`
- `cargo test -p agent-doc-orchestration idle_queue_drain_payload_uses_owner_continuation_for_codex`
- `cargo test -p agent-doc-orchestration`
- `make check`
- Latest CI run `27218407978` (`agent-doc: add prompt cache boundary`) was in progress when checked.
<!-- agent:boundary:f32d3577 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#pcache-measure]~~
- ~~[#pcache-fingerprint]~~
- ~~[#pcache-boundary]~~
- [#pcache-adapters]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
