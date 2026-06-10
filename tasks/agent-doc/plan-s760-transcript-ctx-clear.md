# Plan: transcript-token context-% pre-emptive `/clear` (`#s760wire`)

Goal: gate the pre-emptive queue `/clear` on a **real** context-usage %, derived
from the harness session transcript's token usage (not exchange size, not a
TUI-footer scrape), wired into route dispatch. Sending `/clear` wipes the agent's
context, so correctness here is safety-critical and the rollout stays behind the
existing default-off opt-in until live-verified.

## Already shipped (do not redo)

- `session_accretion::queue_context_reset_opted_in` — per-doc/project/global
  opt-in, **defaults off** (`agent_doc_queue_context_reset`).
- `session_accretion::clear_threshold_for_doc` — resolves the clear threshold %.
- `session_accretion::queue_context_reset_reason*` — current trigger, based on
  **exchange size / accretion** (the heuristic this plan replaces as the ctx%
  source).
- Predicate plumbing (`ContextResetDecision.shouldClearBeforeDispatch`) exists;
  `SubmitAction.kt` is a thin dispatcher with no ctx% — per Shared Foundation the
  ctx% source belongs in `route.rs`, not the JB plugin.

## Why not a TUI scrape

Observed footers vary by harness + config (Claude custom statusline `ctx:NN%`,
Codex `Context NN% used`, OpenCode `NN.NK (NN%)`), so scraping the pane is wrong
for the shipped product. Derive ctx% from the transcript token usage / model
context window instead.

## Phases (one backlog id per phase)

- **`#s760a` — harness-aware transcript locator + token reader.** For each harness,
  find the active session transcript and read cumulative token usage:
  - Claude Code: `~/.claude/projects/<project-hash>/<session-id>.jsonl`; the latest
    assistant entry's `usage` (input + output + cache_read + cache_creation).
  - Codex / OpenCode: their own transcript stores (confirm paths live; emit
    `unsupported` + skip rather than guess).
  Pure function over a transcript path → `Option<UsedTokens>`; unit-test against
  committed fixture transcripts per harness.
- **`#s760b` — model → context-window table + ctx% compute.** Map the resolved
  model to its context window (e.g. Opus/Sonnet 200k; keep a conservative default
  + warn on unknown). `pct = used / window * 100`, clamped. Unit-tested.
- **`#s760c` — route-dispatch gate + diagnostic.** Before route sends a queue
  prompt: if `queue_context_reset_opted_in` and `pct >= clear_threshold_for_doc`,
  decide clear. Emit exactly `[s760] clear-decision optIn=.. threshold=.. pct=..
  clear=..` to ops.log (and stderr only under verbose, per `#opencode-stdout-bleed`).
  Send `/clear` (existing tracked-clear path) only when `clear==true`. Default-off
  opt-in keeps this non-destructive until explicitly enabled.
- **`#s760d` — live-verify (operator).** On a throwaway opted-in doc, drive the
  exchange past the threshold and confirm `[s760] clear-decision ... clear=true`
  fires and `/clear` is sent exactly once at the boundary, with no false fire below
  threshold. Driveable via `scripts/xdotool-live-verify.sh` from a fresh session.

## Safety invariants

- `/clear` is destructive — never fire without the explicit `agent_doc_queue_context_reset`
  opt-in; default-off ships safe.
- Unknown model / missing transcript / unsupported harness → `pct=None` → **never**
  clear (fail safe, log the reason).
- Idempotent: do not re-fire `/clear` until usage crosses the threshold again after
  a reset (track `last_context_clear_at`, already present).

## Run as a fresh focused session

This is a multi-phase, harness-specific, destructive-if-wrong feature. Implement
`#s760a`→`#s760c` with unit coverage in a clean session, then operator live-verify
`#s760d`. Not suited to an end-of-drain degraded session.
