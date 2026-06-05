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
   - **Continuous monitor — built event-driven, not busy-cue poll.** A busy-cue poll would need a new supervisor thread *and* still can't see a harness-*backgrounded* task (the agent's pane looks idle). The reliable signal is the harness turn lifecycle, so the monitor is hook-driven:
     - **SHIPPED — the mechanism (`agent-doc turn-status active|idle`).** Runs inside the agent's own pane and sets that pane's border title (`⟳ agent-doc: turn in progress` on `active`, cleared on `idle`) via `$TMUX_PANE` — no supervisor poll, no doc/pane resolution. Best-effort (no-op outside tmux; never fails the turn). Unit-tested.
     - **Solves the backgrounded case (answer to "is it reliably solvable?": yes).** Because Claude Code fires `Stop` only after the *whole* turn completes — including any Bash it auto-backgrounded and re-invoked on — the `Stop`→`idle` clear brackets the backgrounded window that pane busy-cue fundamentally cannot. Pane *content* never shows backgrounded work; the turn *lifecycle* does.
     - **OPERATOR STEP (live-gated): wire the hooks.** There is no automated Claude-settings hook installer (the `SessionStart`/`autoclaim` hook is configured the same way), so add to `.claude/settings.json`:
       `{"hooks": {"UserPromptSubmit": [{"hooks":[{"type":"command","command":"agent-doc turn-status active"}]}], "Stop": [{"hooks":[{"type":"command","command":"agent-doc turn-status idle"}]}]}}`
       Visibility requires tmux `pane-border-status` (e.g. `set -g pane-border-status top`). Live-verify: start a turn → border shows "turn in progress"; finish (incl. a backgrounded build) → it clears.
     - **SHIPPED — automated hook installer (`agent-doc turn-status install [--settings <path>] [--user]`).** Idempotently merges the hooks into `.claude/settings.json` (UserPromptSubmit→active; Stop **and** SessionStart→idle), preserving existing hooks. The SessionStart→idle clear is the reliability belt-and-suspenders: the turn-status Stop hook does only trivial/idempotent/best-effort work (clear a pane title), so a missed Stop leaves only a stale cosmetic title that self-heals at the next SessionStart — fundamentally decoupled from the consequential closeout work that makes the Codex Stop hook fragile. Validated end-to-end (writes/preserves/idempotent).
     - **Remaining follow-up:** a `.agent-doc/turn-active` marker (with timestamp, for self-expiry) so the route/supervisor can *read* turn-state programmatically and fail-closed — the bridge to the hard busy-lease (option 1/2); and live-verify that Claude fires the hooks against an installed config.
2. **Conduct rule (skill + runbooks).** Add to the agent-doc skill source + installed Claude/Codex/OpenCode mirrors (keep `audit-docs` green): "Long build/test/install commands run on the **main agent** and stream to a **watchable file** scanned with cheap greps; never present them as detached/opaque work, and never report an idle/available session while that work is in flight." Requires the standard SKILL-source → mirror regeneration, so it lands with the skill-surface change set rather than ad hoc.

**Verification:** a long main-agent command shows the non-idle status on the owned pane and clears on return to idle; `audit-docs` green after the skill/runbook edit.

## Verification
- Conduct rule: present in installed Claude/Codex/OpenCode skill surfaces; `audit-docs` green.
- Status surfacing (opt 3): a long main-agent command shows a non-idle busy status on the owned pane.
- Busy-lease (opt 1/2): live repro — start a long turn, attempt a second Run Agent Doc, confirm it fails closed / queues rather than presenting idle.
