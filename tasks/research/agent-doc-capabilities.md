---
agent_doc_session: d902e0e1-b492-4eac-a003-2ee9e07bd070
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Added subagent decision framework: 7 criteria (context state, model, topic spread, file overlap, duration, reversibility, token economics), 6 heuristics, 4 anti-patterns.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Can agent doc identify from the context, weighing differentent factors whether or not to use subagents + worktrees?:
- token cost
- success rate
- quantifiable metrics

Please give me tips on what criteria + attributes that are better suited for subagents. Examples such as:
- context state
- model
- topic spread
### Re: Can agent-doc identify when to use subagents + worktrees?

**Short answer: Not yet.** The infrastructure exists, but the intelligent routing layer doesn't.

**What exists today:**

1. **`worktree.rs`** — Git worktree lifecycle management (create, diff, remove, cleanup per session). Creates isolated branches under `.agent-doc/worktrees/<session>-<index>`.

2. **`parallel.rs`** — Fan-out orchestration: spawns multiple `claude -p` processes across worktrees in tmux panes, polls for completion, collects results as markdown. Supports `--no-worktree` for non-isolated runs, `--dry-run`, per-task timeouts.

3. **SKILL.md** — The non-deterministic orchestrator (LLM decides). Currently delegates all routing decisions to the skill author or user, not to any automated heuristic.

**What doesn't exist:**

| Factor | Status |
|--------|--------|
| Token cost tracking | Not tracked. No per-task or per-session cost metrics. |
| Success rate metrics | Not tracked. No feedback loop on task outcomes. |
| Complexity estimation | No heuristic to estimate whether a task benefits from isolation. |
| Automatic routing | No decision engine that says "this task should use a worktree" vs "inline is fine." |
| Quantifiable metrics | No measurement framework at all — no telemetry, no scoring. |

**The gap:** The binary provides the *mechanisms* (worktrees, parallel fan-out) but the *policy* (when to use them) is entirely manual. The SKILL.md skill could theoretically include heuristics, but today it doesn't — the user explicitly invokes `agent-doc parallel` or the skill author decides.

**What an intelligent routing layer would need:**

1. **Task classifier** — estimate complexity, file-touch scope, independence from other tasks
2. **Cost model** — track token usage per task pattern, build a lookup for "tasks like X typically cost Y tokens"
3. **Success feedback** — record whether worktree tasks merged cleanly vs needed manual resolution
4. **Context budget** — know the remaining context window and decide whether to keep inline or offload
5. **Conflict predictor** — estimate merge conflict probability based on file overlap between parallel tasks

This is a feature that doesn't exist in Claude Code natively either — the `Agent` tool with `isolation: "worktree"` is available but the decision to use it is always made by the LLM or user, never by a heuristic engine.
### Re: Criteria for when to use subagents (HEAD)

Here's a practical decision framework based on real-world usage patterns:

**Use a subagent when:**

| Criterion | Inline (main context) | Subagent | Subagent + Worktree |
|-----------|----------------------|----------|---------------------|
| **Context state** | Task needs prior conversation history, decisions, user preferences | Task is self-contained; can be fully specified in the prompt | Task modifies files that could conflict with main work |
| **Model** | Needs the strongest model (complex reasoning, architecture decisions) | Routine/mechanical work — haiku or sonnet suffices | Same as subagent; worktree is orthogonal to model choice |
| **Topic spread** | Single-topic, linear conversation | Multiple independent topics that don't share state | Independent topics that also touch different files |
| **File overlap** | Editing files you're actively discussing | Read-only research, or editing unrelated files | Editing files that main context might also touch |
| **Task duration** | Quick (< 30s) | Long-running (research, multi-file analysis) | Long-running + produces code changes |
| **Reversibility** | Exploratory, might change direction | Well-defined deliverable, unlikely to need rework | Changes need review before merging to main |
| **Token economics** | High-value reasoning that benefits from full context | Bulk work that would bloat the main context window | Same as subagent |

**Specific heuristics:**

1. **Context pollution test** — Will this task's output (tool calls, file reads, search results) consume >5K tokens of context that won't be useful for the next user interaction? → Subagent.

2. **Independence test** — Can you write a self-contained prompt that fully specifies the task without referencing "what we discussed"? → Subagent. If you'd need to paste half the conversation → inline.

3. **Parallelism test** — Are there 2+ tasks with no data dependency? → Parallel subagents. Even if each task is small, the wall-clock speedup justifies the overhead.

4. **Model downgrade test** — Does the task require creative judgment, architectural reasoning, or nuanced understanding of user intent? → Strong model inline. Is it grep-and-summarize, format conversion, or mechanical refactoring? → Weaker model subagent.

5. **File conflict test** — Will the task modify files that the main context or other tasks might also modify? → Worktree isolation. Read-only or writing to new files → no worktree needed.

6. **Review gate test** — Should the user see and approve the changes before they land in the working tree? → Worktree (changes stay on a branch until explicitly merged).

**Anti-patterns to avoid:**

- **Over-delegation** — Spawning a subagent for a 3-line grep wastes more tokens on the prompt than it saves
- **Context amnesia** — Delegating a task that requires understanding user preferences or prior decisions; the subagent won't have that context
- **Premature parallelism** — Splitting a task into parallel subtasks when the subtasks actually depend on each other's outputs
- **Worktree overhead for read-only tasks** — Worktrees are for isolation of *writes*; pure research doesn't need them
<!-- agent:boundary:f67b1287 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
