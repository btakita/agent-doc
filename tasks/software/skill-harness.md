---
agent_doc_session: a1a5456d-df6d-4c12-bbf9-f25dcf3811f1
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71050223bcaa3626afc3c261e29d2c9e099b99519b00c1be4df7bfa35b0f57ca-20260519-051950.md`*

Compacted content:
- Archived 1 response topic(s): composable-skills next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71050223...
❯ dispatch #spec-test-build-install-commit-push
❯ - do [#0nap]
❯ - do [#wqew]
❯ - do [#yjgb]
### Re: skill-harness compose-skills follow-ups — gpt-5

Implemented and committed the requested `#spec-test-build-install-commit-push` batch.

Completed:
- `#0nap`: added a bundled `compose-skills` install/check matrix covering Claude, Codex, OpenCode, and generic `.agent/skills` targets.
- `#wqew`: tightened `compose-skills` plan validation with explicit one-skill-vs-many rationale checks, `skill-creator` handoff diagnostics, and golden diagnostic regressions.
- `#yjgb`: added an end-to-end oversized workflow fixture that starts from a broad workflow prompt, decomposes the skill architecture, and documents the expected `skill-creator` handoff.

Updated docs/spec surfaces to describe the new validation contract and fixture coverage.

Verification:
- `cargo fmt`
- `cargo test`
- `cargo test --all-features`
- `cargo build`
- `cargo install --path . --force`
- CLI smoke: `install-dir` + `check-dir` for `claude`, `codex`, `opencode`, and `generic`, plus `compose validate` on the oversized workflow fixture.

CI/tmux check: latest root `Agent Doc` workflow run `26077546433` did not start `check` or `tmux-ci`; GitHub annotated both jobs as blocked by billing/spending-limit status, so there were no tmux logs to reproduce locally.

Commits:
- `src/skill-harness`: `87469b4 test: tighten compose-skills validation`
- parent repo submodule pointer: `2dfb794e2 chore: update skill-harness submodule`
❯ Provide clear usage instructions in the README.md for skill-harness. Does skill-harness provide a cli?
### Re: skill-harness README usage — gpt-5

Yes. `skill-harness` provides a CLI named `skill-harness`.

Updated `src/skill-harness/README.md` with clearer usage instructions:
- states explicitly that the installed command is `skill-harness`;
- adds a CLI command overview matching `skill-harness --help`;
- documents common flags: `--harness`, `--root`, `--source`, and `--file`;
- maps harness targets to their install paths for Claude, Codex, OpenCode, Cursor, and generic `.agent/skills`;
- separates examples for bundled `compose-skills`, custom skill directories, single-file `SKILL.md` installs, listing, uninstalling, and composition-plan validation.

- `cargo run -- --help`
- `cargo run -- install-dir --help`
- `cargo run -- check-dir --help`
- `cargo run -- install --help`
- `cargo run -- list --help`

CI/tmux check: latest root `Agent Doc` workflow run `26078260974` did not start `check` or `tmux-ci`; GitHub annotated both jobs as blocked by billing/spending-limit status, so there were no tmux logs to reproduce locally.

- `src/skill-harness`: `d2a49f4 docs: clarify cli usage`
- parent repo submodule pointer: `5f095958d chore: update skill-harness cli docs`
<!-- agent:boundary:40a6d653 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/skill-harness.done.md -->
- 2026-05-19 [#b4ps] [recommended] Make src/skill-harness/skills/compose-skills the canonical source and add a directory-aware check/sync path so Claude and Codex installs can be verified against the bundled skill tree.
- 2026-05-19 [#jw8x] [recommended] Add compose-skills validation fixtures that cover malformed plans, ambiguous one-skill-vs-many splits, and a real agent-doc workflow decomposition example.
- 2026-05-19 [#nztw] [recommended] Extend skill-harness install coverage beyond Claude/Codex for compose-skills, prioritizing OpenCode and the generic .agent/skills layout.
- 2026-05-19 [#0nap] [recommended] Add a CI/eval matrix that runs install-dir plus check-dir for bundled compose-skills across Claude, Codex, OpenCode, and generic temp roots.
- 2026-05-19 [#wqew] [recommended] Tighten compose-skills plan validation with golden diagnostics for required sections, one-skill-vs-many rationale, and handoff boundaries to skill-creator.
- 2026-05-19 [#yjgb] [recommended] Add an end-to-end compose-skills example that starts from an oversized workflow prompt, produces a decomposed skill architecture, and documents the expected next skill-creator handoff.
<!-- /agent:done -->
