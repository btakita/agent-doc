---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
agent: opencode
claude_model: claude-opus-4-6
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260511-154008.md`*

Compacted content:
- Archived 11 response topic(s): optimizing /context, plugins on demand; context audit; token optimization strategies; 8 more
- Prior summary/context: Completed releases: corky v0.26.3 (7-platform GitHub Release, fixed CI through 5 iterations: OpenSSL, host libssl-dev...
❯ My logitech webcam is not connecting to Zoom after the machine rebooted. Please troubleshoot.
### Re: spec created & items transferred — opus-4-6

Done. Created the spec in `tasks/tsift.md` with:

- **Spec: AST-Aware Code RAG** section — covers architecture (tsift + tsift-graph crate), per-submodule isolation tiers, storage layout, new subcommands, output contract, and development phases
- **8 new pending items** added to tsift.md covering the full implementation path:
  - `#ast1`–`#ast4`: tree-sitter integration + SQLite symbols + search wiring
  - `#grp1`–`#grp2`: tsift-graph crate + callers/callees subcommands
  - `#iso1`: per-submodule config + isolation
  - `#bamc`: graphify evaluation (transferred from here)

The existing tsift pending items (release workflow, audit subcommands) are preserved alongside the new ones.

Marked `#bamc` done here — it now lives in `tasks/tsift.md`.
<!-- agent:boundary:6ff88172 -->
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
<!-- /agent:done -->
