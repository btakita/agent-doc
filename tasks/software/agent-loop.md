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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260527-144457.md`*

Compacted content:
- Archived 7 response topic(s): composable skills plan; #0b3x compose-skills implementation; composable-skills packaging; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca30...
recursively commit + push all uncommitted files in the submodule. Note some origins are not available anymore. Commit + push agent-loop.
### Re: recursive submodule commit + push — opus-4-7

Walked the submodule tree inside-out, committing and pushing each dirty repo:

- `mail/mailboxes/lucas` → `Sync shared conversations` (100 files, +29478/-34) — pushed to `btakita/mailbox-lucas` `main`.
- `mail` → `Bump lucas mailbox` — pushed to `btakita/mail` `main`.
- `src/boost-client/src/astro-listings` → `Update .gitignore and node_modules lock` — pushed to `rberkes/astro-listings` `main`.
- `src/boost-client` → `Bump astro-listings and update task notes` — pushed to `btakita/boost-client` `main`.
- `src/equityfundingsource-dev` → already clean (`tmp/` is gitignored; foreach output was stale).
- All other submodules (`src/agent-doc`, `src/agent-kit`, `src/corky`, `src/tmux-router`, `src/tsift`, `src/session-share`, etc.) were already clean — earlier `M` entries in the session-start snapshot reflected pointer bumps that prior cycles had already committed.
- agent-loop superproject → `Bump mail and boost-client submodules` — pushed to `btakita/agent-loop` `main` (`088981fa2..83e5305e8`).

No origin failures encountered this pass — all pushes succeeded. Session document closing through `finalize`.
<!-- agent:boundary:aba27cbd -->
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
