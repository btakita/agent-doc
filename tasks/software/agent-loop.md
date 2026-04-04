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
### Re: Research — hooks, SKILL effectiveness, and rust crates

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

eval-runner should run project specific evals.
We can add some general evals that may change over time.
I want to add an evaluation on token count + time to run commands. I want an expert AI Engineer to offer suggestions on how to reduce token usage + have faster responses.
### Re: eval-runner scope + token/time evaluation + optimization

#### Project-specific vs general evals

```
evals/
├── agent-doc/           # project-specific (30 cases exist)
├── corky/               # project-specific
├── general/             # cross-project, evolving
│   ├── token-efficiency/cases.yaml
│   ├── response-time/cases.yaml
│   └── instruction-adherence/cases.yaml
└── results/
```

#### Token count + time evaluation

eval-runner already captures per-case latency and tokens. Extend with budget constraints:

```yaml
- id: simple-file-read
  metrics:
    max_input_tokens: 5000
    max_output_tokens: 500
    max_latency_ms: 3000
```

Fail cases that exceed token/latency budgets (currently only LLM-as-judge scoring).

#### Optimization suggestions (reducing tokens + faster responses)

1. **SKILL.md audit** — agent-doc skill is ~6KB loaded every invocation. Review for redundancy
2. **`--diff-only` preflight** — skip full `document` field when only responding to diffs
3. **Subagent discipline** — direct tool calls are ~10x faster than subagents for simple lookups
4. **Auto-compaction** — compact exchange when it exceeds 2KB to prevent context bloat
5. **Checkpoint batching** — flush every 2-3 sections, not every section
6. **Model tiering** — use haiku for routine ops, reserve opus for complex reasoning
7. **Incremental diff hashing** — 1.5s diff computation is the preflight bottleneck
8. **Preflight caching** — cache document hash to skip diff when unchanged

Yes...are we using prompt caching and is this included in the list?
### Re: Prompt caching (HEAD)

**Yes — Claude Code uses prompt caching automatically.** Cached tokens cost ~90% less (0.30/MTok vs 3.00/MTok for Sonnet). System prompt, CLAUDE.md, and conversation prefix are cached between turns in the same session.

**Where it applies:**
- SKILL.md loads into system prompt → cached after first `/agent-doc` call in a session
- Conversation history → cached prefix grows each turn
- CLAUDE.md files → cached as part of system prompt

**Where it breaks:**
- New session = new cache (each tmux pane has its own)
- 21 stash panes = 21 separate caches
- eval-runner calls Claude API directly → needs explicit `cache_control` breakpoints (code change)

**Updated optimization priority:**

| # | Optimization | Impact | Status |
|---|-------------|--------|--------|
| 0 | **Prompt caching** | ~90% cost on repeated turns | Active in Claude Code; eval-runner needs it |
| 1 | `--diff-only` preflight | Reduce per-turn input tokens | Not started |
| 2 | Auto-compaction | Prevent context bloat over sessions | Not started |
| 3 | SKILL.md audit | Reduce first-call + cache size | Not started |
| 4 | Model tiering | Use haiku for routine ops | Not started |
<!-- agent:boundary:63838201 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Add `cache_control` breakpoints to eval-runner API client
- [ ] Extend eval-runner with token/latency budget constraints
- [ ] Create `evals/general/token-efficiency/cases.yaml` with baseline cases
- [ ] Implement `--diff-only` preflight mode in agent-doc
- [ ] Add auto-compaction threshold to agent-doc exchange components
- [ ] Audit SKILL.md files for redundant instructions
<!-- /agent:pending -->
