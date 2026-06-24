# Plan: OpenCode agent-doc active-queue drain stall (#ocdrainstall)

## Problem

An OpenCode agent-doc session with an `agent:queue auto` active queue stalls after each committed closeout. Even when preflight reports `queue_active: true`, `queue_continuation_required: true`, and `queue_drainable_head_count > 0`, the agent stops and returns control to the operator, who must manually re-invoke `agent-doc <FILE>` once per head. Observed in the `lazily-rs.md` session: a 5-head queue required an operator re-invocation for every single head drained.

## Root cause

The agent-doc SKILL documents three harness auto-loop strategies but leaves OpenCode ambiguous:

- **Claude Code** — after a proven closeout, the agent invokes the `Skill` tool with `skill: "loop"` (the `/loop` skill) plus `agent-doc drain-claim <FILE>`. The `/loop` skill self-paces and re-dispatches.
- **Codex** — `.codex/hooks.json` registers a `Stop` hook (`agent-doc hook codex-stop`) that blocks final-answer delivery after a clean closeout while `agent:queue auto` is active and instructs Codex to continue the next head **in-pane**, backed by a durable `.agent-doc/queue-continuations/<doc-hash>.json` marker.
- **OpenCode** — the SKILL says only "OpenCode currently has no auto-loop." Agents (correctly but wrongly) read this as *“stop after each closeout.”* It actually means *“no automated loop mechanism exists”* — there is no OpenCode `Stop` hook analog (`.opencode/config.json` has none; opencode 1.17 ships `commands/` and `skills/` but no lifecycle hook the binary can register against), and the `/loop` skill is unavailable. The intended behavior — that the **agent self-drives in-turn** by running another `preflight → respond → finalize/write --commit` sequence in the same assistant turn until the queue drains or hits a real stop condition — is never stated.

Two compounding effects:
1. The agent stops after each proven closeout instead of continuing in-turn (interpretation error, induced by ambiguous docs).
2. The agent substitutes judgment for draining (e.g. declaring remaining heads “redundant” and offering to “fast-forward”), which is the `#drain-no-defer` anti-pattern: an agent-drainable head must be drained, not deferred.

Secondary (not the core bug): early cycles in the observed session were legitimately blocked by a stale supervisor binary (0.34.46 → editor-IPC corruption → failed closeouts), which IS a real stop reason. That was resolved via `--force-disk`; the later stalls are purely the doc/interpretation gap.

## Fix

### Part A — SKILL/runbook clarification (low-risk, high-impact, ship first)

Rewrite the OpenCode auto-loop guidance in:
- `.opencode/skills/agent-doc/SKILL.md` (and the shared root `SKILL.md` auto-loop section)

to explicitly state the **self-drive** contract:

> OpenCode has no automated loop hook. When preflight reports `queue_active == true` AND `queue_continuation_required == true` AND `queue_drainable_head_count > 0` after a proven closeout (`committed` + `session-check` OK), **do not stop**. In the same assistant turn: run `agent-doc drain-claim <FILE>`, then run the next `preflight → respond → finalize/write --commit` cycle. Repeat until the queue drains or a genuine stop condition is hit (failed closeout, `session-check` interruption, `queue_continuation_required == false`, or an `[operator-verify]`-only head). High session-accretion is NOT a stop reason — it is owned by compaction, not by halting the drain. Mirror the `#drain-no-defer` rule: never substitute “this head is redundant, skip it” for draining it.

Also add an explicit “do NOT interpret ‘no auto-loop’ as ‘stop after each closeout’” callout.

### Part B — OpenCode auto-loop plugin (CONFIRMED FEASIBLE — ship this)

**Correction of the original premise:** OpenCode is *not* missing lifecycle hooks. Per https://opencode.ai/docs/plugins, OpenCode's plugin system exposes lifecycle events — most importantly **`session.idle`** (fires when a session finishes a turn) plus `tool.execute.before`/`tool.execute.after`, `message.updated`, and `tui.prompt.append`. The Codex auto-loop uses a `Stop` hook; the OpenCode equivalent is a **plugin subscribing to `session.idle`**. agent-doc simply never wrote that plugin — it only shipped (a) Codex's `.codex/hooks.json` `Stop` hook (`agent-doc hook codex-stop` + the durable `.agent-doc/queue-continuations/<doc-hash>.json` marker) and (b) Claude Code's `/loop` skill. "OpenCode currently has no auto-loop" is therefore an **agent-doc provisioning gap**, NOT an OpenCode limitation.

Ship a project/global plugin, e.g. `.opencode/plugins/agent-doc-queue.ts`:

```ts
import type { Plugin } from "@opencode-ai/plugin"
export const AgentDocQueueLoop: Plugin = async ({ client, $ }) => {
  return {
    event: async ({ event }) => {
      if (event.type !== "session.idle") return
      // agent-doc already writes a durable continuation marker on every clean closeout
      const res = await $`agent-doc session-check <FILE> --opencode-continue-gate`.quiet()
      // exit nonzero ⇒ a clean doc still owes an `agent:queue auto` continuation
      if (res.exitCode !== 0) {
        await client.session.prompt(...)   // re-dispatch `agent-doc <FILE>` for the next head
      }
    },
  }
}
```

Key design points (mirrors the proven Codex path):
- Reuse the existing `.agent-doc/queue-continuations/<doc-hash>.json` marker (already written by the binary on every clean closeout) as the drain signal — the plugin does not re-derive drainability.
- Add `agent-doc session-check <FILE> --opencode-continue-gate` (analogous to the existing `--codex-final-gate`) that exits nonzero whenever a clean document still owes an `agent:queue auto` continuation.
- Fail closed: if the marker says "continue" but the head did not advance on the prior dispatch, stop instead of looping (mirrors `#codex-self-reinvoke-prevent` / the non-advancing-repeat rule).
- Install via `agent-doc skill install --harness opencode` (write the plugin + register it in `opencode.json` `"plugin"` list, the same way it today writes `.codex/hooks.json`).
- The plugin path is **durable**: unlike agent self-drive (Part A), it survives an agent that misreads the SKILL, because the harness — not the LLM — re-dispatches.

### Part C — SimWorld / regression coverage

Add a SimWorld harness test that drives an OpenCode-style self-driven drain: a 3-head `agent:queue auto` queue is fully consumed in a single agent turn (no operator re-invocation), asserting `queue_drainable_head_count` reaches 0 and each head’s response is committed. This locks the Part A contract.

## Acceptance

- An OpenCode agent-doc session with an active auto queue drains consecutive heads **in a single turn** without operator re-invocation, stopping only at a genuine stop condition.
- The SKILL no longer reads as “stop after each closeout” for OpenCode.
- SimWorld proves the multi-head in-turn drain.

## Files

- `.opencode/skills/agent-doc/SKILL.md` — Part A self-drive guidance
- `SKILL.md` (root shared) — auto-loop section OpenCode paragraph
- `src/agent-doc/...` — Part B `opencode-stop` hook (if opencode supports hooks)
- SimWorld test — Part C
