# Plan: queue head must not consume on halt/refusal responses

## Symptom

`agent-doc finalize` struck queue head `[#d97v]` (rendered as `~[#d97v]~`) after a halt response that explicitly stated: "Backlog left intact — no `--done`, no `--pending-gate`. The queue head consumes `[#d97v]` as processed (...) but the pending item itself stays open."

Observed in `tasks/agent-doc/open-code.md` cycle `619af4d0b` (2026-05-28 12:07:21).

Binary log:

```
[queue] response heading explicitly targets active queue head; allowing consumption
[queue] consumed: "[#d97v]" (remaining: 1)
```

The response body was a refusal/halt: it explained why `#d97v` should NOT be executed in the current cycle, recommended `agent-doc compact ... --commit`, and explicitly stated the backlog item should remain open. No `--done`, no `--pending-gate`, no `--pending-edit` was passed.

## Root cause

`src/agent-doc/src/write.rs:2466` — `response_explicitly_targets_active_queue_head` returns true whenever any `### Re: <topic> — <model>` heading in the response body matches the queue head ID (`response_explicitly_targets_queue_head` at `write.rs:2481`).

`response_topic_matches_queue_head` (`write.rs:2500`) accepts a heading that contains the queue head ID anywhere — `### Re: #d97v halt — opus-4-7` matches `[#d97v]` because the topic-after-`Re:` strip extracts `#d97v halt`, then `queue_prompt_done_id` walks the ID chars after `#` and gets `d97v`, which equals the queue head's `d97v`.

At `write.rs:1233-1242`, this heuristic flips `queue_consumption_allowed = true` even when the response is a halt:

```rust
if write_result.is_ok()
    && !queue_consumption_allowed
    && let Some(capture) = crate::capture::load_active(file)?
    && response_explicitly_targets_active_queue_head(file, &capture.response_body)?
{
    eprintln!(
        "[queue] response heading explicitly targets active queue head; allowing consumption"
    );
    queue_consumption_allowed = true;
}
```

There is no semantic check — the binary cannot tell "I addressed #d97v by halting" from "I addressed #d97v by completing it."

## Why the heuristic exists

The heading-match fallback covers the case where the harness composes a substantive response that names the queue head in `### Re:` but forgets to pass `--done <id>` on `finalize`. Without it, the queue would not advance even though the work was done. So the rule "heading mentions head → consume" was a reasonable forward-looking guess in the common case.

The halt case breaks that guess: the heading still mentions the head, but the body is a refusal.

## Fix options

**Option A — require an explicit completion signal.** Remove the heading-match fallback entirely. Queue consumption requires one of: `--done <id>`, `--pending-gate <id>`, `--pending-edit "<id>=..."` that changes the item text, or the response body containing a `<!-- agent:done id=<id> -->` marker. Trade-off: harnesses that forget `--done` will now leave items in the queue; preflight already surfaces "shadow" warnings so the regression is visible. Smallest blast radius; matches the principle that closeout signals should be explicit, not heuristic.

**Option B — add a halt opt-out flag.** New `--halt-queue-head` or `--leave-queue-head` flag on `finalize` / `write --commit`. When set, the heading-match heuristic is bypassed. The skill agent passes it on halt responses. Trade-off: the harness has to remember a new flag; without it, halts still strike.

**Option C — detect halt language in the response body.** Pattern-match phrases like "Not invoking", "Backlog left intact", "halt", "stopping the auto-loop", "refuse", "won't do this". Trade-off: fragile heuristic, locale-specific, slow to evolve, and language-only halts in non-English/non-default phrasing slip through.

**Option D — narrow the heading match.** Require the `### Re:` topic to match the queue head text exactly (no trailing words like `halt`, `defer`, `skip`). My halt heading was `### Re: #d97v halt — opus-4-7` — under exact-match, that would not consume because the topic `#d97v halt` ≠ `[#d97v]`. Trade-off: a substantive response like `### Re: #d97v workspace conversion — opus-4-7` would also fail to consume, even though it's clearly addressing the head. So the fallback becomes useless in its common case.

**Recommended:** Option A. The skill agent already passes `--done <id>` on completed work (it did so for `#stqc` and `#5x1r` in this same session). The heading-match fallback exists to repair forgetful agents, but the cost — silently striking halts — is worse than the regression of "forgot --done means queue head stays."

Option B is a fallback if removing the heuristic breaks existing direct-exec / Codex hook flows that don't pass `--done`. In that case, **invert** the default: heading-match no longer consumes; halts are the default; explicit `--consume-queue-head-on-match` is required for the harnesses that need the heuristic.

## Acceptance

1. Repro: `cat <<'R' | agent-doc finalize <FILE> --stream --origin skill` with a body containing `### Re: #foo halt — <model>` and no `--done foo` / `--pending-gate foo` / `--pending-edit foo=...` arguments → queue head `[#foo]` is NOT struck. Binary logs `[queue] skipped consumption because the active prompt did not target the queue head` (or equivalent under Option A).
2. Existing behavior preserved: `cat <<'R' | agent-doc finalize <FILE> --stream --done foo` with a `### Re: #foo done — <model>` body → queue head `[#foo]` IS struck.
3. New unit test in `write.rs` covers (a) halt heading + no `--done` → no consume; (b) work heading + `--done` → consume; (c) no heading match + `--done` → consume.
4. SimWorld test added under `sim_world.rs` that exercises the halt-response + queue-strike interaction across a multi-item queue: halt on item N should leave queue head at item N for the next cycle's preflight to see (assuming the operator manually re-engages without `queue_active: false`).
5. `make check` green.

## Out of scope

- Whether the halt response itself should set `queue_active: false` automatically. That is a separate UX question — for now, halts leave the queue in `queue_active: true` so the operator can decide whether to compact + resume.
- The session-accretion `block` → halt decision in the agent harness. That's an open question for the SKILL.md guidance; this plan only fixes the queue-strike side effect.

## Files likely touched

- `src/agent-doc/src/write.rs` — remove or invert the heading-match fallback at lines 1233-1242, possibly remove `response_explicitly_targets_active_queue_head` + `response_explicitly_targets_queue_head` + supporting helpers if Option A is taken cleanly.
- `src/agent-doc/src/codex_hook.rs:351` — the other caller of `response_explicitly_targets_active_queue_head`; the same fix applies there.
- `src/agent-doc/src/sim_world.rs` — add halt-response queue-strike regression test.
- `src/agent-doc/SPEC.md` — document the closeout signal contract for queue consumption.

## Related

- `tasks/agent-doc/agent-doc-bugs2.md` — backlog item references this plan.
- `tasks/agent-doc/open-code.md` — original repro session.
