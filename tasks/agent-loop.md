---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
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

<!-- agent:exchange -->

### GSD Review

**get-shit-done** (34k stars, MIT) is a meta-prompting / context engineering framework for Claude Code (and other AI coding agents). Key observations:

**What it does well:**
- **Context rot mitigation** — its core thesis is sound: spawn fresh subagents with clean context windows instead of accumulating conversation history. This is the same principle behind our subagent strategy in CLAUDE.md.
- **Spec-driven pipeline** — requirements → research → plans → execution → verification. Structured `.planning/` directory with markdown + JSON state files.
- **Multi-agent orchestration** — 15 specialized agents (debugger, verifier, planner, UI auditor, etc.) spawned by thin orchestrator workflows (41 workflows, 37 commands).
- **File-based state** — no database, everything is human-readable markdown in `.planning/`. Survives `/clear` and context resets.
- **Multi-runtime** — supports Claude Code, Gemini CLI, OpenCode, Codex, Copilot, Antigravity.

**Architecture:**
```
Commands (slash commands) → Workflows (orchestration) → Agents (fresh context)
                                                           ↓
                                                     CLI Tools (gsd-tools.cjs)
                                                           ↓
                                                     .planning/ (file state)
```

**Comparison with our setup:**
| Aspect | GSD | agent-loop |
|--------|-----|------------|
| State management | `.planning/` dir with templates | `tasks/` docs + agent-doc snapshots |
| Agent orchestration | Markdown-defined agents spawned per-task | Claude Code subagents + skills |
| Context engineering | Fresh context per agent spawn | agent-doc template mode (exchange/pending) |
| Verification | Dedicated verifier agent + plan-checker | `make check` + manual review |
| Scope | General project management framework | Workspace-specific tooling |

**Takeaways:**
- GSD is opinionated and heavy (37 commands, 41 workflows, 15 agents). Good for greenfield projects where you want a full lifecycle framework.
- Our agent-doc approach is lighter — the document IS the interface, no separate `.planning/` state dir.
- Worth borrowing: their verification patterns (dedicated verifier agent), codebase mapping agent, and the "thin orchestrator" pattern for workflows.

---

**Re: your skill idea** — your message appears truncated: *"I want to have a skill for general development and r"*. Could you complete the thought? I'm guessing something like "general development and research" or "general development and refactoring"?
<!-- /agent:exchange -->
