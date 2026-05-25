# Plan: Claude Code skill auto-loops `/agent-doc <FILE>` while queue is active

**Bug class:** harness UX / SKILL.md
**Related:** Codex `Stop` hook continuation (already in `.codex/hooks.json` per `specs/07-orchestration-commands.md:249`), [#queueadvgated] (already landed — queue auto-strike honors `agent:review [/]`)
**Severity:** medium — Codex auto-loops same-turn while queue is active; Claude Code requires the operator to retype `agent-doc <FILE>` each cycle

## Problem

Claude Code has no Stop-hook equivalent of Codex's `.codex/hooks.json` Stop entry that re-invokes `agent-doc <FILE>` when `queue_active: true` + `agent:queue auto` + ready next head. Each `/agent-doc <FILE>` invocation runs one preflight → dispatch → finalize cycle, then the turn ends. The queue head advances at the end of the cycle, but the next dispatch requires a fresh user invocation, so the user-visible behavior is "queue stalls between turns".

User wants: queue auto-completes by default; frontmatter caps iterations.

## Goal

After a successful `finalize` / `write --commit` cycle on Claude Code, if preflight reports `queue_active == true` AND `queue_trigger == "auto"` AND `queue_prompts` is non-empty AND the iteration counter hasn't hit the configured cap, the SKILL invokes `/loop agent-doc <FILE>` via the `Skill` tool to drive the next cycle. When any of those conditions fails, the loop terminates cleanly.

## Plan

### Phase 1: Iteration counter signal in preflight

**File:** `src/agent-doc/src/preflight.rs`, `src/agent-doc/src/ops_log.rs`

1. Emit a `queue_auto_loop_iteration: <n>` field in preflight's JSON output. `n` is the number of consecutive queue-driven cycles for this document since the last non-queue prompt or queue drain. Read from a sidecar file under `.agent-doc/state/queue-loop-counter.json` keyed by document hash.
2. Increment when the consumed prompt this cycle was the queue head (matches `queue_consumed` log line); reset when the queue drains, when the user types a non-queue prompt, or when the document leaves `queue_active: true`.

### Phase 2: Frontmatter / project-config iteration cap

**File:** `src/agent-doc/src/frontmatter.rs`, `src/agent-doc/src/project_config.rs`

1. Add `agent_doc_queue_max_iterations: Option<usize>` to both. Unset = unlimited.
2. Preflight surfaces the resolved cap as `queue_auto_loop_max_iterations: <n | "unlimited">`.

### Phase 3: Claude Code SKILL.md auto-loop instruction

**File:** `src/agent-doc/src/skill.rs` (Claude-harness-specific text), regenerate installed `~/.claude/skills/agent-doc/SKILL.md`

Add a new step at the end of the Claude-Code-specific closeout flow:

> **Auto-loop while queue active (Claude Code only):** After a successful `finalize` / `write --commit` cycle whose `session-check` returns OK, if `preflight.queue_active == true` AND `preflight.queue_trigger == "auto"` AND `preflight.queue_prompts.len() >= 1` AND (`preflight.queue_auto_loop_max_iterations` is `"unlimited"` OR `preflight.queue_auto_loop_iteration < preflight.queue_auto_loop_max_iterations`), invoke the `Skill` tool with `skill: "loop"`, `args: "agent-doc <FILE>"`. This drives the next cycle from the same Claude Code session. Skip this step on any failed closeout or session-check interruption — those need explicit operator attention.

Codex and OpenCode skip this step (Codex has its own Stop hook; OpenCode would need its own approach).

### Phase 4: Termination + safety

1. If the user types a non-queue prompt mid-loop, the next preflight's `prompt_bearing_changes` will include the user's text and the loop should NOT auto-re-invoke. Easy rule: only auto-loop when `prompt_bearing_changes` is empty OR is exactly the queue-synthetic prompt.
2. Hard cap default: if `agent_doc_queue_max_iterations` is unset and an environment kill switch (`AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP`, default `50`) is exceeded, the SKILL refuses to re-invoke and emits a stderr warning. Prevents runaway loops on malformed queues.
3. Stop fence handling: existing `--- stop` fence already halts the queue; the auto-loop check happens after preflight, so a stop-fence-induced `queue_active == false` naturally terminates the loop.

### Phase 5: Documentation + tests

1. Update `runbooks/harness-invocation.md` with the Claude Code auto-loop semantics alongside the Codex Stop-hook section.
2. Preflight unit tests for the new fields (`queue_auto_loop_iteration` counter behavior + cap resolution).
3. Skill install test that the regenerated Claude SKILL.md contains the auto-loop instruction and Codex/OpenCode do NOT.

## Testing

- Manual: queue with 3 do-items, `agent_doc_queue_max_iterations: 2`. After 2 auto-loop cycles, the SKILL refuses to re-invoke; user must manually invoke for the 3rd.
- Manual: queue with 3 do-items, no cap. SKILL auto-loops until queue drains.
- Manual: queue active + user types a non-queue prompt. SKILL does NOT auto-loop; the user's prompt dispatches first.
- `make check` clean.

## Open questions

- Should the default hard cap be configurable per-project (e.g. `agent_doc_queue_max_iterations_hard_cap` in `.agent-doc/config.toml`) or only via env var? Env-only is simpler; per-project is more flexible.
- Should `queue_auto_loop_iteration` be visible to OpenCode SKILL.md too in case OpenCode grows a Stop-hook equivalent? Cheap to add; safe to skip.
