---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
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
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260602-151510.md`*

Compacted content:
- Archived 1 response topic(s): recursive submodule commit + push
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca30...
❯ Fix "Cargo still emits the pre-existing workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router` during some builds." seen in lazily-rs.md session.
❯ commit + push

### Re: Fix workspace patch warnings for agent-kit, tagpath, tmux-router — opus-4-8

**Root cause.** The repo-root `.cargo/config.toml` declared `[patch.crates-io]` for `tmux-router`, `agent-kit`, and `tagpath`. Cargo merges config from the cwd up to the filesystem root, so that single file applied its patches to **every** submodule build in the tree. Any build whose dependency graph lacked a patched crate (e.g. `lazily-rs`) — or used a different version (`tsift` wants `tagpath 0.11.1`; local is `0.12.1`) — emitted `warning: patch ... was not used in the crate graph`.

**Fix (scoped the patches to the workspaces that actually consume them):**

- Removed the global repo-root `.cargo/config.toml`. This is already in `HEAD` — a concurrent `agent-doc-bugs2` session committed the deletion (`8734f7b66`, HEAD now `4813ed0f6`), so the global pollution is gone from the committed tree. My `git rm` was a redundant no-op against the same already-deleted path.
- `src/agent-doc/.cargo/config.toml` (gitignored local-dev wiring) now patches all three siblings — `tmux-router`, `tagpath`, `agent-kit` — so the agent-doc workspace still builds against local sources.
- `src/instruction-files/.cargo/config.toml` (new, untracked — mirrors corky's untracked local config) patches `agent-kit` so the repo-root `make check` (`cd src/instruction-files && cargo clippy && cargo test`) keeps exercising the local crate. Local `agent-kit` currently has no source changes (Cargo.lock/README only), so this is behavior-preserving rather than a functional shift.

**Verification (current tree):**

- `lazily-rs`, `tsift`, `corky`, `agent-doc` — `cargo metadata` clean, **no patch warnings**.
- `agent-doc` still resolves `tmux-router`/`tagpath`/`agent-kit` → `src/<crate>` (local).
- `make check-shared` (instruction-files `clippy -D warnings` + tests): green, no patch warnings.

**Note:** the gitignored/untracked per-workspace configs are local-dev conveniences (same pattern as corky's existing untracked `.cargo/config.toml`); they are not part of any committed surface. The committed superproject change is the root-config removal, already in `HEAD`. Pushing the 2 ahead-of-origin commits to publish it.
<!-- agent:boundary:07482540 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:pending -->
- [ ] [#bx57] Disable unused MCP servers — currently managed via Claude Code UI, not settings.json
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-11 [#aaj9] Transfer snapshot GC feature to agent-doc-feat.md
- 2026-05-11 [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- 2026-05-19 [#0b3x] [recommended] Create a composable-skill-design skill or extend skill-creator with a composability audit workflow, backed by examples and validation checks
- 2026-05-19 [#cdvx] [recommended] Check crates.io availability for candidate public crate names before renaming skill-harness
- 2026-05-19 [#25k6] [recommended] Keep skill-harness as the current Rust crate/package boundary, or rename only to an available precise crate name such as agent-skillkit/agent-skill-tools after checking registry availability; do not target the taken agent-skills crate name
- 2026-05-19 [#e92b] [recommended] Move compose-skills toward skill-harness/agent-skills with Rust validation and Claude/Codex installer coverage
<!-- /agent:done -->
