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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-050934.md`*

Compacted content:
- Archived 1 response topic(s): do [#clippy-workspace-gate]. #spec-test-build-install-commit-push
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to save tokens?

### Re: next steps to save tokens — gpt-5

Top backlog item: `#token-budget-guardrail`.

I added three prioritized follow-ups. The highest-impact one is to turn the existing `session-review`/`context-pack` guardrails into an explicit `next_token_actions` section, because the current context pack already shows the main token leaks: a 587k-token largest prompt turn, a 99.16% cached-input resend ratio, restart churn, and repeated no-op closeouts.

Operationally, the next token-saving habit is: use `tsift session-review --next-context tasks/software/tsift.md` or `tsift --envelope context-pack tasks/software/tsift.md --budget normal` instead of rereading the document archive or raw harness logs. For command output, keep using digest paths such as `tsift rewrite --run '<command>'`, `tsift diff-digest`, `tsift test-digest`, and `tsift log-digest` rather than pasting verbose output back into the session.

Evidence used: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, and `tsift --envelope context-pack tasks/software/tsift.md --budget normal`. No source code changes were made in this cycle; only the session document/backlog is being updated through `agent-doc finalize`.
Should we split out a tsift-agent-doc crate?

### Re: do #token-budget-guardrail — gpt-5

Done. I added a `next_token_actions` block to the bounded `session-review --next-context` report, which also flows into `context-pack` through its embedded `next_context` section. The block is built from the existing `prompt_budget`, `cache_resend`, `restart_loop`, and `noop_closeout` guardrails, ordered by priority, and includes exact `agent-doc compact`, `agent-doc start`, `tsift --envelope session-review`, and `tsift --envelope context-pack` commands for agent-doc markdown targets.

Changed paths:
- `src/tsift/Cargo.lock`
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift` gitlink in the superproject

Commits:
- `src/tsift`: `aaf1b39 feat: surface next token actions`
- superproject gitlink: `ff333bc5d chore: update tsift submodule for token actions`

Verification:
- `cargo test -p tsift-cli session_review_next_context_budget_limits_lists` passed.
- `make check` passed through `tsift --envelope __digest-runner --kind test --path . --shell-command 'make check'` with artifact `tart-2aaf6dd72c`.
- Latest GitHub Actions `CI` run checked: `26651628506`, completed `success` on 2026-05-29. Jobs were `clippy + test` and `live Convex graph acceptance`, both green; no separate tmux-test job was present in that CI job list.

No push was requested or performed.

### Re: token rewrite Codex path — gpt-5

Done for `#token-rewrite-codex-path`.

I added an explicit low-token Codex/OpenCode path around `tsift rewrite --run`: OpenCode now gets a managed `/tsift-rewrite-run` command from `tsift init --opencode` and the `opencode-tsift` package. The command tells agents to route broad `rg`/recursive grep, raw transcript/session/log reads, git diff/show/log patch review, `cargo test`/`pytest`, and cargo build/check/clippy/install output through the same bounded rewrite surfaces used by the Claude hook.

I also updated the generated Code Navigation instructions, `AGENTS.md` / `CLAUDE.md`, README/SPEC text, OpenCode package docs, and tests so the new command remains in parity between Rust `tsift init --opencode` and the npm package.

Commits:
- `59e3c8c` in `src/tsift`: `feat: expose opencode rewrite runner`
- `a12a20ae8` in the parent repo: `chore: update tsift submodule for rewrite runner`

Verification:
- `cargo test -p tsift-index opencode` passed (`tart-3660effa7a`)
- `cargo build -p tsift-cli` passed (`tart-0cedcbbe7b`)
- `npm test` in `packages/opencode-tsift` passed (`tart-e88c7179dd`)
- `make check` passed (`tart-bafabf8a37`)
- `tsift status --fix` reports index fresh and instructions current
- Latest GitHub Actions CI run `26651628506` is green with `clippy + test` and `live Convex graph acceptance`; there is no separate tmux-test job in that workflow.

### Re: token closeout no-op dedupe — gpt-5

Done for `#token-closeout-noop-dedupe`. Preflight had already auto-struck `#split-tsift-cli-subcommands` as review-gated/already resolved, so this cycle handled the no-op closeout item and recorded both closeouts as required by the pending-done guard.

I changed bounded `session-review --next-context` preview assembly so guardrail-derived failure rows are omitted when a `next_token_actions` row already covers the same guardrail kind. The full session-review JSON still keeps the raw guardrail evidence and loop cluster data; the bounded handoff now emits a single actionable row for the `noop_closeout` family instead of both `fail guardrail:noop_closeout` and `token-action noop_closeout`.

