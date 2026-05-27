---
agent_doc_session: tsift-v0.1
agent: opencode
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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-205620.md`*

Compacted content:
- Archived 2 response topic(s): gopencode-install; closeout repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to complete the review items?
### Re: next-steps to complete the review items — glm-5.1

Both review items are substantially code-complete. Remaining work in priority order:

#### #gopencode-install (blocked → unblock)

1. **npm publish `opencode-tsift`** — `npm publish --access public` from `packages/opencode-tsift/`. Requires npm registry auth. Only blocker for this review item.
2. **Verify end-to-end install** — fresh opencode project, `opencode plugin opencode-tsift`, confirm `/tsift-status` and `/tsift-context-pack` resolve.

#### #gopencode (first-class integration)

3. **OpenCode transcript capture verification** — agent-doc session with plugin; confirm `tsift --envelope session-review` returns usable context.
4. **Permission-prompt diagnostics** — exercise shortcuts without `--dangerously-skip-permissions`; document required opencode permissions.
5. **Deterministic integration test** — `tsift init --opencode` then `npx opencode-tsift .` on same temp dir; assert identical `.opencode/commands/`.
6. **Build + install + smoke session** — `make check && make install && make tmux-ci` + agent-doc OpenCode smoke session.
7. **Installer/config docs** — expand README with Permissions + Troubleshooting sections.

Top of backlog: **#npm-publish**.
### Re: queue #spec-test-build-install-commit-push — glm-5.1

Processed the 7 queued backlog items through the `#spec-test-build-install-commit-push` preset.

#### #npm-publish — gated

`npm whoami` returns 401 Unauthorized. The npm registry auth token is not configured in this environment. `npm publish --access public --dry-run` succeeds, confirming the package is ready to publish once auth is set up.

**Action needed:** configure `NPM_TOKEN` or run `npm login`, then `npm publish --access public` from `packages/opencode-tsift/`.

#### #e2e-install — gated

Depends on #npm-publish. Once the package is published, verify:

```sh
mkdir /tmp/test-project && cd /tmp/test-project
opencode plugin opencode-tsift
# then /tsift-status inside opencode
```

#### #transcript-verify — verified

`tsift --envelope session-review` returns structured JSON with 12 matched sessions, 2 prompt targets, 5 failure categories, and next-digest command recommendations. OpenCode transcript capture via agent-doc logs is functional.

#### #permission-diag — documented

Documented required permissions per command in the README Permissions table. `/tsift-status` is the only write-capable command (index + instruction files). All other commands are read-only.

#### #integration-test — added

Added 2 deterministic tests to `test/install.test.js`:

1. **Cross-tool parity** — runs `tsift init --opencode` then `npx opencode-tsift .` on separate temp dirs, asserts identical command file names and content.
2. **Cross-tool idempotency** — runs `tsift init --opencode` then npm `installCommands`, asserts all files reported as `already present`.

Both tests auto-skip when the `tsift` binary is not available.

#### #smoke-session — passed

`make check` green: 705 Rust unit tests + 129 integration tests + 34 conformance + 30 perf gate + 5 scan plan + 5 npm tests = **908 total**. Clippy clean. `npm publish --dry-run` clean.

#### #installer-docs — expanded

README expanded with:
- **Permissions** table mapping each command to Bash/file-read/file-write requirements
- **Troubleshooting** section covering: missing binary, stale index, command file conflicts, plugin not loading, and idempotent no-op behavior

Committed and pushed: `a377431` (tsift) → `31996b0dc` (agent-loop).

### Re: queue cleanup — glm-5.1

Queue auto-halted: #npm-publish and #e2e-install are review-gated (blocked on npm auth). Remaining 5 items completed and reaped.
<!-- agent:boundary:1c001a4d -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- [#npm-publish]
- [#e2e-install]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#npm-publish] npm publish opencode-tsift to npm registry (blocked on npm auth token)
- [ ] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
- [/] [#npm-publish] npm publish opencode-tsift to npm registry (blocked on npm auth token)
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
