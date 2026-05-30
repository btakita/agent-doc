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
<!-- agent:boundary:ede1ccfb -->
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
- do [#graph-neighborhood-budget]
- do [#token-action-coverage]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
