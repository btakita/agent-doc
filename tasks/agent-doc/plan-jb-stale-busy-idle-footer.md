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

## BLOCKER — needs live ground truth before implementing

This is exactly the live-tmux / pane-UI class the project gates behind real
captures + `make tmux-ci` + the live-replay item `#run-agent-doc-stale-busy-replay`.
Two strings must come from real captures (a wrong matcher here false-idles a busy
turn — a serious regression that dispatches/clears into a live Claude turn):

- **The exact Claude idle footer/status line(s)** to mark ignorable. The report's
  `22.4K (11%) ctrl+p commands` is ambiguous (`ctrl+p commands` is an OpenCode
  marker), so the literal Claude footer text for the running CLI version must be
  captured, not guessed.
- **The exact Claude busy/active-turn line** (`esc to interrupt` + spinner glyph /
  timer shape) to anchor the new busy cue.

Capture from the reported panes (`tmux capture-pane -p -t %21` etc.) or any live
Claude Code session in both idle and mid-turn states.

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
