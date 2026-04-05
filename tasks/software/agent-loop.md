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
### Re: Prompt caching

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

do
- [ ] Add `cache_control` breakpoints to eval-runner API client
- [ ] Extend eval-runner with token/latency budget constraints
- [ ] Create `evals/general/token-efficiency/cases.yaml` with baseline cases
- [ ] Implement `--diff-only` preflight mode in agent-doc
- [ ] Add auto-compaction threshold to agent-doc exchange components
- [ ] Audit SKILL.md files for redundant instructions
### Re: Execute optimization tasks

All 6 items done. Summary:

**1. `cache_control` breakpoints in eval-runner** ([done])
- Restructured `call_anthropic_api()` to send system prompt as a structured content block with `cache_control: {"type": "ephemeral"}`
- Added `Usage` response parsing: extracts `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`
- Logs cache hit/miss stats to stderr per API call
- Added `input_tokens`/`output_tokens` fields to `CaseResult` (serialized in JSON output)

**2. Token/latency budget constraints** ([done])
- Added `max_input_tokens`, `max_output_tokens`, `max_latency_ms` optional fields to `EvalCase`
- Budget violations checked after judge scoring in `run_case()`
- Violations append to reasoning string and force `passed = false` regardless of judge score
- Latency cases already had `threshold_ms`; new fields are for LLM-rubric cases

**3. `evals/general/token-efficiency/cases.yaml`** ([done])
- 5 baseline cases: `simple-question-concise`, `approval-minimal-response`, `single-line-edit-response`, `multi-topic-structured`, `no-changes-fast-exit`
- Each has `max_output_tokens` budget constraints (100-800 tokens depending on complexity)
- Tagged `[token-efficiency]` with 2 smoke cases

**4. `--diff-only` preflight mode** ([done] -- already existed)
- Flag `--diff-only` already implemented in agent-doc v0.31.14
- Omits `document` field from preflight JSON, reducing tokens on subsequent cycles
- SKILL.md already references it in the optimization table

**5. Auto-compaction threshold** ([done])
- Added `agent_doc_auto_compact` frontmatter field (line count)
- Checked in preflight step 2c: parses exchange component, counts lines, runs `compact::run()` if exceeded
- Only applies to template-mode documents with an `exchange` component
- Usage: `agent_doc_auto_compact: 200` (compact when exchange > 200 lines)
- All 38 agent-doc tests pass

**6. SKILL.md audit** ([done] -- findings documented)
- Total: 14 SKILL.md files, ~1,858 lines / ~7,400 tokens
- High redundancy: agent-doc (284 lines), dev (210), existence (180)
- Medium: browser+chromium-bridge overlap (314 combined), module-harness (142)
- Low: corky, email, linkedin, livestream, testing
- **Estimated savings: ~1,350 tokens (24%)** via runbook extraction, deduplication, and consolidation
- Recommended next steps: reduce agent-doc SKILL.md first (highest impact), then consolidate browser/chromium-bridge
do
- [ ] Apply SKILL.md reductions (agent-doc first, then browser/chromium-bridge)
- [ ] Run token-efficiency evals against baseline to establish metrics
### Re: SKILL.md reductions + eval baseline

**SKILL.md reductions applied:**

| File | Before | After | Reduction |
|------|--------|-------|-----------|
| agent-doc | 284 | 132 | 54% |
| browser | 150 | 41 | 73% |
| chromium-bridge | 166 | 53 | 68% |
| **Total** | **600** | **226** | **62%** |

Key changes:
- agent-doc: consolidated preflight/routing into tables, merged append/template write-back into single pipe command, removed duplicate baseline instructions, condensed document format section
- browser: removed all examples duplicating chromium-bridge, kept only MCP fallback table + safety + config
- chromium-bridge: removed "Common Patterns" section (duplicated individual commands), condensed chatbox formatting to one example + selector table

**Token-efficiency eval baseline (sonnet via Claude CLI):**

| Metric | Value |
|--------|-------|
| Pass rate | 4/5 (80%) |
| Mean score | 0.74 |
| P50 latency | 33.9s |
| P95 latency | 70.0s |

Results saved to `evals/general/token-efficiency/baselines/sonnet-2026-04-05.json`.

One failure: `approval-minimal-response` -- model doesn't know the "do" convention without system prompt context (expected). Token counts not available via Claude CLI backend; need `ANTHROPIC_API_KEY` in pass store for the anthropic-api backend to get `cache_control` metrics.

I don't have any budget for anthropic api. Can we use claude code instead?
> - [ ] Add ANTHROPIC_API_KEY to pass store to enable token-count evals
### Re: Using Claude Code CLI for token-count evals (HEAD)

Yes -- the Claude CLI `--output-format json` already includes full usage data. Updated eval-runner's `call_claude_cli()` to parse the JSON output instead of text, extracting:
- `input_tokens`, `output_tokens`
- `cache_creation_input_tokens`, `cache_read_input_tokens`

No Anthropic API key needed. Smoke test results with token counts:

| Case | Score | Output Tokens | Budget | Status |
|------|-------|--------------|--------|--------|
| `simple-question-concise` | 0.60 | 376 | max 300 | EXCEEDED |
| `no-changes-fast-exit` | 0.95 | 61 | max 100 | PASS |

Cache stats visible in stderr (e.g., `read=11163 created=12443 uncached=3`).

The `simple-question-concise` case now correctly catches verbose responses via the `max_output_tokens: 300` budget constraint -- the model used 376 tokens (25% over budget). This is exactly the kind of efficiency regression the eval is designed to catch.

Removing the ANTHROPIC_API_KEY pending item since it's no longer needed.
<!-- agent:boundary:1450a3b0 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Test auto-compact threshold on a real session document
- [ ] Add system prompt to token-efficiency evals for approval case
- [ ] Tune token budgets based on baseline (simple-question needs tighter prompt or higher budget)
<!-- /agent:pending -->
