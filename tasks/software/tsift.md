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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260506-220801.md`*

Active backlog:
- [ ] [#ts1c] Add MCP/tool-native structured terse envelopes so clients can render summaries and request follow-up expansion without prose-heavy defaults
- [ ] [#ts1e] Add a command/test-run envelope schema with transcript artifact handles so green runs stay summary-first by default
- [ ] [#ts1f] Add tsift runner/rewrite integration for cargo test/build/install so agent-doc can opt into digest-first execution output
- 1 more active item(s)

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#ts1c]. spec-test-build-install-commit-push
### Re: structured envelopes — gpt-5

Implemented `#ts1c` in `src/tsift` with a new global `--envelope` flag for `search`, `explain`, `session-review`, and `context-pack`. Envelope JSON now adds `tool`, `view`, `summary`, `truncated`, `follow_up`, and `report`, while preview reports keep their item-level `handle` + `expand` fields for narrow reruns.

Updated `src/tsift/SPEC.md` and `src/tsift/VERSIONS.md`, added CLI plus end-to-end regression coverage, and ran `cargo fmt --manifest-path src/tsift/Cargo.toml`, `cargo test --manifest-path src/tsift/Cargo.toml`, `cargo build --manifest-path src/tsift/Cargo.toml`, and `cargo install --path src/tsift --force`.

Committed and pushed the `tsift` repo change as `c1d816c` (`feat: add structured output envelopes`). I left the parent `agent-loop` submodule pointer uncommitted because the superproject `main` branch is already ahead with unrelated work, so a root push would not have been path-isolated to this task.
❯ do [#ts1e]. spec-test-build-install-commit-push
### Re: #ts1e digest-runner envelope + handles — gpt-5
Implemented `#ts1e` in `src/tsift` with a new `digest-runner` structured envelope path for `tsift --envelope __digest-runner ... --json`. Green runs now stay summary-first by default while the full stdout/stderr transcript is persisted under `.tsift/artifacts/` and returned through a stable artifact handle with `handle`, `path`, `bytes`, `lines`, and `expand`. The envelope reports command metadata, exit status, success, the artifact reference, and the existing digest payload for both test and log runs.

I updated `src/tsift/SPEC.md` and `src/tsift/VERSIONS.md`, added regression coverage in `src/tsift/tests/exit_code.rs`, and tightened `rewrite --run` so already-`tsift search` commands default to `--timeout 0` unless the caller set an explicit timeout.

Verification completed with `cargo fmt --manifest-path src/tsift/Cargo.toml`, targeted digest-runner and rewrite regression tests, `TMPDIR=/home/brian/.tmp/tsift-tests cargo test --manifest-path src/tsift/Cargo.toml`, `TMPDIR=/home/brian/.tmp/tsift-tests cargo build --manifest-path src/tsift/Cargo.toml`, and `TMPDIR=/home/brian/.tmp/tsift-tests cargo install --path src/tsift --force`. The `tsift` repo change is committed and pushed as `5d5aef5` on `main` with message `feat: add digest runner transcript envelopes`.

I left the parent `agent-loop` submodule pointer uncommitted so this cycle only shipped the path-isolated `src/tsift` change and did not mix in unrelated root-repo state.
❯ do [#ts1f]. spec-test-build-install-commit-push
### Re: #ts1f rewrite-run digest envelopes — gpt-5
Implemented `#ts1f` in `src/tsift` by wiring global structured-output flags through `tsift rewrite` so agent-facing callers can opt into digest-first execution output for cargo rewrites instead of falling back to prose-only output. `tsift --envelope rewrite --run 'cargo test ...'`, `cargo build ...`, and `cargo install ...` now forward the structured mode into the rewritten `__digest-runner` command, which means agent-doc can request the same summary-first `digest-runner` envelope that the hidden runner already emits directly.

I also fixed the adjacent cap bug so rewritten `--envelope` tsift commands are treated as structured output and do not get line-capped like human text. Spec/version docs were updated, unit coverage now locks the forwarded rewrite shape for `cargo install` plus structured flag forwarding, and integration coverage exercises real `rewrite --run` envelope execution for `cargo test` and `cargo build` on a temp crate.

Verification completed with `cargo fmt --manifest-path src/tsift/Cargo.toml`, focused rewrite regressions, `TMPDIR=/home/brian/.tmp/tsift-tests cargo test --manifest-path src/tsift/Cargo.toml`, `TMPDIR=/home/brian/.tmp/tsift-tests cargo build --manifest-path src/tsift/Cargo.toml`, and `TMPDIR=/home/brian/.tmp/tsift-tests cargo install --path src/tsift --force`. The `tsift` repo change is committed and pushed as `717c61d` on `main` with message `feat: forward rewrite digest envelopes`.

I left the parent `agent-loop` submodule pointer uncommitted so this turn only shipped the path-isolated `src/tsift` change. Preflight also flagged recent no-op closeout accretion on this document, so `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --commit` is the right move before another large turn.
<!-- agent:boundary:72fdbcf1 -->
<!-- /agent:exchange -->
###

<!--
What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
---
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ts1d] Improve tsift lock/degraded-mode UX so active index writers fall back to read-only or exact search with a minimal actionable summary
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
