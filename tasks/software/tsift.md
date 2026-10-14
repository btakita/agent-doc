---
agent_doc_session: tsift-v0.1
resume:
  claude: 03da93b9-58a8-4aa3-b530-ca5405b2ea0b
  codex: 01a11754-12ab-7700-8938-4ec9298bb945
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#gh-fix': fix then close
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#subagent': 'run in a subagent'
  '#subagents': 'run the remaining items in subagents'
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20261005-234122.md`*

Compacted content:
- Archived 10 response topic(s): [#haivenrootsync] dispatched to a subagent; [#haivenrootsync] done; [#jaicratepublish] approved; 7 more
- Prior summary/context: prior compacted content: Archived 25 response topic(s): recovered response; Should we release + publish?; #releasepub...

### Re: GitHub issues 26–28 — gpt-5 · 2026-10-07T11:30-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/tsift/issues/26

> **Queue prompt:** #gh-fix https://github.com/btakita/tsift/issues/27

> **Queue prompt:** #gh-fix https://github.com/btakita/tsift/issues/28

Fixed, integrated, pushed, and closed all three issues.

- #26 now reports the actual stale-instruction cause in both human and structured output, including the missing reference path, without a misleading equal-version comparison.
- #27 now preserves each workspace scope persisted personal/off policy; unconfigured scopes inherit the root resolved mode.
- #28 now sweeps scopes in personal and off modes as well as shared mode, so the printed workspace remedy converges.
- Integration reconciled the overlapping #27/#28 mode semantics and retained both regression families.
- Verification passed: focused status/instruction tests, all combined init_workspace_* tests, git diff --check, and the full make check gate.
- Main was pushed and verified at d26d1ab61902f464567611acc398b10b0cbb2708; GitHub issues #26, #27, and #28 are closed.
- The active queue is large/restart-heavy, but it remains drainable; continuing to release + publish without compacting.

### Re: v0.1.104 release — gpt-5 · 2026-10-07T12:11:45-04:00

> **Queue prompt:** release + publish

Released and published tsift v0.1.104 successfully.

- Release commit: f1f39fb260c01e0f45c516238baca7c76d7b156c; annotated tag v0.1.104 resolves to that commit on origin.
- GitHub release: https://github.com/btakita/tsift/releases/tag/v0.1.104 with Linux, macOS, Windows, and checksum assets.
- crates.io: https://crates.io/crates/tsift/0.1.104 verified by downloading registry metadata.
- npm: opencode-tsift reports version 0.1.104.
- Local CLI: tsift 0.1.104 installed from the locked release tree.
- Verification: focused issue regressions and full local make check passed; GitHub Actions release run 37645547218 completed successfully, including release-input CI, all platform builds, crates.io publish, GitHub release, and npm publish.
- Non-blocking workflow annotations note upstream Node.js 20 action deprecation and the upcoming ubuntu-latest runner migration.

### Re: upgrade-friendly tsift skill — gpt-5 · 2026-10-07T12:27:54-04:00

> **Queue prompt:** Make the tsift skill upgrade friendly? Can the skill in the harness refer to the package tsift skill/runbook?

Yes. The harness skill is now an upgrade-safe bootstrap over package-owned live instructions.

- Added `tsift skill [PATH]` for the authoritative SKILL.md bundled with the running binary and `tsift skill --reference` for its detailed code-navigation runbook.
- Generated harness skills now delegate to `tsift skill` once per session. Live output includes an explicit recursion guard; generated skill and reference files remain complete, versioned fallbacks when command execution is unavailable.
- A normal binary upgrade can therefore expose current guidance without rewriting harness directories or tracked repository files.
- Existing pre-bootstrap skills need one `tsift init` after adopting the first release containing this change. The current Codex harness bootstrap is refreshed and points to the user-scoped canonical skill.
- Added CLI parsing, unit, integration, generated-surface, and specification coverage.
- Full `make check` passed in the worker branch. Parent verification passed focused package-skill, package-reference, and CLI integration tests plus git diff checks.
- Installed the development build locally and verified both live instruction surfaces.
- Pushed main at 7c1d378570778dc98d8cbdde166b0566491a6307. This change is on main after the v0.1.104 tag and awaits a later release.

### Re: v0.1.105 release — gpt-5 · 2026-10-07T13:58-04:00

> **Queue prompt:**
>
> do [#releasepublish]


Released and published tsift v0.1.105 successfully.

- Dispatched release preparation to an isolated subagent worktree, then integrated and verified it serially on the coordinator path.
- Release commit: `2d307407ad4c9207b2e837b6f27e6bfa6d2b5378`; `origin/main` and annotated tag `v0.1.105` both resolve to it.
- GitHub release: https://github.com/btakita/tsift/releases/tag/v0.1.105 with Linux, macOS, Windows, and matching checksum assets.
- crates.io: https://crates.io/crates/tsift/0.1.105 verified from the public registry by downloading its metadata.
- npm: `opencode-tsift@0.1.105` verified from the public registry.
- Local CLI: `tsift 0.1.105` installed from the locked release tree using the repository-documented `cargo install --path . --locked` path (the project has no `make install` target).
- Verification passed: focused package-skill regression, all 27 workspace version/package checks, npm publish check, locked all-languages release build, worker and coordinator full `make check` gates, main CI run 37658875282, and Release run 37658906350.
- The Release workflow completed successfully across input validation, all three platform builds, crates.io OIDC publication, GitHub release publication, and npm OIDC publication.

### Re: #fixsdkrun dispatched — opus-5.5 · 2026-10-14T17:20-04:00

**Deferred:** `do [#fixsdkrun]` is claimed by `subagent:fixsdkrun`. The worker is running in its own tsift worktree under the `#spec-test-build-install-commit-push` preset. I'll do one `make install` and close the item once it reports back.

> **Operator note:** Why is the agent searching for symbols that are not in files?

The two failures have different causes:

1. **`parse_args`: the agent guessed.** `scripts/check-godot.py` never defines `parse_args`. It only calls `parser.parse_args()` (argparse) at line 344. The agent took a name from a call site and assumed it was defined in the file. tsift made this worse, because its error says only "no indexed symbol matched". It doesn't mention the call site or list the symbols the file does define, so the agent has nothing to correct with. The fix makes the error do both and suggests a `source-read` window instead.
2. **`main`: a real tsift bug.** `main` is in the file, at line 164 in the `actionable-review-20261014-2` haiven-sdk worktree. That worktree's `.tsift/index.db` holds `main|163`, but `tsift status` still reports the index as `fresh` with 0 stale files. Either the symbol rows are stale while freshness says fresh, or symbol-read rejects a row that drifted and reports that as "not matched". The same command succeeds in `src/haiven-dev/src/haiven-sdk`. The worker is finding which of the two it is, fixing it so a drifted file is re-parsed when requested, and adding regression tests for both cases.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- do [#fixsdkrun]: Why is the agent searching for symbols that are not in files?
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#fixsdkrun] Fix from sdk.md run:
  ```
  • Failed (exit 1) tsift --envelope symbol-read main --file scripts/check-godot.py --budget normal
  └ Error: no indexed symbol matched "main" in scripts/check-godot.py; searched: <root>

  • Failed (exit 1) tsift --envelope symbol-read parse_args --file scripts/check-godot.py --budget normal
  └ Error: no indexed symbol matched "parse_args" in scripts/check-godot.py; searched: <root>
  ```
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

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
