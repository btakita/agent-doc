# Plan: repair stale-busy projection from a Claude Code idle-composer footer (`#jb-stale-busy-idle-footer`)

## Symptom

Three JB Run Agent Doc / Clear Session Context reports queued-instead-of-dispatch
or blocked Clear while the live pane footer showed a Claude Code idle composer:

- `monsterrodholders.md` (actor gen 233, pane `%19`)
- `equityfundingsource.md` (gen 35, pane `%3`)
- `tsift.md` (pane `%21`, footer reported as `22.4K (11%) ctrl+p commands`) — the
  strongest stale-busy candidate: Clear was blocked though the footer indicated idle.

## Root cause (code-confirmed)

`session_actor_cmd::live_pane_prompt_ready` (`src/session_actor_cmd.rs`) classifies
a live pane:

```rust
fn live_pane_prompt_ready(harness, captured) -> bool {
    if harness.has_busy_cue(captured) { return false; }
    if harness.last_prompt_candidate(captured)
        .is_some_and(|line| harness.is_dispatch_ready_prompt_line(&line)) { return true; }
    // … → false (AliveBusy)
}
```

Two gaps make a genuinely-idle Claude pane project as `AliveBusy`:

1. **Claude footer chrome is not ignorable.** `HarnessConfig::is_ignorable_output_line`
   (`agent-doc-orchestration/src/harness.rs`) handles only `codex` and `opencode`
   footers; for `claude` it is `_ => false`. So `last_prompt_candidate` (bottom-up
   first non-ignorable line) returns the Claude footer/status line (e.g. the
   reported `22.4K (11%) …` context indicator) instead of the `❯` / `⏵⏵ … (shift+tab
   to cycle)` composer one line above it. The footer is not a dispatch-ready prompt
   line, so `prompt_ready = false → AliveBusy`. This is the direct analogue of the
   existing `last_prompt_candidate_skips_codex_footer` behavior, which Claude lacks.

2. **Claude has no busy cue.** `HarnessConfig::dispatch_blocker_reason` / `has_busy_cue`
   return `None` for any binary that is not `codex` or `opencode`. So for Claude the
   first guard above never fires.

The existing dispatch-ready composer detection is already correct and safe:
`is_dispatch_ready_prompt_line` for `claude` matches bare `❯` / `⏵` and
`⏵⏵ … (shift+tab to cycle)`, and rejects `❯ <drafted text>`.

## Why the obvious one-line fix is unsafe

Just adding Claude footer chrome to `is_ignorable_output_line` (so the composer
becomes the last candidate) is **not** sufficient and is dangerous on its own.
During an active Claude turn the bottom region is roughly:

```
❯ <input box>
✶ … (esc to interrupt)        <- spinner / busy line
<footer / context indicator>  <- would become "ignorable"
```

If only the footer is skipped, a bottom-up scan can surface the bare `❯` input box
and mis-classify a busy turn as idle → Clear/dispatch would clobber a live turn.
The fix therefore **must** also give Claude a busy cue so the first guard
(`has_busy_cue → false`) fires before any composer-based idle inference.

## Safe fix design (two parts, both required)

1. **Add a Claude busy cue.** Extend `dispatch_blocker_reason` (and thus
   `has_busy_cue`) with a `claude` branch that returns a reason when recent pane
   output contains the active-turn marker (`esc to interrupt`, and/or the
   `✶/✳/·` working spinner + elapsed timer). Mirror the codex branch shape
   (scan the last N lines, lower-cased).
2. **Make Claude footer chrome ignorable.** Add a `claude` arm to
   `is_ignorable_output_line` matching the **static** idle footer/status line(s)
   only — never the spinner. With the busy cue in place, `last_prompt_candidate`
   then resolves to the `❯` / `⏵⏵` composer for a genuinely idle pane → `AliveIdle`,
   repairing the stale busy actor/lease projection before queue/Clear-block.

Ordering safety: part 1 guarantees a busy turn fails the `has_busy_cue` guard first,
so part 2 can only surface the composer for panes with no active-turn marker.

## Ground truth — live captures (2026-05-29, this Claude Code session)

Captured real Claude Code panes (`tmux capture-pane -p`). This **corrects** the
report's footer description (`22.4K (11%) ctrl+p commands` does NOT match Claude
rendering — `ctrl+p commands` is an OpenCode marker; that pane was likely OpenCode
or the description is inaccurate).