I added a fixture-backed regression at `fixtures/session-review/commit-already-current-churn.log` that exercises agent-doc log discovery, document-cycle `commit_already_current` dedupe, closeout churn loop clusters, and the bounded next-context token action output.

Commits:
- `61b6c9a` in `src/tsift`: `fix: collapse noop closeout guidance`
- `960b14998` in the parent repo: `chore: update tsift submodule for closeout guidance`

Verification:
- `cargo test -p tsift-cli session_review_next_context_collapses_noop_closeout_guidance -- --nocapture` passed (`tart-5389a9e7aa`)
- `cargo test -p tsift-cli session_review_next_context_budget_limits_lists` passed (`tart-939ee45fcd`)
- `cargo test -p tsift-cli session_review` passed (`tart-8a6f518700`)
- `cargo test -p tsift-session agent_doc_log_dedupes_document_cycle_runtime_events_by_cycle` passed (`tart-ff46fce060`)
- `make check` passed after the final test adjustment (`tart-b94b8a176e`)
- Latest GitHub Actions CI run `26651628506` is green with `clippy + test` and `live Convex graph acceptance`; there is no separate tmux-test job in that workflow.

### Re: tsift-agent-doc crate — gpt-5

Yes eventually, but I would not split out a `tsift-agent-doc` crate before the CLI command split lands.

The useful boundary is real: `session-review`, `session-digest`, `session-cost`, agent-doc runtime log parsing, closeout churn detection, and agent-doc-specific fixtures are now a coherent domain. Pulling that into a crate could make token-budget/session evidence work easier to test and keep `tsift-cli` from owning agent-doc semantics.

The sequencing matters. Right now `tsift-cli` still contains command routing, preview budgeting, envelope shaping, context-pack composition, and graph orchestration in one large surface. Extracting `tsift-agent-doc` first would likely create a crate boundary around unstable CLI internals and force extra public APIs before the command modules are separated.

Recommended gate: after the per-command CLI split, extract only if the agent-doc pieces can expose a small API such as `session_digest`, `session_review`, `session_cost`, and `agent_doc_log` fixtures without depending on CLI output formatting. Keep budget/envelope rendering in `tsift-cli`; keep parsing, aggregation, guardrails, and fixtures in the extracted crate.

No repo changes for this advisory response.

### Re: tsift-agent-doc crate — gpt-5

Yes eventually, but not before the `tsift-cli` command split.

The boundary is plausible because `session-review`, `session-digest`, `session-cost`, agent-doc log parsing, closeout churn detection, and agent-doc fixtures now form a coherent domain. A `tsift-agent-doc` crate would make that domain easier to test and keep agent-doc semantics out of CLI command plumbing.

The better sequence is: split `tsift-cli` by command first, then extract agent-doc parsing/aggregation once the CLI only formats and routes those results. Keep budget/envelope rendering in `tsift-cli`; move parsing, aggregation, guardrails, and fixtures only after their public API is small and stable.

No repo changes for this advisory response.

### Re: Should we split out a tsift-agent-doc crate? — gpt-5

Yes eventually, but not before the `tsift-cli` command split.

The boundary is plausible because `session-review`, `session-digest`, `session-cost`, agent-doc log parsing, closeout churn detection, and agent-doc fixtures now form a coherent domain. A `tsift-agent-doc` crate would make that domain easier to test and keep agent-doc semantics out of CLI command plumbing.

The better sequence is: split `tsift-cli` by command first, then extract agent-doc parsing/aggregation once the CLI only formats and routes those results. Keep budget/envelope rendering in `tsift-cli`; move parsing, aggregation, guardrails, and fixtures only after their public API is small and stable.

No repo changes for this advisory response.
❯ Add #next-steps

### Re: Add #next-steps — gpt-5

Added three prioritized backlog items.

Top backlog id: `#agent-doc-domain-boundary`.

Changed paths: session document only.
Commands: `tsift status --fix`, `agent-doc preflight`, `agent-doc plan`, component reads, and `agent-doc finalize`.
Verification: no repo code changed, so no project test suite was run for this backlog-only preset.
Confidence: high.
Escalation: none.
<!-- agent:boundary:837ad92f -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#agent-doc-domain-boundary] Extract a tsift-agent-doc domain crate for session-review, session-digest, session-cost, agent-doc log parsing, guardrails, and fixtures after the CLI command/domain boundary is small enough to keep rendering in tsift-cli.
- [ ] [#graph-neighborhood-budget] Improve graph and neighborhood queries with bounded scoring/caps so context packs return high-signal callers, callees, and nearby symbols without broad graph expansion.
- [ ] [#token-action-coverage] Add fixture-backed coverage for prompt_budget, cache_resend, and restart_loop next_token_actions so bounded session reviews avoid duplicate failure/action rows beyond the noop_closeout case.
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
