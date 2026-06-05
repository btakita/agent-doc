# Plan: Claude session must not look idle while the main agent's turn is still running

**Source:** agent-doc-bugs2.md queue item — "Claude stopped the run with a background agent to build. agent-doc Claude should not present an idle Claude session while the previous turn is still running... The build should happen on the main agent... not a subagent."

**Related:** review item `#subagent-blocks-session` (item 31), `#opencode-post-turn-false-active`.

## Two distinct concerns

### (a) Build on the main agent, not a subagent — process conduct (no code)
What the operator saw: `make check` was dispatched and the Claude session appeared to hand work to an opaque "subagent." In fact no `Agent` subagent was spawned — the Claude Code harness **auto-backgrounded** the long-running `make check` Bash call (returning a background task id) and the `| tail` buffered output to EOF, so the session looked idle with no visible progress.

Corrected behavior (already applied this session): run long build/test on the **main agent**, stream to a **watchable log file**, and scan it with cheap greps for progress/errors/warnings instead of dumping output or letting it buffer. This is a skill/runbook conduct rule, not a binary change.

**Action:** add an explicit rule to the agent-doc skill + runbooks: "Long build/test/install commands run on the main agent and stream to a file the operator can watch; never present them as detached/opaque work, and never report an idle/available session while that work is in flight."

### (b) Session must not present as idle/available while a turn's work is in flight — harness-level
The deeper request: while the main agent is mid-turn (e.g. a multi-minute build), the agent-doc/Claude session should not look idle or accept a new turn. This is a **harness-scheduling** property that agent-doc cannot directly enforce — Claude Code owns tool scheduling and the between-tool "idle" presentation; agent-doc has no API to block it.

Design options (need a scope decision before implementation):
1. **Supervisor/route busy-lease while a turn is active.** The route-owned supervisor already tracks pane busy/active state. Extend it so a pane with an in-flight turn (including a long main-agent Bash) presents `alive-busy` and route/JB Run Agent Doc fails closed / queues instead of appearing idle. Overlaps `#opencode-post-turn-false-active` busy-cue detection.
2. **Stop/PreToolUse hook gate.** A hook that marks the session busy for the duration of background/long work and clears on completion, so a second dispatch is rejected. Mirrors the Codex `Stop`-hook backstop. Claude Code hook surface needed.
3. **Status surfacing only.** Cheaper: surface a `turn in progress` status (tmux `display-message` / footer) so the session visibly is not idle, without hard-blocking. Lowest risk.

**DECISION (operator-chosen 2026-06-05): option 3 — status surfacing only, on the supervisor layer**, plus the (a) conduct rule. No hard busy-lease yet (option 1/2 stays gated on the `#subagent-blocks-session` scope decision — which layer blocks, what counts as "the work", how the lease releases). Status-only is the low-risk, no-wedge choice; promote to a hard lease only if it proves insufficient.

### Implementation spec (focused route-layer follow-up)
This is a feature in the sensitive, known-buggy route/supervisor busy-detection (`#opencode-post-turn-false-active`), so it ships as its own cycle, not folded into an unrelated change:

1. **Status surfacing (`route.rs` / supervisor).** When the owned pane's busy-cue detection reports an in-flight turn (the same signal `#opencode-post-turn-false-active` hardens), surface a `turn in progress` status on the pane via tmux `display-message` (the existing managed-status channel — must NOT write to the child pane stdout/stderr where it perturbs prompt detection). Clear it when the pane returns to a dispatch-ready/idle prompt. No hard block — a second Run Agent Doc still routes per existing rules.
   - **SHIPPED (`236658e6`):** the *interaction-point* case — when an operator's dispatch-only Run Agent Doc lands on a busy active-turn pane and is queued, route now surfaces a tailored turn-in-progress + queued status on the pane (`emit_busy_route_queued_diagnostic`) instead of returning silently. Closes the silent-queued gap.
   - **REMAINING:** a *continuous* busy-cue monitor that surfaces/clears the status whenever a turn is active without an operator dispatch. Note: busy-cue (pane content) cannot detect a harness-*backgrounded* task (the agent's pane looks idle) — that case is fundamentally the hard-lease/harness-signal problem (option 1/2, gated on `#subagent-blocks-session`), not status-surfacing.
2. **Conduct rule (skill + runbooks).** Add to the agent-doc skill source + installed Claude/Codex/OpenCode mirrors (keep `audit-docs` green): "Long build/test/install commands run on the **main agent** and stream to a **watchable file** scanned with cheap greps; never present them as detached/opaque work, and never report an idle/available session while that work is in flight." Requires the standard SKILL-source → mirror regeneration, so it lands with the skill-surface change set rather than ad hoc.

**Verification:** a long main-agent command shows the non-idle status on the owned pane and clears on return to idle; `audit-docs` green after the skill/runbook edit.

## Verification
- Conduct rule: present in installed Claude/Codex/OpenCode skill surfaces; `audit-docs` green.
- Status surfacing (opt 3): a long main-agent command shows a non-idle busy status on the owned pane.
- Busy-lease (opt 1/2): live repro — start a long turn, attempt a second Run Agent Doc, confirm it fails closed / queues rather than presenting idle.
