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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260529-072627.md`*

Compacted content:
- Archived 7 response topic(s): do [#cli-cluster-infra]; do [#cli-cluster-digests]; What are #next-steps to complete the review items?; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do A — #split-sim-world via Option A (test-harness crate) — opus-4-8

Done, committed, pushed, CI green. Option A re-homed the SimWorld harness as a test-harness crate **and** fixed the dead-test bug — which turned out to have a third root cause and a wider blast radius than the investigation surfaced.

#### What landed → `packages/tsift-sim-world`

A new workspace crate whose `src/lib.rs` carries **no production surface** — the deterministic seeded-corpus + named-edge-trace tests live in `tests/sim_world.rs`, with `tsift` + `tsift-cli` as **dev-dependencies**. Dev-deps don't form a lib cycle, so root `tsift` does not (and cannot) re-export it. `src/sim_world.rs` is deleted (git tracked it as a rename).

#### Three root causes the harness had silently rotted on

The investigation found two; building it surfaced a third:

1. **No `mod sim_world`** anywhere → never compiled. (known)
2. **`crate::rewrite_command`** now lives in `tsift-cli` as `pub(crate)` → unreachable from root. (known) → **promoted `tsift_cli::rewrite_command` to `pub`.**
3. **`session_digest::extract_prompt_targets_from_text_block`** became `#[cfg(test)] pub(crate)` in `tsift-session` after the session split → invisible cross-crate, so the harness couldn't compile even as a sibling. **(new)** → added a **`test-support` feature** to `tsift-session` that re-exposes it as `pub` (`#[cfg(any(test, feature = "test-support"))]`), enabled by `tsift-sim-world`'s dev-dependency. No production bloat — the feature is off in every non-harness build.

This confirms the review item's literal "root `tsift` re-exports via `pub use`" framing was **impossible** (cycle), and Option A's dev-dependency shape is the only form that both extracts and re-activates it.

#### The wider blast radius — `make check` wasn't running these tests at all

The harness was dead because `make check`'s `test:` target ran **bare `cargo test`** = root `tsift` package only (56 tests). It never ran `tsift-cli`'s 479 tests, the 3 sim tests, or `tsift-resolution`'s suite. **Switched `test:` → `cargo test --workspace`.**

That immediately caught **one more stranded dead test**: `tsift-resolution/src/scoring.rs` used `GraphFreshness` without importing it. The irony — a prior `#cli-cleanup-unused-imports` cycle had *removed* that import as "unused" precisely because the test never compiled under root-only `cargo test`. Re-added `use tsift_core::GraphFreshness`. The whole class is now guarded going forward.

#### Verification

- `cargo test --workspace` — **993 passed across 34 suites** (was effectively 56 reaching CI before)
- `make check` — green (clippy `-D warnings` clean, workspace tests, opencode npm dry-run); confirmed `Running tests/sim_world.rs` now executes
- `cargo install --path packages/tsift-cli` + `cargo build --release -p tsift-cli` — both binaries refreshed; `tsift --version` → `0.1.62`
- **CI green** — run `26623438920`, both `clippy + test` and `live Convex graph acceptance` jobs succeeded (CI runs `make ci-full` → the same `cargo test --workspace` path)

#### Commits

- tsift @ `7f0e1c6` — `#split-sim-world` (pushed)
- agent-loop @ `2eed5dbdb` — submodule pointer bump (pushed)

VERSIONS.md `Unreleased` updated. Marked `[#split-sim-world]` **done**.

#### Top of backlog

Next actionable: **`[#split-tsift-digest]`** — extract `diff_digest` + `log_digest` + `metric_digest` + `test_digest` into a `tsift-digest` crate. Low-risk: `tsift-cli` already calls these as `tsift::diff_digest::*`, so crate + root re-export keeps callers compiling. Then `#split-tsift-search` → `#demote-root-tsift`; the npm chain (`#npm-auth-setup` →) stays externally blocked on your GitHub repo-settings access.
<!-- agent:boundary:b7999beb -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
dispatch #spec-test-build-install-commit-push
- do [#split-tsift-digest]
- do [#split-tsift-search]
- do [#demote-root-tsift]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#split-tsift-digest] Extract diff_digest + log_digest + metric_digest + test_digest (src/, ~3.7k LOC) into a new tsift-digest crate. tsift-cli already calls these as tsift::diff_digest::* etc., so crate + root re-export keeps callers compiling. (promoted from agent:review)
- [ ] [#split-tsift-search] Extract sift + impact + tagpath_adapter (src/) into a new tsift-search crate (summarize already in tsift-summarize). Depends on tsift-index + tsift-graph (both exist). (promoted from agent:review)
- [ ] [#demote-root-tsift] After the sim-world/digest/search splits land, demote root tsift to a thin re-export shim (or remove lib.rs); verify cargo install tsift consumers keep working. (promoted from agent:review)
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#split-tsift-cli-subcommands] [recommended] tsift-cli is ~39k LOC in a single lib.rs — propose per-command module split (cmd_search, cmd_index, cmd_graph, cmd_explain, etc.) once root tsift is drained, to make command surface independently testable
- [/] [#demote-root-tsift] After all module splits land, demote root tsift crate to a thin re-export shim (or remove lib.rs entirely) — leave only the workspace root + binary metadata; verify external consumers (cargo install tsift) keep working
- [/] [#split-tsift-search] Extract sift + summarize + impact + tagpath_adapter (~3.2k LOC) into tsift-search crate — depends on tsift-index + tsift-graph; covers search ranking, AST-aware summaries, impact analysis, and tagpath annotation surface
- [/] [#split-tsift-digest] Extract diff_digest + log_digest + metric_digest + test_digest (~3.7k LOC) into tsift-digest crate — high-cohesion emitter group with shared serialization, schema versioning, and pruning patterns; minimal cross-deps
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
