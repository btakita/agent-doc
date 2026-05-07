---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260507-025115.md`*

Active backlog:
- [ ] [#ts1d] Improve tsift lock/degraded-mode UX so active index writers fall back to read-only or exact search with a minimal actionable summary

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#ts1d]. spec-test-build-install-commit-push
### Re: #ts1d — gpt-5
Implemented the degraded `tsift search --autoindex` path for live `index.lock` contention.

- When another tsift writer is actively refreshing a stale index, search now keeps going in read-only mode against the current index snapshot and emits one concise retry hint.
- When the index is still missing and a live writer holds the lock, search now falls back to exact live-file search instead of failing closed.
- Updated the versioned surfaces for this behavior in `Cargo.toml`/`Cargo.lock`, `VERSIONS.md`, `SPEC.md`, `CLAUDE.md`, and the code-navigation marker.

Verification + install:
- `cargo test` passed with `TMPDIR=/home/brian/.tmp-tsift-tests-full` to keep the test temp roots away from the host `/tmp/.tsift` ancestor.
- `cargo build` passed.
- `cargo install --path . --force` passed; installed binary now reports `tsift 0.1.38`.

❯ Commit + push:
❯ - `6e4ed3a feat: degrade search when index writer is active`
❯ What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
❯ ---
❯ Can tsift use rtk to compress tool call output in the context?
❯ -->
❯ With the latest tagpath updates, what are #next-steps to save tokens?
### Re: token-saving next steps — gpt-5
The top backlog item is `#tctx`.

With the latest tagpath work, tsift already has the right primitive for semantic compression: search preview can collapse repeated symbol surfaces into one canonical `tag_alias` family, emit stable handles, and give an expansion command instead of dumping every hit. The next savings should come from making those bounded surfaces the default agent path rather than optional flags.

Priority order:

1. `#tctx` — Make the token-saving surfaces automatic in agent workflows. `tsift init`, Code Navigation guidance, hooks, and `tsift rewrite` should steer agent-facing calls toward `tsift --envelope ... --max-items ... --max-bytes ...` for `search`, `explain`, `session-review`, and `context-pack`, and toward digest-runner artifacts for build/test/log output.
2. `#trtk` — Yes, tsift can use RTK, but the clean boundary is interop rather than replacing tsift's own envelopes. RTK is installed here and already compresses cargo, tests, grep, JSON, logs, and similar command families. tsift should detect RTK when available, delegate generic command-output filtering to it where RTK is stronger, then wrap the result in tsift's summary/artifact/envelope shape so callers still get stable handles and expansion commands.
3. `#tbud` — Add budget presets. A caller should be able to say small/normal/deep, or let tsift recommend `max-items` / `max-bytes` for the current task shape, instead of hand-tuning every command.
4. `#tsum` — Extend context packs with cached summaries behind handles. For touched files, failure anchors, and tag_alias families, include short current summaries when available and expose expansion commands when not.

