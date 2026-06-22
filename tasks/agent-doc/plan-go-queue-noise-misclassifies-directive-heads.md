# Plan — go-mode queue misclassifies real directive heads as stale noise (#goqnoise)

## Symptom

A `go`-mode `agent:queue` carrying a `preset=` (e.g. `preset="#spec-test-commit-push"`)
with plainly-actionable free-text lines reports `queue_continuation_required=false`
and `queue_drainable_head_count=0`, so the in-session loop refuses to drain and the
supervisor idle-watch defers — the queue **stalls with genuine work stranded**.
`session-check` labels the lines `queue_stale_noise_lines=N` and tells the operator
to "clear the noise lines", even though they are real feature requests.

Live repro (2026-06-15, `tasks/professional/equityfundingsource.md`). After the head
item drained, two genuine work items were left and both classified as noise:

```
- "CPA / attorney letter or supporting document" in https://.../accreditation and the dialog should allow multiple files. Use our standard multi-file upload component.
- deploy
```

`drainable_head_count` returned `0`; `queue_continuation_required=false`. The operator
had to intervene.

## Root cause

`queue_continuation::is_drainable_queue_head` (`agent-doc-orchestration/src/queue_continuation.rs`)
classifies a free-text head (no `#id`, not a `?` question, not a slash command) as
drainable **only** if it contains a verb from a hardcoded closed list
`QUEUE_DIRECTIVE_VERBS`. That list is incomplete and the heuristic ignores the
queue's own `preset`/`go` context:

1. **`deploy` is not in `QUEUE_DIRECTIVE_VERBS`.** A bare `deploy` head — a
   first-class operator action (`#commit-push-deploy`, `#spec-test-commit-push-deploy`
   presets) — is classified as noise. This is the most glaring gap: `deploy` is a
   shipped preset verb yet the drain classifier doesn't know it.
2. **The CPA line's actual imperative verbs are absent from the list.** Its directive
   is "the dialog **should allow** multiple files. **Use** our standard multi-file
   upload component." Neither `allow` nor `use` is in `QUEUE_DIRECTIVE_VERBS`, and the
   line leads with a quoted noun phrase, so the verb-presence scan finds nothing and
   demotes a clear feature request to noise.
3. **The `preset` is ignored.** When a queue block carries `preset="#spec-test-commit-push"`
   and `go`, the operator's intent is that the preset supplies the action applied to
   **every** line — each line is a directive *by construction* (the line is the
   object, the preset is the verb). The noise filter evaluates each line in isolation
   as if it had to carry its own verb, which contradicts the preset contract.

A closed verb-list is inherently brittle: every legitimate phrasing the operator
invents that doesn't happen to contain a listed verb silently stalls the queue, and
the failure mode is invisible (it reads as "no work left / clean the noise").

## Fix options (do NOT implement yet — operator chooses)

### A. Preset-aware drainability (preferred)
When the enclosing `agent:queue` carries a `preset=` attribute (and/or `go`), treat
every non-empty, non-fenced `Prompt` line as a drainable directive head — the preset
is the verb. Thread the queue-level preset/go context into `is_drainable_queue_head`
(or gate the noise demotion at the `first_drainable_head` / `drainable_head_count`
call sites where the queue attrs are already in scope). Inert noise demotion (fenced
console pastes, bare observations) still applies **only** to queues with no preset.
This directly matches the operator contract and removes the closed-list brittleness
for the common case.

### B. Add the missing verbs
At minimum add `deploy` (shipped preset verb) and the common imperatives `use`,
`allow`, `support`, `show`, `match`, `set`, `change`, `replace`, `convert`,
`migrate`, `sync`, `upload`, `download` to `QUEUE_DIRECTIVE_VERBS`. Cheap, but does
not fix the structural brittleness — the next un-listed phrasing stalls again.

### C. Broaden the heuristic beyond a verb list
Recognize imperative mood more robustly: a line that contains a URL plus an
action clause, or that begins with / contains a recognized directive **anywhere**
(not only first token), or whose first alphabetic token is a base-form verb. Higher
risk of re-admitting genuine pasted-observation noise; weigh against A.

Recommendation: **A** (preset-aware) as the primary fix, with **B**'s `deploy`
addition folded in regardless, since `deploy` being absent is an outright omission.

## Tests (deterministic; `queue_continuation.rs` `#[cfg(test)]`)
- `deploy` head (no preset) — document current vs. desired behavior for the chosen fix.
- Preset-bearing go-queue: every non-fenced line is drainable (option A);
  `drainable_head_count` counts them; `queue_continuation_required=true`.
- The live repro CPA line is drainable under the chosen fix.
- Regression: a fenced console-paste line stays noise even under a preset queue.
- Regression: a non-preset queue with a bare observation line stays noise.
- `queue_stale_noise_lines` no longer counts preset-queue directive lines.

## Verification
`make check` (0 FAIL) + clippy clean; `cargo build --release` + `cargo install --path .`
+ `agent-doc lib-install`; recycle. Live confirmation: a preset go-queue with a
`deploy` line and a descriptive feature line reports `queue_drainable_head_count>0`
and `queue_continuation_required=true` instead of `queue_stale_noise_lines`.

## Related
- `#goqueuestall` (`plan-go-queue-stall.md`) — the inverse case (undrainable
  clean-session/operator-verify heads correctly deferred). This bug is the
  false-**negative** twin: drainable heads wrongly deferred.
- `#goqstall2` — the `queue_stale_noise_lines` diagnostic introduced for fenced
  console pastes; this plan narrows its over-reach.
