# Plan: route must not dispatch into a crashed/exited harness (bare shell)

## Report

Operator: "JB `Run Agent Doc` on virtuousai.com did not submit. Then it crashed."
(screenshot `tasks/agent-doc/img_48.png`, document `tasks/recruit/virtuousai.md`,
~14:03).

## What the screenshot shows

1. A `/model opus` switch had just run ("Set model to Opus 4.8 … saved as default";
   ".claude/settings.local.json pins Opus 4.6 — that applies on restart"), so a
   model change / restart was in play.
2. The pane shows `agent-doc /home/brian/work/btakita/agent-loop/tasks/recruit/virtuousai.md`
   typed at a bare **zsh `%` prompt**, NOT submitted, with the normal shell
   prompt below it (`~/work/btakita/agent-loop main 12 *5 !39 ?30 … 14:03:22`).

So the harness (Claude) had **exited/crashed to a shell**, and "Run Agent Doc"
then routed the `agent-doc <FILE>` trigger into that **dead shell** — where it
sits as un-run drafted text ("did not submit"). The trigger landing in a bare
shell is the diagnosable agent-doc bug; the harness crash itself is the
upstream cause.

## Two distinct issues

### A — Route dispatched into a crashed/exited harness (agent-actionable)

`route.rs` owns pane prompt detection + dispatch-ready gating ("starting actor
reroutes are prompt-gated"; "fail closed before input"). A bare interactive
**shell** prompt (`%` zsh / `$` bash) is NOT a harness dispatch-ready prompt, yet
the trigger was typed there. Either:
- route's readiness check accepted a bare shell prompt as ready, or
- the harness crashed AFTER readiness was proven but BEFORE/DURING the send
  (a race), and route did not re-verify a live harness prompt at send time.

**Fix direction:** route must positively identify a *harness* dispatch-ready
prompt (Claude/Codex/OpenCode prompt shapes) and treat a bare shell prompt as
NOT-ready → fail closed with claim/restart guidance (or auto-restart the
configured harness) instead of typing the trigger into a shell. Re-verify the
harness prompt immediately before the send to close the crash-mid-dispatch race.
Add a route diagnostic distinguishing "dispatched into shell (no live harness)"
from a normal dispatch. SimWorld coverage for the bare-shell-prompt case.

### B — The harness crash root-cause (needs operator input)

Why Claude exited to a shell on `recruit/virtuousai.md` around the model switch
is not reproducible from this document's logs (the crash is in the
`recruit/virtuousai.md` session's own logs / the now-dead pane scrollback). Like
`#tsiftmdcrash`, this needs the operator's crash output. **Operator action: paste
the crash error text / the `recruit/virtuousai.md` pane scrollback from the
crash moment, or the cycle id, so the harness exit can be root-caused.** Note the
`/model opus` + settings-pins-Opus-4.6-on-restart context — the crash may relate
to a model-switch/restart path.

### C — Cold-start race: dispatched into a still-starting harness (`#jbtsiftnosub`)

Operator-reported (2026-06-19, `tasks/software/tsift.md`): JB `Run Agent Doc`
**auto-started a new supervisor/tmux pane**, typed the `agent-doc <FILE>` trigger
into the Claude input, but **did not submit** it. Unlike issue A (a *crashed*
harness → bare shell), here the harness pane was freshly **cold-starting** — the
Claude TUI had not finished coming up to a dispatch-ready prompt when the route
typed the trigger, so the keystrokes landed in a not-yet-ready composer and the
submit (Enter) never registered as a submitted prompt.

This is the same root invariant as A but the *opposite* pane state: A is a
**dead** pane that looks not-ready and was wrongly treated as ready; C is a
**starting** pane that briefly accepts input but is not yet submit-capable. The
binary already has "starting actor reroutes are prompt-gated" (promote a
`starting` actor to `ready` only after a harness-specific dispatch-ready prompt
is observed) — issue C is that the *auto-start* path that creates a fresh pane
does not hold the dispatch behind that same gate: it sends as soon as the pane
exists rather than after the harness prompt is dispatch-ready.

**Fix direction:** when `Run Agent Doc` auto-starts a fresh pane/supervisor, the
dispatch must **wait for the harness dispatch-ready prompt** on that new pane
(bounded poll, the same prompt-shape detection route already owns) and re-verify
it **immediately before** the send — then submit. If the dispatch-ready prompt
never appears within the bound, fail closed with claim/restart guidance rather
than typing into a starting composer. Distinguish in ops.log:
`dispatch_into_starting_pane` (cold-start race) vs `dispatch_into_shell` (issue
A) vs normal dispatch. SimWorld coverage for the starting-pane
(accepts-input-but-not-submit-ready) case, mirroring the bare-shell case.

**Operator capture at repro:** `.agent-doc/logs/supervisor-stderr.log` + the new
pane's scrollback at send time, so the cold-start window (pane created → harness
prompt ready) is in the trace.

### D — Retry loop re-injects the trigger on every readiness poll (`img_52`)

Operator-reported (2026-06-20, `tasks/professional/equityfundingsource.md`,
screenshot `tasks/agent-doc/img_52.png`): after **restarting** an agent-doc
session, JB `Run Agent Doc` produced **~7 stacked copies** of
`agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
typed into the pane, **none submitted**.

This is the same root as A/C (dispatch into a not-yet-ready / post-restart pane)
but exposes a distinct, agent-actionable bug in the *retry shape*: the
readiness poll/retry loop **re-types the trigger text on each attempt** instead
of typing at most once. So a slow post-restart boot (the 29s "Still waiting for
claude … active claude turn" window from the monsterrodholders report) turns
into N accumulated, unsubmitted copies in the composer — the visible
"duplicate prompts with no submit." Even once the harness becomes ready, the
composer now holds N concatenated triggers, so a submit (if it ever fires) is
corrupt.

**Fix direction:** the route must separate *readiness polling* from *injection*.
Poll for the harness dispatch-ready prompt **without** typing; inject the
trigger exactly once, only after a dispatch-ready prompt is positively
identified, and re-verify immediately before the single send. On a retry after a
failed/aborted send, **clear the composer first** (or never re-type) so retries
cannot accumulate. A not-ready/booting/shell pane must fail closed (or wait
silently) **without** injecting any text. ops.log: record each injection with an
attempt counter (`dispatch_inject attempt=N`) so a multi-inject regression is
provable from logs; assert at most one injection per dispatch.

## Acceptance

- SimWorld: a route to a pane showing a bare shell prompt (no live harness) fails
  closed / triggers restart instead of sending the trigger; ops.log records the
  dead-harness diagnostic.
- SimWorld (`img_52`): a post-restart dispatch whose pane stays not-ready across
  several readiness polls injects the trigger **zero** times while not-ready and
  **exactly once** after the dispatch-ready prompt appears; ops.log shows a
  single `dispatch_inject attempt=1`, never N stacked injections.
- SimWorld (`#jbtsiftnosub`): an auto-start dispatch to a freshly-created pane that
  is still starting (input accepted, not yet submit-ready) waits for the harness
  dispatch-ready prompt before sending, then submits; ops.log records
  `dispatch_into_starting_pane` if the send is attempted before ready.
- Operator-provided crash output lets issue B be root-caused separately.