The main UX principle is summary-first, expand-on-demand. The user should see enough context to decide what to inspect next, while raw transcripts, full hit lists, and verbose logs stay behind artifacts or expansion commands.
❯ do [#tctx]. spec-test-build-install-commit-push
### Re: #tctx — gpt-5
Implemented `#tctx` and marked it done in the backlog.

What changed:
- `tsift rewrite` now defaults agent-facing rewrites to token-saving envelopes: `rg` / recursive `grep` become `tsift --envelope search ... --exact --max-items 5 --max-bytes 160`, and cargo/pytest/build/install rewrites become artifact-backed `tsift --envelope __digest-runner ...` commands.
- `tsift rewrite --run` now executes the current `tsift` binary instead of whichever `tsift` is first on `PATH`, so local testing and freshly installed versions do not accidentally call an older binary.
- Exact search now honors an explicit non-federated `--path` directly instead of promoting temp/non-git paths too broadly.
- `context-pack` now enriches next-context and log previews from diff-touched symbols when the session/log surfaces do not have symbol refs on their own.
- Updated `tsift init` Code Navigation output, `AGENTS.md`, `CLAUDE.md`, `SPEC.md`, and `VERSIONS.md`; bumped tsift to `0.1.39`.

Verification:
- `TMPDIR=/home/brian/.tmp-tsift-tests-full cargo test` passed: 545 unit tests and 84 integration tests.
- `cargo build` passed.
- `cargo install --path . --force` passed.
- Installed binary reports `tsift 0.1.39`.

Commits pushed:
- `btakita/tsift@4a015e3` — `feat: default agent rewrites to envelopes`
- `btakita/agent-loop@724880792` — `chore: update tsift submodule`
❯ do [#trtk]. spec-test-build-install-commit-push
### Re: #trtk — gpt-5
Implemented `#trtk` and marked it done in the backlog.

What changed:
- `tsift --envelope __digest-runner ...` now probes `rtk rewrite` when RTK is installed, executes supported command families through RTK's compact command filters, and records the chosen filter in `report.filter`.
- Digest-runner reports now preserve both `report.command` (the original caller command) and `report.executed_command` (the actual RTK-filtered or fallback command), while keeping the existing exit-code preservation, digest payload, and persisted artifact handle.
- Envelope summaries now include a `filter` metric (`rtk` or `none`) so harnesses can tell which compression path was used.
- Updated `SPEC.md`, `CLAUDE.md`, `AGENTS.md`, `src/init.rs`, `VERSIONS.md`, and bumped tsift to `0.1.40`.

Verification + install:
- Targeted RTK delegation regression passed.
- `TMPDIR=/home/brian/.tmp-tsift-tests-full cargo test` passed: 545 unit tests and 85 integration tests.
- `cargo build` passed.
- `cargo install --path . --force` passed.
- Installed binary reports `tsift 0.1.40`.

Commits pushed:
- `btakita/tsift@531b36f` — `feat: wrap digest runner with rtk filters`
- `btakita/agent-loop@5fa3a4f6b` — `chore: update tsift submodule`
❯ do [#tbud]. spec-test-build-install-commit-push
### Re: #tbud — gpt-5
Implemented `#tbud` and marked it done in the backlog.

What changed:
- Added `--budget <small|normal|deep|auto>` to `search`, `explain`, `session-review`, and `context-pack` preview surfaces.
- `tsift --envelope` now applies the adaptive budget by default for preview-capable commands when no explicit caps are supplied. `auto` reads `TSIFT_CONTEXT_WINDOW`, `CODEX_CONTEXT_WINDOW`, or `CLAUDE_CONTEXT_WINDOW`: <=64k uses `small`, >=200k uses `deep`, otherwise `normal`.
- `rg` and recursive `grep` rewrites now emit `tsift --envelope search ... --exact --budget normal` instead of hard-coded `--max-items 5 --max-bytes 160`.
- Updated `SPEC.md`, `AGENTS.md`, `CLAUDE.md`, `src/init.rs`, `VERSIONS.md`, and bumped tsift to `0.1.41`.

Verification + install:
- `cargo check` passed.
- `TMPDIR=/home/brian/.tmp-tsift-tests-full cargo test` passed: 547 unit tests and 85 integration tests.
- `cargo build` passed.
- `cargo install --path . --force` passed.
- Installed binary reports `tsift 0.1.41`.
- Live checks confirmed `--budget small` emits `max_items: 3` / `max_bytes: 120`, and bare `--envelope search` defaults to the normal adaptive budget here.

Commits pushed:
- `btakita/tsift@884b46c` — `feat: add agent budget presets`
- `btakita/agent-loop@3c675f37d` — `chore: update tsift submodule`
<!-- agent:boundary:ebc0f1dc -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tsum] Extend context-pack/digest surfaces to surface cached symbol/file summaries behind stable handles for touched files, failures, and tag_alias families
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
