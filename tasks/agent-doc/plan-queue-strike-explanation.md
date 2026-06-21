# Plan: free-text queue-head auto-strike must be explained + conservative (#qstrikeexplain)

## Problem (operator report, 2026-06-21)

> "I'm typing in a queue item and agent-doc automatically strikes it. This should
> not happen without a response written into the document explaining why it was
> auto-struck."

The `#ftstrike` free-text queue-head auto-strike (`queue_consume` / finalize closeout)
strikes an answered free-text head (`- foo` → `- ~~foo~~`) silently. Two distinct
failure modes the operator is hitting:

1. **No explanation.** When the strike fires, nothing in `agent:exchange` tells the
   operator *why* their queue line was struck. A line they typed just disappears
   (gets struck), which reads as agent-doc deleting their input. The operator wants
   every auto-strike to be accompanied by a written response that names the struck
   head and says it was treated as answered.
2. **Striking a mid-typed line.** The strike heuristic (`#ftstrike`, response-match
   based) can match a head the operator is *still typing* during a turn — a partial
   line that happens to fuzzy-match the response — and strike it before it was ever
   a complete, intended prompt. A strike should only fire for a head that the
   committed response demonstrably answered, never for an in-flight live-buffer edit.

## Goal

A free-text queue head is struck **only** when (a) the closeout is proven-committed,
(b) the response in this cycle actually answered that head (tightened match), and
(c) the response body records a one-line note naming each head it auto-struck and
why. No silent strikes; no striking a line still being authored.

## Phases

### Phase 1 — strike provenance + explanation (consumer side)
- In the finalize/`queue_consume` `#ftstrike` path, when a free-text head is struck,
  emit a structured fact (struck text + match basis) back to the response-composition
  layer so the skill can include a "> Auto-struck queue head: …" note. Decide whether
  the note is binary-injected (deterministic, preferred per the all-deterministic-in-
  binary rule) or skill-authored from a new `preflight`/finalize field.
- Prefer binary-injected: append a deterministic `<!-- agent:exchange -->` note line
  (or a dedicated `queue_strike_note` the closeout writes) so the explanation cannot
  be skipped by the agent forgetting to mention it.

### Phase 2 — conservative match (don't strike mid-typed lines)
- Tighten the `#ftstrike` match so a head is struck only when the response answered
  it: require the struck head to have been present in the **preflight baseline**
  (a stable pre-turn head), not a head that first appeared in the live buffer *during*
  this turn (an in-flight operator edit). Reuse the `op_affectedness` /
  `user_intent_prompt_changes` classification so a brand-new live-buffer queue line is
  never eligible for same-cycle strike.
- A head the operator added mid-turn defers to the next cycle (consistent with
  `#queue-user-edit-overwrite` / editor-wins): it is drained (and then struck) on the
  cycle that actually answers it, with an explanation.

### Phase 3 — tests + spec
- Unit: a head present in baseline + answered → struck WITH a recorded note; a head
  that first appears in the live buffer this turn → NOT struck.
- SimWorld: a finalize cycle that strikes a free-text head emits the explanation note
  into `exchange`.
- SPEC + `runbooks/respond.md` (`#qdeferstrike` section) aligned: every auto-strike is
  explained; mid-typed heads are never same-cycle struck.

## Related
- `#qheadstrike` (strike-all-answered-heads + recovery-path strike) — this builds on it.
- `#ftstrike` (`#qdeferstrike` in `runbooks/respond.md`).
- `#queue-user-edit-overwrite` / `#editorbufwin` (editor-wins for live-buffer queue edits).
- `#rt83` projection-lag phantom pin (a stale projection re-emitting a struck/again line
  can look like a spurious strike; verify the explanation logic against a lagging
  projection too).
