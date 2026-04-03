---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
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

**Compacted** (details in git history, commits `f80f524`..`d058b75`):

1. **DEV.md files** created for agent-doc, corky, instruction-files
2. **Submodule commits** — multiple rounds of inside-out recursive commits across all dirty submodules + parent pointer updates
3. **Crate releases:** instruction-files v0.1.5, tmux-router v0.2.9→v0.3.2, tagpath v0.5.1, agent-doc v0.25.12, corky v0.21.1
4. **eval-runner** — created GitHub repo btakita/eval-runner, registered as submodule, scaffolded Rust CLI (~300 LOC)
5. **evals/** — 30 YAML test cases for diff-interpretation; keep YAML format (ecosystem standard). Details in tasks/software/eval-runner.md
6. **src/git.rs** — orphaned test file, safe to delete
<!-- agent:boundary:f4465278 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
