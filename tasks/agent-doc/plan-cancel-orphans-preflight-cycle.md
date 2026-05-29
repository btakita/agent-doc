---
agent_doc_session: plan-cancel-orphans-preflight-cycle
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

# Plan: canceled run leaves an orphaned `preflight_started` cycle that blocks `Run Agent Doc` re-dispatch

**Bug id:** `#cancel-orphans-preflight-cycle`
**Status:** investigation + fix plan (triage capture)
**Severity:** medium — recoverable (the binary fails closed correctly and prints a recovery hint), but the UX traps the user behind a staleness wait after an explicit cancel.

## Symptom (reported 2026-05-29, live)

User cancels an in-progress run in IntelliJ, then immediately invokes `Run Agent Doc` again. The route fails closed with:

```
Error: authoritative actor generation 2 for tasks/monsterrodholders/mrh-project.md owns pane %30 but route could not drain the active closeout before dispatch: empty preflight_started cycle has no response capture for tasks/monsterrodholders/mrh-project.md: previous cycle `cycle-1780071335102` is still `preflight_started`, the live document has unresolved prompt_target: "Do step 3...", and no response exists to replay. The cycle is only 24s old; wait until it is stale or restart the harness pane and rerun.
```

## What is correct vs. what is the bug

- **Correct (not the bug):** the route *fails closed* instead of dispatching over an in-flight cycle, and prints an actionable recovery hint. This is the intended "do not drain an active closeout" behavior.
- **The bug:** an **explicit cancel** of the run does not abandon/reclaim the `preflight_started` cycle. The cycle stays open until the staleness window elapses, so the very next `Run Agent Doc` is blocked even though the user already abandoned that turn. The binary cannot distinguish "user canceled, reclaim now" from "still running, wait."

## Root-cause hypothesis

`preflight_started` cycles are only reclaimed by age (stale) or an explicit `start` from a fresh pane. There is no signal from the JB plugin that the run was canceled, so `cycle_state` keeps the cycle open. Related but distinct from the stale-*busy*-projection items: those concern the actor/lease busy projection; this concerns the per-document `cycle_state` phase left dangling by a cancel.

## Proposed fix options

- **Option A (recommended):** have the JB "cancel run" action emit a cancel signal (FFI / IPC) that abandons the current `preflight_started` cycle for that document (`cycle_state` → `Abandoned`) when no response capture exists. The next `Run Agent Doc` then starts a fresh cycle immediately. Shared-foundation: the abandon decision is a pure `cycle_state` function in the binary; the plugin is a thin reporter ("run canceled for <file>").
- **Option B:** shorten the reclaim window specifically for `preflight_started` cycles with no capture and a dead/cancelled harness pane (detect the pane is gone / idle-composer), so `Run Agent Doc` reclaims without waiting the full staleness window. Overlaps `#run-agent-doc-stale-busy-replay` / `#snrun`.
- **Option C:** surface the recovery as a one-click "reclaim and rerun" affordance in the plugin instead of a raw error dump (UX only; does not fix the root state).

Recommend **Option A** (explicit cancel → abandon cycle) as the smallest change that matches user intent, with Option B as a defense-in-depth for crashes where no cancel signal arrives.

## Related

- `#prompt-duplicated-while-typing` (review) — the IPC duplicate-line corruption the same report observed while typing into the post-exchange comment; plugin-side PatchWatcher double-count.
- `#run-agent-doc-stale-busy-replay` / `#snrun` — stale-busy projection + live replay regression (actor/lease, distinct from per-document cycle_state).
- `#jb-stale-busy-idle-footer` — idle-composer-footer-as-idle direct evidence for busy actor repair.

## Out of scope / do not implement yet

Triage capture only. No code changes until the cancel-signal seam (Option A) vs. shortened-reclaim (Option B) is chosen.