**Idle Claude (panes %8 Opus, %22 Sonnet) — last meaningful lines:**
```
────────…────────
❯
────────…────────
  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664
  ⏵⏵ bypass permissions on (shift+tab to cycle)
```
The last meaningful line is `⏵⏵ bypass permissions on (shift+tab to cycle)`, which
`is_dispatch_ready_prompt_line` **already** matches → `ready_prompt_candidate`
returns ready. So the common idle case is already detected correctly.

**Busy Claude (pane %33, `tasks/resume.md`, mid-turn):**
```
· Roosting… (14s · ↓ 487 tokens · thinking with high effort)   <- active-turn spinner
────────…────────
❯
────────…────────
  Opus 4.8 (1M context) ctx:23% ~/…/agent-loop/resume main brian@cachyos-x8664
  ⏵⏵ bypass permissions on · 1 shell                            <- NO "(shift+tab to cycle)"
```
During an active turn the bottom permissions line becomes `⏵⏵ bypass permissions on
· N shell` (no `(shift+tab to cycle)`), which `is_dispatch_ready_prompt_line` does
NOT match → `ready_prompt_candidate` returns not-ready. So the busy case is also
classified correctly today. **Critically, the `❯ ` composer is present in BOTH idle
and busy**, confirming a footer-skip-to-composer fix would false-idle a busy turn.

## Revised analysis

The common idle/busy Claude cases are **already** classified correctly by the
existing `(shift+tab to cycle)` / `· N shell` distinction in the bottom permissions
line. The `tasks/resume.md` "still starting" route error (pane %33) is therefore
most likely **correct fail-closed behavior** — the pane was genuinely busy/starting
and not at a dispatch-ready prompt within the wait window.

Two narrower, still-open questions need targeted captures before any code change:

1. **Is there a Claude state where the last meaningful line is the status line
   (`Opus … ctx:N% …`) rather than the `⏵⏵ …` permissions line?** In every capture
   the `⏵⏵` line was last (below the status line), so the status line never became
   the candidate. If some config/state drops the `⏵⏵` line, the status line would
   become last → false not-ready, fixable by marking only the `Opus … ctx:N% …`
   status line ignorable (safe: skipping it surfaces the `⏵⏵` line, not `❯`).
2. **Error-wording accuracy:** when a pane is busy on an *active turn* (spinner
   present) rather than cold-starting, the route error still says "still starting
   (waited Ns for claude startup)". Distinguishing "busy on an active turn" from
   "still starting" needs a Claude busy cue keyed on the active-turn spinner
   (`· <Gerund>… (<N>s · …)`), which is fragile (random gerund, variable suffix —
   `esc to interrupt` vs `thinking with high effort`). Lower priority; UX-only.

## BLOCKER — confirm the real failure mode before coding

A code change is only justified once we have a capture proving a *misclassification*
(an idle pane showing `⏵⏵ … (shift+tab to cycle)` while route still refuses, or a
state where the status line is the last candidate). Without that, the existing
behavior is correct and any change risks false-idle. Capture needed: `tmux
capture-pane -p -t <pane>` at the moment route reports the actor not ready, in the
state the operator believes is idle.

## Acceptance

1. New `harness.rs` unit tests: a captured idle Claude footer + `❯`/`⏵⏵` composer →
   `is_idle_chrome_only_output` / `last_prompt_candidate` resolve to the composer
   (idle); a captured busy Claude tail (`esc to interrupt`) → `has_busy_cue` true
   (busy) even with a footer present and a bare `❯` above it.
2. `session_actor_cmd` test: a stale projected `AliveBusy` actor over an idle Claude
   pane repairs to `AliveIdle` so `session clear` / Run Agent Doc dispatch is not
   blocked/queued; a busy Claude pane still fails closed.
3. Live JB replay (ties to `#run-agent-doc-stale-busy-replay`): Run Agent Doc on a
   stale-busy-projected idle Claude pane dispatches instead of enqueuing.
4. `make check` green; `make tmux-ci` reviewed for the test-bearing change.

## Files likely touched

- `agent-doc-orchestration/src/harness.rs` — `dispatch_blocker_reason` (claude busy
  cue) + `is_ignorable_output_line` (claude footer chrome) + unit tests.
- `src/session_actor_cmd.rs` — projection-repair test; no logic change expected
  (the existing `live_pane_prompt_ready` path already consumes both helpers).
- Possibly `agent-doc-orchestration/src/route.rs` if the dispatch-side busy
  projection needs the same idle-direct-evidence repair before enqueue.

## Related

- `#run-agent-doc-stale-busy-replay`, `#snrun` — live-replay regressions
  (`tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md`).
- OpenCode precedent: `is_opencode_idle_chrome_line` / `is_context_usage_status_line`
  (`harness.rs`) and `is_idle_chrome_only_output`.
