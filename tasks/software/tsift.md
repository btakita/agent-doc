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

### Re: #tsift-crate-publish-preflight — gpt-5

Implemented and pushed the split-crate publish preflight.

What changed:

- Normalized all Rust workspace crate versions and internal path dependencies to `0.1.62`.
- Added publish metadata across the split crates: `publish = true`, README paths, keywords, categories, and package READMEs where missing.
- Made the root `tsift` package own the public `tsift` binary, delegating to `tsift-cli`, so `cargo install tsift` keeps working without a workspace duplicate-bin warning.
- Kept `tsift-cli` as the dispatch/test package with its own `tsift-cli` test binary.
- Updated the release workflow to validate every manifest version against the tag, package-list every split crate during verification, build release assets from the public root package, and run `cargo publish --dry-run` immediately before each real dependency-ordered publish.
- Updated README/SPEC release notes and added tests that enforce the release order and manifest publish metadata.

Verification:

- `cargo test -p tsift-cli release_publish_gate_requires_secret_variable_and_dry_run` passed.
- `cargo test -p tsift-cli split_crate_manifests_are_publish_ready` passed through the full suite.
- `cargo test -p tsift-cli graph_db_cli_conformance_matches_sqlite_and_convex_snapshot_queries` passed.
- `cargo build -p tsift --release --locked` passed.
- Dependency-order `cargo package -p "$package" --locked --allow-dirty --list` passed for every Rust crate.
- `cargo publish -p tsift-core --locked --dry-run --allow-dirty` passed.
- `cargo publish -p tsift-graph --locked --dry-run --allow-dirty` fails before upload because `tsift-core` is not published to crates.io yet; that is the expected crates.io dependency constraint and is why the workflow dry-runs each crate immediately before its real publish, after prior dependencies have landed.
- `make check` passed after the final binary-target cleanup.
- GitHub Actions `CI` run `26687756843` passed: `clippy + test` and `live Convex graph acceptance` both green. The live Convex job only emitted the expected no-op configuration annotation plus GitHub's Node 20 actions deprecation notice.

Commits pushed:

- `src/tsift`: `24ef10d chore: prepare split crates for publish`
- parent repo: `0bb7abdee chore: update tsift submodule`

I also checked for a tmux CI leg in the tsift repo and there is no `tmux`/`tmux-ci` workflow target to inspect.
❯ release + publish

### Re: release + publish — gpt-5

Released and published `tsift` v0.1.62.

The apparent stall was crates.io throttling new-crate publishes. The release creates many first-time crate names, and crates.io returned `429 Too Many Requests` with retry-after windows. The final failure before success was a separate ordering bug: `tsift-sim-world` was published before root `tsift`, even though `tsift-sim-world` has a dev-dependency on `tsift`. I fixed the order and reran the tag.

Changed paths:
- `src/tsift/.github/workflows/release.yml`
- `src/tsift/README.md`
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- parent submodule pointer `src/tsift`

Commits pushed:
- `src/tsift` `fca3045` `fix: allow release package file checks after ci`
- `src/tsift` `5f9c56b` `fix: resume crates publish on release rerun`
- `src/tsift` `4f95873` `fix: publish root crate before sim world`
- parent `db50360cf` `chore: update tsift submodule`
- parent `72b487df3` `chore: update tsift release pointer`
- parent `e25044502` `chore: update tsift release order`

Verification:
- `cargo test -p tsift-cli release_publish_gate_requires_secret_variable_and_dry_run` passed after each release-workflow fix.
- `make check` passed after the final ordering fix.
- Latest `CI` run for `4f95873` passed: `26690820586`.
- Final `Release` run passed: `26690822959`, completed `2026-05-30T18:10:11Z`.
- GitHub Release `v0.1.62` is published with Linux, macOS, and Windows archives plus SHA256 assets: `https://github.com/btakita/tsift/releases/tag/v0.1.62`.
- crates.io verification passed for every `0.1.62` crate: `tsift-core`, `tsift-graph`, `tsift-sqlite`, `tsift-algorithms`, `tsift-resolution`, `tsift-tokensave`, `tsift-libsql`, `tsift-index`, `tsift-summarize`, `tsift-quality`, `tsift-agent-doc`, `tsift-digest`, `tsift-search`, `tsift-status`, `tsift-session`, `tsift-cli`, `tsift`, and `tsift-sim-world`.

Notes:
- OpenCode npm publish remains skipped by the release gate because npm publishing is not configured with the required repo variable/secret. The dry-run package check still passes.
- GitHub emitted Node.js 20 deprecation annotations for current Actions dependencies; those did not block the release.
- The `src/tsift` worktree is clean. The parent worktree still has unrelated pre-existing dirty files, which I left untouched.

Confidence: high. The tag, release assets, CI, Release workflow, and all crates.io package versions are verified.

Escalation: no approval escalation was needed; I used the existing approved git, cargo, make, gh, and agent-doc paths.
<!-- agent:boundary:a0588abc -->
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
