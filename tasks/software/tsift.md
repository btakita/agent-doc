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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-142805.md`*

Compacted content:
- Archived 6 response topic(s): next steps to extract tsift crates; queued next steps closeout; dormant crates usage; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: tsiftcli-direct-crates — gpt-5

Done. I removed `tsift-cli`'s dependency on the root `tsift` shim and rewired CLI modules to import the split crates directly (`tsift-index`, `tsift-search`, `tsift-digest`, `tsift-agent-doc`, `tsift-status`, `tsift-sqlite`, and the existing sibling crates). The root `tsift` crate remains a backwards-compatible re-export surface.

Spec/test coverage:
- Updated `SPEC.md` to state that `tsift-cli` must depend directly on sibling `tsift-*` crates, not the root shim.
- Added `cli_manifest_uses_split_crates_without_root_shim` to lock the manifest boundary.
- Verified no remaining `tsift::` or `tsift = { path = "../.." }` references under `packages/tsift-cli`.

Verification/build/install:
- `tsift rewrite --run 'cargo check -p tsift-cli'` passed.
- `tsift rewrite --run 'cargo test -p tsift-cli cli_manifest_uses_split_crates_without_root_shim'` passed.
- Final `make check` passed after the last edit: workspace clippy, workspace tests, opencode plugin tests, and npm dry-run publish check.
- `tsift rewrite --run 'cargo build -p tsift-cli'` passed.
- `tsift rewrite --run 'cargo install --path packages/tsift-cli --locked --force'` passed.
- Latest GitHub Actions `CI` run `26677485874` was successful; this repo has no separate tmux workflow/job to inspect.

Commits pushed:
- `src/tsift`: `4a11085 refactor: import split crates in tsift cli`, pushed to `origin/main`.
- `agent-loop`: `59a970951 chore: update tsift submodule`, pushed to `origin/main`.

`#tsift-dormant-crates` and `#tsift-crate-publish-preflight` remain queued for later cycles.

### Re: #tsift-dormant-crates — gpt-5

Wired both dormant graph crates into real surfaces.

- `tsift-algorithms` now backs a new `tsift analyze` command that reports SCCs, composite health, dead-code reachability, and module coupling over indexed call edges.
- Fixed a dormant `tsift-algorithms` health-analysis Tarjan recursion bug that stack-overflowed on a dense cycle once the CLI exercised it.
- `tsift-tokensave` is now available as `tsift graph-db --backend tokensave` for read-only provider-neutral node/kind/edge/neighborhood/path queries against `.tokensave/tokensave.db`.
- Root `tsift` re-exports both crates as `tsift::algorithms` and `tsift::tokensave`; `SPEC.md` now describes the matching CLI and root API surface.

Commits pushed:

- `a319ad8` in `src/tsift`: `feat: wire dormant tsift graph crates`
- `6644f7692` in `agent-loop`: `chore: update tsift submodule`

Verification:

- `cargo check -p tsift-cli`
- `cargo check -p tsift`
- `cargo test -p tsift-algorithms dense_cycle_does_not_recurse_on_current_node`
- `cargo test -p tsift-cli analyze_json_runs_graph_algorithms`
- `cargo test -p tsift-cli graph_db_tokensave_backend_queries_tokensave_db`
- `cargo test -p tsift root_reexports_algorithm_and_tokensave_crates`
- `make check` (clippy, workspace tests, opencode tests, npm publish dry-run)
- `cargo build -p tsift-cli`
- `cargo install --path packages/tsift-cli --locked --force`
- GitHub Actions `CI` run `26686994059` completed `success` after push.

Workflow note: this repo currently has `CI` and `Release` workflows; there is no separate tmux workflow/job to inspect.

### Re: closeout recovery — gpt-5

Recovered the session document closeout after the prior response was already committed but the live buffer still had agent-owned bookkeeping drift.

Changed paths: only `tasks/software/tsift.md` in this recovery cycle. The tsift implementation and submodule pointer were already committed and pushed in the previous cycle.

Commands: `agent-doc repair`, `agent-doc preflight`, `agent-doc plan`, and this `agent-doc finalize` recovery closeout.

Verification: no source files changed in this recovery cycle. The prior implementation verification remains `make check`, `cargo build -p tsift-cli`, `cargo install --path packages/tsift-cli --locked --force`, and CI run `26686994059` passing.

Confidence: high. Escalation: none.
<!-- agent:boundary:9b747b31 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#tsiftcli-direct-crates]~
- ~do [#tsift-dormant-crates]. wire~
- do [#tsift-crate-publish-preflight]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsift-crate-publish-preflight] Normalize extracted-crate release metadata and version policy: align split-crate versions/readmes/publish settings, then run cargo check/test plus cargo publish --dry-run in dependency order.
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
