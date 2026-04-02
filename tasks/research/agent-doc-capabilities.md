---
agent_doc_session: d902e0e1-b492-4eac-a003-2ee9e07bd070
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Researched agent-doc subagent/worktree capabilities. Infrastructure exists (worktree.rs, parallel.rs), but no intelligent routing or metrics layer.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Can agent doc identify from the context, weighing differentent factors whether or not to use subagents + worktrees?:
- token cost
- success rate
- quantifiable metrics

Pleas
### Re: Can agent-doc identify when to use subagents + worktrees? (HEAD)

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
<!-- agent:boundary:431d48e5 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
