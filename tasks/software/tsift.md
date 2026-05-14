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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260514-211658.md`*

Compacted content:
- Archived 1 response topic(s): stale tsift index reminders
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please read audit in /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md

❯ I want to make tsift public and publish to crates and add a `curl ... | sh` install script.
### Re: public tsift release — gpt-5

I read the audit in `tasks/agent-doc/agent-doc-bugs2.md` and treated it as a release-readiness checklist: public repo first, install story next, crates.io only after the package can actually pass a dry run.

What is done:

- Made `btakita/tsift` public: https://github.com/btakita/tsift
- Added `README.md` with the install command: `curl -fsSL https://raw.githubusercontent.com/btakita/tsift/main/scripts/install.sh | sh`
- Added `scripts/install.sh`, which detects Linux/macOS targets, downloads GitHub release assets, verifies SHA-256, and installs to `$HOME/.local/bin` by default.
- Switched `tagpath` from a sibling path dependency to the published `tagpath = 0.6.2` crate, updated `Cargo.lock`, removed CI/release sibling checkouts, and copied the tagpath token-savings fixture into `tsift` so tests no longer depend on `../tagpath`.
- Sanitized the obvious public-audit hits: removed a personal absolute path from a test fixture string and replaced the private project example in `SPEC.md`.
- Committed and pushed the `tsift` submodule as `8378e0f` (`chore: prepare public tsift release`).
- Updated the superproject submodule pointer in commit `d7562b443` (`chore: update tsift release pointer`).
- Tagged and pushed `v0.1.42` in `tsift` to start the GitHub release workflow.

Crates.io is not published yet. `cargo publish --dry-run --allow-dirty` still fails because `sift` is a git dependency with no version requirement, and crates.io already has an unrelated `sift = 0.1.0` package. The `tagpath` blocker is fixed; the remaining blocker is the upstream `sift` dependency strategy.

Verification run locally in `src/tsift`:

- `sh -n scripts/install.sh`
- public-audit `rg` scan for obvious secrets/personal paths/private project names; remaining hits are expected token/API-key env var names in docs/tests, not secret values.
- `make ci-full` passed, including clippy, normal tests, and ignored deterministic SimWorld coverage.
- `cargo publish --dry-run --allow-dirty` failed only on the `sift` git dependency blocker above.

Remote release status at closeout: the `v0.1.42` Release workflow verified successfully, and Linux, macOS arm64, and Windows build jobs completed successfully. The macOS x86_64 job was still queued on GitHub Actions, so the final GitHub Release object and `latest/download` installer assets will appear only after that queued leg completes.
<!-- agent:boundary:93b22b58 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#98qd] Support agent-doc context shaping/orchestration: add tsift-backed backlog coverage for job packets, source handles, and bounded worker context.
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->
<!-- /agent:done -->
