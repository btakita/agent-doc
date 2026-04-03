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

Research:
- What hooks can we add to measure token usage?e
- How about SKILL effectiveness?
- Should we add anything of our rust crates such as agent-kit or instruction-files?
### Re: Research — hooks, SKILL effectiveness, and rust crates (HEAD)

#### 1. Token usage measurement via hooks

Claude Code hooks are event-based (`UserPromptSubmit`, `Stop`) and execute shell commands — they're for operational automation, not observability. **No built-in hook exposes token metrics.**

Options for measuring token usage:
- **Session transcripts** — parse session data for usage metadata (most practical)
- **API-level instrumentation** — eval-runner and custom tools get token counts in API responses
- **`/cost` command** — per-session cost display, not per-skill
- **Custom `Stop` hook** — scrape session cost and log to a file (requires parsing Claude Code output)

#### 2. SKILL effectiveness measurement

No built-in mechanism. Approaches:
- **Eval-runner cases** — YAML cases testing skill behavior (30 exist for diff-interpretation already)
- **Session outcome tracking** — log success/failure per skill invocation
- **Before/after diffs** — compare document state to measure signal-to-noise

#### 3. Rust crates relevant to agent-loop

| Crate | Version | Role |
|-------|---------|------|
| **agent-kit** | v0.3.2 | Foundation toolkit for CLI tools in agent loops (audit, hooks) |
| **instruction-files** | v0.1.5 | AGENTS.md/CLAUDE.md discovery and auditing |
| **eval-runner** | v0.1.0 | LLM eval execution with YAML cases + LLM-as-judge |
| **module-harness** | v0.2.0 | Spec/contracts/evals embedded in source doc comments |
| **tagpath** | v0.5.1 | Tag-based identifier parsing (13 languages, tree-sitter) |

Also present: agent-doc (v0.31.4), corky (v0.23.0), tmux-router (v0.3.8), chromium-bridge (v0.2.1), webmaster (v0.1.2), agent-resume (v0.1.0), lazily-rs (v0.1.1).

**Recommendation:** agent-kit + instruction-files are the compliance foundation. For eval infrastructure, invest in eval-runner + module-harness.
<!-- agent:boundary:7426cd63 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Investigate session transcript parsing for token usage tracking
- [ ] Design SKILL effectiveness eval cases for eval-runner
- [ ] Determine which crates need evals coverage
<!-- /agent:pending -->
