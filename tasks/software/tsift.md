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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

OpenCode npm package path code-complete and pushed. #gopencode-install is gated on npm registry auth to publish opencode-tsift.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-200555.md`*

Compacted content:
- Archived 7 response topic(s): tokensave vs tsift; OpenCode integration next steps; OpenCode integration next steps; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#gopencode-install]. #spec-test-build-install-commit-push
### Re: gopencode-install — gpt-5

Implemented and pushed the OpenCode npm package path for tsift, but the registry publish itself is gated on npm permissions.

What changed:
- Added `packages/opencode-tsift`, an npm/OpenCode plugin package that carries the same marker-owned `/tsift-*` command templates as `tsift init --opencode`.
- The package exposes an OpenCode server plugin entrypoint (`./server`) so `opencode plugin opencode-tsift` can load it after publication, writes the six command files into `.opencode/commands`, and refuses unmanaged same-name command conflicts.
- Added a direct `opencode-tsift` CLI installer, Node tests, Rust parity coverage asserting the package command files match `tsift init --opencode`, and release workflow support for npm publish dry-run plus gated tagged npm publication.
- Installed the generated tsift OpenCode shortcuts into the agent-loop root and advanced the `src/tsift` submodule pointer.

Verification:
- `make check` passed in `src/tsift` after the final tree, including clippy, all Rust tests, Node installer tests, and `npm publish --access public --dry-run`.
- `cargo build --release` passed.
- `cargo install --path . --force` replaced the local `~/.cargo/bin/tsift` with `tsift 0.1.62`.
- Fresh OpenCode smoke passed with a temp project: `opencode plugin /home/brian/work/btakita/agent-loop/src/tsift/packages/opencode-tsift` detected the server target, then a fresh TUI startup loaded the plugin and created all six `.opencode/commands/tsift-*.md` files.
- `npm publish --access public --dry-run` passed; actual `npm publish --access public` failed at the registry permission/auth gate (`E404 ... not found or you do not have permission`).
- GitHub Actions `btakita/tsift` CI for `ac1ca49` passed. The root `agent-loop` Agent Doc workflow for `1fed363e9` failed with empty-step jobs and no logs, so I classified it as an external CI-start/runner allocation failure rather than a repo regression.

Commits pushed:
- `btakita/tsift` `ac1ca49` — `feat: add opencode npm package`
- `btakita/agent-loop` `1fed363e9` — `chore: install tsift opencode shortcuts`

`#gopencode-install` is gated, not done, because the only remaining work is external registry authorization: publish `opencode-tsift` with an npm account/token that can create that package name, or configure `NPM_TOKEN` plus `TSIFT_ENABLE_NPM_PUBLISH=true` for the tagged release path.

OpenCode references used: custom commands are markdown files under `.opencode/commands` and run by slash command name in the TUI; npm plugins are listed in `plugin` config and installed by Bun at startup. Sources: https://opencode.ai/docs/commands/ and https://opencode.ai/docs/plugins/.

### Re: closeout repair — gpt-5

Closeout repair only: committed the already-written status, queue strike-through, and gated `#gopencode-install` transition through the binary-owned path. No additional repo work was started after the prior response.
<!-- agent:boundary:e7d0224a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gopencode-install] Make tsift installable as an opencode harness via `tsift init --opencode`: publish the command shortcuts to the opencode marketplace/registry so users can install tsift opencode support without cloning the repo, document the install path, and verify end-to-end with a fresh opencode session.
- [/] [#gopencode] [tag: opencode] [tag: integration] Make OpenCode a first-class tsift integration: add installer/config docs, envelope shortcuts for session/log/test/diff digest use, verify OpenCode transcript capture and permission-prompt diagnostics, add deterministic tests, then build/install and run an end-to-end agent-doc OpenCode smoke session.
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
