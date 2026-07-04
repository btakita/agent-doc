---
name: feedback_deep_supervisor_refactor_fresh_focused_cycle
description: "Deep live-supervisor-critical refactor heads go to a FRESH focused cycle, not crammed into a long in-session /loop drain (operator chose 2026-06-22)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b425ba2a-7dd0-498c-8734-137b7a3191b8
---

When an `agent:queue` `do [#id]` head is a **deep live-supervisor-critical refactor** — one whose own plan scopes it as "a clean focused cycle with `make check` + `make tmux-ci` across two live panes" and whose correctness genuinely depends on the two-pane tmux-ci I can't run from the driving session (e.g. `#agentreloadrestart` Phase 1b: rebuild the supervisor `child_factory` / PTY child-spawn path to fresh-spawn a changed harness) — and the in-session `/loop` is already **several heavy features deep in one growing context**, the operator prefers ending the turn for a **fresh focused cycle** over cramming it inline.

Asked via AskUserQuestion 2026-06-22 (after draining log-validation + `#fbwire` + `#j9ja` + `#qnodemerge4wire` in one `/loop` chain), the operator chose **"Fresh focused cycle"** for `#agentreloadrestart` Phase 1b.

**Why:** thin self-contained additions with strong inertness/safety (a convergence gate that can only bounded-defer, editor-op reporters that fall back to diff-guess, a log marker) are verifiable at the `make check`/SimWorld level and safe to drain inline. A **refactor of the supervisor's deepest internals** (child-spawn factory, fresh PTY respawn) is high-blast-radius, has no SimWorld coverage of the actual execution, and only the live two-pane tmux-ci proves it — so a degraded 4-features-deep `/loop` context is the opposite of the clean cycle it needs.

**How to apply:** distinguish *thin addition* heads (drain inline: implement + `make check` + SimWorld + push, gate only the live-pane proof) from *deep supervisor/child-spawn refactor* heads. For the latter, when the `/loop` is already many heavy features deep, end the turn (no `ScheduleWakeup`) so the supervisor recycles to clean context (a queued `lib-install` recycle helps), and surface the fork via AskUserQuestion rather than auto-continuing. This is NOT general permission to stop on accretion (the loop contract says accretion alone is not a stop reason) — it is specific to a deep-supervisor-refactor head whose plan mandates a focused cycle + an unrunnable tmux-ci proof. See [[feedback_operator_drive_items_on_go_queue_churn]] and the auto-loop `#drain-no-defer` rule for the inline-drain default.
