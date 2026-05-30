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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-061203.md`*

Compacted content:
- Archived 8 response topic(s): next steps to save tokens; do #token-budget-guardrail; token rewrite Codex path; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: agent-doc domain boundary — gpt-5

Completed `#agent-doc-domain-boundary`.

Changed paths:
- `src/tsift/packages/tsift-agent-doc/` added as the new domain crate for `session_cost`, `session_digest`, and `session_review`.
- `src/tsift/packages/tsift-session/` reduced to a compatibility shim that re-exports the new crate and forwards `test-support`.
- `src/tsift/Cargo.toml`, `src/tsift/Cargo.lock`, `src/tsift/src/lib.rs`, and `src/tsift/packages/tsift-cli/Cargo.toml` updated so root `tsift` depends on/re-exports `tsift-agent-doc` directly and `tsift-cli` no longer carries the redundant `tsift-session` dependency.
- `src/tsift/SPEC.md` and `src/tsift/VERSIONS.md` updated for the new boundary.
- Superproject `src/tsift` submodule pointer updated to the pushed tsift commit.

Commits:
- `src/tsift`: `1f04724` (`tsift(#agent-doc-domain-boundary): extract agent-doc session crate`) pushed to `btakita/tsift:main`.
- Superproject pointer: `13fed0648` (`tsift: update submodule for agent-doc domain crate`) prepared locally; it will be pushed after this document finalize step.

Verification:
- `cargo fmt --package tsift-agent-doc --package tsift-session --package tsift --check`
- `cargo check --workspace`
- `cargo test --package tsift-agent-doc --package tsift-session --package tsift-sim-world`
- `cargo clippy --package tsift-agent-doc --package tsift-session --package tsift --all-targets -- -D warnings`
- `make check` after the final manifest cleanup: workspace clippy, workspace tests, OpenCode plugin tests, and npm publish dry-run all passed.
- `cargo build --workspace`
- `cargo install --path packages/tsift-cli --force`
- `tsift --version` => `tsift 0.1.62`
- Latest `btakita/tsift` GitHub Actions `CI` run before this push was green (`26651628506`, 2026-05-29). The tsift workflow has no `tmux`/`tmux-ci` job or Makefile target, so there was no separate tmux leg to inspect.

Escalation: none.

Confidence: high. The move is a crate-boundary split with unchanged module bodies except rustfmt import ordering in `session_review`; existing digest/cost/review and SimWorld coverage moved with the crate and passed.

### Re: #graph-neighborhood-budget — gpt-5

Implemented `#graph-neighborhood-budget` in `src/tsift`.

What changed:
- `graph-db related` now score-ranks semantic-seeded incident/outgoing expansion before applying per-node edge caps and node discovery caps, so semantic/source/caller/callee links survive high-degree neighborhoods ahead of low-signal edges.
- `graph-db neighborhood` keeps stable node-id pagination unchanged, but caps additive `ranked_neighbors` to a score-sorted preview and reports that cap in the ranking-gate diagnostics.
- `tsift traverse <node> --limit N` now queues scored incident neighbors first, instead of allowing lexicographic neighbor order to decide tight caps.
- Added regression coverage for score-before-cap semantic expansion, capped traversal-neighborhood selection, and capped ranked-neighbor scoring. Updated `SPEC.md` and `VERSIONS.md`.

Verification:
- `cargo test -p tsift-resolution ranked_neighbors_capped_truncates_after_scoring`
- `cargo test -p tsift-cli graph_db_semantic_seeded_neighborhood_scores_before_caps`
- `cargo test -p tsift-cli traversal_neighborhood_handles_prioritizes_high_signal_edges_when_limited`
- `cargo test -p tsift-cli graph_db_api_queries_sqlite_neighborhood_and_schema`
- `cargo clippy -p tsift-resolution -p tsift-cli --all-targets -- -D warnings`
- `make check`
- `cargo build --workspace`
- `cargo install --path packages/tsift-cli --force`
- `tsift --version` -> `tsift 0.1.62`
- GitHub CI `26677186684` passed after rerunning a failed runner job; the initial failed attempt was GitHub runner disk exhaustion (`No space left on device`), not a code/test failure.

Commits:
- `src/tsift`: `cd10a3a tsift(#graph-neighborhood-budget): bound graph neighborhood previews` (pushed)
- parent repo: `1a0903191 tsift: update graph neighborhood budget implementation` (submodule pointer only; parent push follows this document closeout)

I also checked for tmux CI coverage in this repo; there is no `tmux` workflow or `make tmux-ci` target in `src/tsift`.

### Re: #token-action-coverage — gpt-5

Implemented `#token-action-coverage` in `src/tsift`.

Added fixture-backed CLI coverage for `session-review --next-context` token actions:
- `fixtures/session-review/token-action-guardrails.codex.jsonl` exercises Codex token-count guardrails for `prompt_budget` and `cache_resend`.
- `fixtures/session-review/restart-loop.log` exercises agent-doc restart churn for `restart_loop`.
- `packages/tsift-cli/tests/exit_code.rs` now asserts those three actionable guardrails appear once in `next_token_actions` and are collapsed out of bounded envelope `unresolved_failures`, matching the existing `noop_closeout` behavior.
- `VERSIONS.md` records the Unreleased note.

Committed and pushed tsift submodule commit `86b4a537 tsift(#token-action-coverage): cover session token actions`, then committed the parent submodule pointer as `9c1103632 tsift: update token action coverage`.

Verification:
- `cargo test -p tsift-cli session_review_next_context_collapses_actionable_guardrail_failures`
- `cargo test -p tsift-cli session_review_next_context_collapses_noop_closeout_guidance`
- `make check`
- `cargo build --workspace`
- `cargo install --path packages/tsift-cli --force`
- `tsift --version` -> `tsift 0.1.62`

No `tmux-ci` target or tmux workflow exists in `src/tsift`, so there was no tmux leg to run.

### Re: closeout repair — gpt-5

Closed the leftover agent-doc queue-drain and boundary-normalization diff from the `#token-action-coverage` write-back. No additional repo work was needed.
<!-- agent:boundary:792ed211 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#agent-doc-domain-boundary]~
- ~do [#graph-neighborhood-budget]~
- do [#token-action-coverage]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
