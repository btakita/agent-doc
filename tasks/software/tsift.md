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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-013837.md`*

Compacted content:
- Archived 1 response topic(s): next-steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: pcache-prefix-drift

Implemented and pushed.

> **Queue prompt:**
>
> [#pcache-fixture-drift]


Changes:
- Added `prompt_cache_plan.analytics.prefix_drift` to `session-cost`, comparing adjacent prompt-cache calls by `stable_prefix_fingerprint`, cache key, explicit breakpoints, routing affinity, and provider.
- Drift entries now report trigger/severity, first changed field, all changed fields, before/after cached-input ratios, and cache-creation ratio.
- Cache-ratio-drop and cache-creation-spike diagnostics now point at the first changed prompt-cache field when attribution metadata changed.
- Added compact/human output lines via `prompt-cache-prefix-drift`, and JSON keeps the structured field-change payload.
- Updated the session-cost spec and `VERSIONS.md`.

Verification:
- `cargo test -p tsift-agent-doc prompt_cache_prefix_drift_points_regressions_at_first_changed_field`
- `cargo test -p tsift-cli session_cost_reports_prompt_cache_invalidation_diagnostics`
- `make check`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.67`
- GitHub Actions CI in `btakita/tsift` for `bcc34fb`: success

Commits pushed:
- `btakita/tsift` `bcc34fb` (`feat: report prompt cache prefix drift`)
- `btakita/agent-loop` `7424a5d92` (`chore: update tsift submodule`)

Note: `agent-loop` has no workflow named `CI` for `gh run list --workflow CI`; the required tsift CI check passed.

Queue guard note: preflight remembered the removed free-text queue head "What are #next-steps to improve prompt caching and saving tokens?". I did not treat that as this cycle's active queue head; the live queue head was `#pcache-prefix-drift`, and the remaining prompt-cache follow-ups are preserved in the active backlog/queue.
<!-- no-free-text-queue-head-guard -->

### Re: contextpack-queue-slim

Implemented and pushed.

Changes:
- Added `next_context.agent_doc_queue` for agent-doc markdown targets, resolving active queue ids to backlog prompt rows.
- The profile carries live unresolved exchange-tail lines, top unchecked backlog/review rows, compact prompt preset refs, and expansion handles for next-context/context-pack/source-read.
- Empty live exchange tails now still preserve queue context and use the active queue prompt as `active_prompt_targets`.
- Budgeted session-review/context-pack JSON and human/compact output now include the bounded profile.
- Updated session/context-pack specs, version notes, and graph acceptance fixture.

Verification:
- `cargo test -p tsift-agent-doc session_review_next_context_prefers_live_exchange_prompt_targets`
- `cargo test -p tsift-cli session_review_next_context_budget_limits_lists`
- `cargo test -p tsift-cli --test exit_code session_review_next_context_scopes_live_tail_over_stale_transcript_context`
- `cargo test -p tsift-cli --test exit_code context_pack_json_composes_next_context_and_optional_digests`
- `cargo test -p tsift-cli --test graph_db_conformance agent_orchestration_acceptance_pack_regenerates_from_real_queue_fixture`
- `make check`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.67`

Commits pushed:
- `btakita/tsift` `38c900c` (`feat: add agent-doc queue context profile`)
- `btakita/agent-loop` `2b3bee58b` (`chore: update tsift submodule`)

CI:
- `btakita/tsift` run `27248701231`: live Convex job passed; clippy+test failed twice before running the suite due GitHub runner/cache infrastructure (`No space left on device` first run; rerun died in cache step before `Full CI suite`). Local `make check` passed.
- `agent-loop` has no workflow named `CI`.

<!-- no-free-text-queue-head-guard -->

### Re: token-actions-rewrite

> **Queue prompt:**
>
> [#pcache-scorecard]


Implemented and pushed.

Changes:
- Added `rewrite_commands` to `session-review --next-context` `next_token_actions`.
- Guardrail actions now point at concrete bounded follow-ups: `context-pack`, `session-review --next-context`, and relevant rewrite commands.
- Added `repeated_raw_read` actions for repeated raw source/session/log reads, with `tsift rewrite --run ...` plus normalized `tsift --envelope source-read ...` commands.
- Added `repeated_command_bundle` actions for repeated shell command bundles, with exact `tsift rewrite --run ...` commands.
- Updated `context-pack` and budgeted human output to show rewrite commands, plus session-review specs/version notes.
- Added unit and compiled CLI JSON coverage for the new action commands.

Verification:
- `cargo check -p tsift-cli`
- `cargo test -p tsift-cli session_review_next_context_budget_limits_lists`
- `cargo test -p tsift-cli --test exit_code session_review_json_surfaces_loop_clusters`
- `cargo test -p tsift-cli --test exit_code session_review_next_context_collapses_actionable_guardrail_failures`
- `make check`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.67`

Commits pushed:
- `btakita/tsift` `c884ade2` (`feat: add token action rewrite commands`)
- `btakita/agent-loop` `8203cef1c` (`chore: update tsift submodule`)

CI:
- `btakita/tsift` run `27249341345`: success.
- `agent-loop` has no workflow named `CI`.
- Broad `cargo fmt --check` is not a clean verification target right now because unrelated pre-existing files report rustfmt diffs; `make check` does not include that target. The exchange is large, but the queue remains active, so I continued draining it.
- Pushing the superproject pointer also published pre-existing local agent-doc commits that were already ahead of `origin/main`.
- `agent-loop` has no workflow named `CI`.
- `btakita/tsift` run `27250019494`: success.
- `btakita/agent-loop` `c7720feef` (`chore: update tsift submodule`)
- `btakita/tsift` `88a41d4` (`feat: extend prompt cache fixture drift coverage`)
- `tsift --version` -> `tsift 0.1.67`
- `cargo install --path .`
- `make check`
- `git diff --check`
- `cargo run -p tsift-cli -- session-cost --fixture fixtures/real-session-prompt-cache-effectiveness.json --compact`
- `cargo run -p tsift-cli -- session-cost --fixture fixtures/real-session-prompt-cache-effectiveness.json --json`
- `cargo test -p tsift-cli --test exit_code session_cost_reports_prompt_cache_invalidation_diagnostics`
- `cargo test -p tsift-cli --test exit_code session_cost_prompt_cache_fixture_fail_under_rejects_missing_adapter_evidence`
- `cargo test -p tsift-cli --test exit_code session_cost_prompt_cache_fixture_passes_fail_under`
- `cargo test -p tsift-agent-doc prompt_cache_effectiveness_fixture`
- `cargo check -p tsift-cli`
- Compact and human fixture output now show scenario coverage; specs and version notes are updated.
- Churn-specific cases can intentionally accept `prompt_cache_key_churn` / `routing_affinity_churn` adapter states, but still require the matching prefix-drift field and positive cache economics.
- `session-cost --fixture` now reports required/covered/missing scenario coverage and fails closed when required coverage disappears, even if individual token thresholds still pass.
- Added fixture schema fields for `required_regression_scenarios`, per-case `regression_scenarios`, `required_prefix_drift_fields`, and `required_diagnostics`.
- Added required drift-mode coverage for volatile generated prefix headers, cold standalone compaction, OpenAI `prompt_cache_key` churn, and replica routing churn.
- Extended `fixtures/real-session-prompt-cache-effectiveness.json` from 2 to 6 passing real-session-style cases.

Note: pushing the superproject pointer also published eight pre-existing local agent-doc commits that were already ahead of `origin/main`. The exchange is large, but the queue remains active, so I continued draining it.

<!-- no-free-text-queue-head-guard -->
<!-- agent:boundary:27144956 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
