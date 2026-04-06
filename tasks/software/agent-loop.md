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
**Decisions/outcomes:**
- Auto-compact: user wants more control; aggressive auto-compact unwanted. Partial auto-compact (keep active topics, archive older) preferred but not yet implemented. `agent_doc_auto_compact: 250` set as safety net.
- Eval tuning (all done): system prompt added explaining do/conciseness/### Re: header conventions; token budgets tuned per case; `simple-question-concise` fixed (0.3→0.7) by clarifying headers are multi-topic only.
- Partial compact feature was considered but dropped from pending by user.
<!-- agent:boundary:bb7a3e75 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] Fix simple-question-concise eval (system prompt vs expected_behavior conflict)
<!-- /agent:pending -->
