---
agent_doc_session: tsift-v0.1
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260529-171128.md`*

Compacted content:
- Archived 3 response topic(s): do A; dispatch #spec-test-build-install-commit-push → do [#split-tsift-digest] / [#split-tsift-search] / [#demote-root-tsift]; Please evaluate the extraction
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#clippy-workspace-gate]. #spec-test-build-install-commit-push
### Re: do [#clippy-workspace-gate]. #spec-test-build-install-commit-push — opus-4-8

Done — the clippy coverage gap is closed. `make check` now lints the whole workspace, and the `tsift-cli` debt that blocked the flip is cleared. Committed, pushed, CI green.

#### The flip

`make check`'s clippy leg: `cargo clippy --all-targets` → **`cargo clippy --workspace --all-targets -- -D warnings`**. This is the lint analog of the `test:` → `--workspace` fix from `#split-sim-world` — every crate is now linted, not just root `tsift`. (Also updated the SPEC release-verification note to match.)

#### Clearing the tsift-cli debt the flip required

- **Deleted 8 orphaned `graph_db_*` wrapper fns** — `graph_db_neighborhood_depths`, `_page_handle_coverage_pct`, `_node_has_handle_coverage`, `_duplicate_name_precision`, `_has_community_signal`, `_has_semantic_signal`, `_source_handle_is_fresh`, `_edge_kind_rank_score`. Each was a one-line uncalled passthrough to `tsift::resolution::*` (zero call sites anywhere — verified), left behind when call sites switched to the library functions directly. Dead code, removed.
- **`#[cfg(test)]`-gated `traversal_relative_path_is_generated_artifact`** — its only four callers live in the lib test module, so it's now compiled only where it's used.
- **`#[allow(clippy::too_many_arguments)]` on `commands/graph.rs::cmd_explain`** — an 11-arg `#[cfg(test)]` budget-wrapper entry point; the production path goes through `cmd_explain_with_budget`.

#### Verification

- `cargo clippy --workspace --all-targets -- -D warnings` — **clean across every crate** (was 21 issues across tsift-libsql/tokensave/resolution/cli before this + the prior `#lint-cleanup-non-root-crates` pass)
- `make check` — green (now with workspace clippy)
- `cargo test --workspace` — **993 passed across 40 suites**, unchanged (deleted code was non-test; the gated fn still runs under test)
- `cargo install` + `cargo build --release -p tsift-cli` — both binaries refreshed, `tsift --version` → `0.1.62`
- **CI green** — run `26651628506`: `clippy + test` passed on the first attempt; the `live Convex graph acceptance` job hit the same transient runner-died-during-cache infra failure as before (its acceptance step never started — no logs), and a re-run passed clean. My change touches no Convex code.

#### Commits

- tsift `897a336`, agent-loop pointer `d249e3040` (both pushed)

#### Backlog

`#clippy-workspace-gate` → **done**. Remaining backlog is the externally-blocked OpenCode npm publish chain (`#npm-auth-setup` → `#npm-publish-verify` → `#full-e2e-verify`), which needs your GitHub repo-settings access (`NPM_TOKEN` + `TSIFT_ENABLE_NPM_PUBLISH=true`). **Top of backlog: `#npm-auth-setup`** (blocked — needs you). With this, both coverage gaps from the extraction arc (test + clippy) are closed and the workspace is fully gated.
<!-- agent:boundary:697975fd -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#split-tsift-cli-subcommands] [recommended] tsift-cli is ~39k LOC in a single lib.rs — propose per-command module split (cmd_search, cmd_index, cmd_graph, cmd_explain, etc.) once root tsift is drained, to make command surface independently testable
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
