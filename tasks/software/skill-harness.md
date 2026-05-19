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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71050223bcaa3626afc3c261e29d2c9e099b99519b00c1be4df7bfa35b0f57ca-20260519-060605.md`*

Compacted content:
- Archived 2 response topic(s): release + publish; CI issues
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71050223...
❯ Please add gh releases for Linux, OSX, and Windows.
❯ Add a cross-platform `curl | sh` installation script.
### Re: release assets + installer — gpt-5

Implemented the release/installer follow-up in `src/skill-harness`.

Changed:
- Added `workflow_dispatch` to `.github/workflows/release.yml` so the GitHub Release workflow can be run manually as well as by `v*` tags.
- Extended the Release matrix with `aarch64-pc-windows-msvc`, so future tagged releases publish Windows ARM64 alongside the existing Linux, macOS, and x86_64 Windows assets.
- Reworked `install.sh` into a cross-platform `curl | sh` installer for Linux, macOS, and Windows POSIX shells (`mingw`, `msys`, `cygwin`). It now selects `.tar.gz` for Unix targets and `.zip` for Windows, supports `VERSION=...` and `INSTALL_DIR=...`, and can extract Windows zips with `unzip` or `powershell.exe`.
- Documented the installer knobs and release asset names in `README.md`.

Current release state:
- `v0.1.1` already has GitHub Release assets for Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- The new Windows ARM64 asset will be produced on the next release tag or manual Release workflow run after these changes are pushed.

Verification:
- `sh -n install.sh`
- workflow YAML parse with PyYAML
- `cargo fmt --check`
- `cargo clippy --all-features -- -D warnings`
- `cargo test --all-features` passed, 30 tests
- `cargo build --all-features`
- live installer smoke test: `VERSION=v0.1.1 INSTALL_DIR=<tmp> ./install.sh` then `<tmp>/skill-harness --help`
- `gh release view v0.1.1 --repo btakita/skill-harness --json tagName,assets,url`
- `skill-harness` latest CI on `main` is green: run `26079111898`
- Parent `agent-loop` latest `Agent Doc` CI still fails before runner startup: both `check` and `tmux-ci` have empty step lists/logs, `runner_id=0`, and the annotation says the job was not started because account payments failed or the spending limit needs to be increased. I classified that as the same external CI-start blocker rather than a code or tmux regression.
<!-- agent:boundary:7fb2d84e -->
<!-- /agent:exchange -->
###
<!--
-->

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
