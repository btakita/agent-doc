---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
agent_doc_auto_compact: 250
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
Completed releases: corky v0.26.3 (7-platform GitHub Release, fixed CI through 5 iterations: OpenSSL, host libssl-dev, aarch64 features), agent-kit v0.4.0 (GitHub Release). All submodules committed and pushed (84b98b52). Pending: litellm proxy + local model testing.
<!-- agent:boundary:3a2f3520 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] **corky v0.26.3**: released — GitHub Release + local install done
- [x] **agent-kit v0.4.0**: GitHub Release created
- [x] Commit and push submodules
- [x] Commit and push agent-loop
- [ ] Run litellm proxy pointing at vllm
- [ ] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->
