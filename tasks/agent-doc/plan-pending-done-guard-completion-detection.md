# Plan: pending_done_guard completion detection — fix false negatives + false positives

| Field | Value |
|---|---|
| **Backlog** | tasks/agent-doc/agent-doc-bugs2.md (`#pdgd` — to be added) |
| **Status** | Planning only — do not implement until prioritized |
| **Reporter** | Brian Takita |
| **Observed** | 2026-05-20, session-share/tasks/root.md cycle "do [#arsq] [#axid] [#rprd]" |

## Symptom

A cycle was issued three explicit `do [#id]` directives. Each was fully executed:

- `[#rprd]` — three docs amended (`jesse-orbit-relationship.md` decision section + two PRDs with version bumps), committed to BuildParty/BuildParty `dev` and pushed.
- `[#axid]` — `build-party/specs/database-strategy.md` gained a new `agent_id` discriminator section, committed and pushed in the same commit.
- `[#arsq]` — `prd-orbit-agent-livekit.md` §4.1 formally requires `agent_id` on every `find_*` tool; `[#qew8]` gated via `--pending-gate`; backlog items `#p635` and `#fvtg` rescoped via `--pending-edit`.

The cycle's `agent-doc finalize` invocation issued:

```
--pending-gate qew8
--pending-edit "x63e=...RESCOPED [#axid] 2026-05-20..."
--pending-edit "p635=...RESCOPED [#arsq] 2026-05-20..."
--pending-edit "fvtg=...RESCOPED [#arsq] 2026-05-20..."
--pending-edit "2v00=...RESCOPED [#axid] 2026-05-20..."
```

No `--done arsq --done axid --done rprd` was passed. `session-check` printed:

> warn: response appears to complete existing pending #fvtg, #qew8, #p635 but no matching `--done` was recorded this cycle
> hint: re-run with --done fvtg --done qew8 --done p635 or add `pending_done_guard: off`

This is **doubly wrong**:

- **False positives:** `#fvtg`, `#qew8`, `#p635` are *not* complete — they were rescoped (`--pending-edit`) or gated (`--pending-gate`). The guard suggested closing items that the same cycle had just explicitly kept open.
- **False negatives:** `#arsq`, `#axid`, `#rprd` *are* complete — concrete spec edits, news entry, two commits (BuildParty/BuildParty@39d60e9 + ClaudeScore/SessionShare@bdfca27f) — yet the guard said nothing about them.

The user had to notice and ask manually, which defeats the purpose of `pending_done_guard`.

## Root cause hypothesis

The current `pending_done_guard` heuristic appears to scan the **`--pending-edit` / `--pending-gate` flag content** for mentions of other pending IDs and treats those mentions as "this response is talking about that item, so it might be done." That explains the false positives on `#fvtg` / `#qew8` / `#p635` — their IDs appeared inside the rescope text of other `--pending-edit` flags (e.g. `"x63e=...RESCOPED [#axid]..."`).

It also explains the false negatives: `#arsq`, `#axid`, `#rprd` were the *subjects* of the cycle (named in response headers and section bodies inside the `<!-- patch:exchange -->` block) but were **not** mentioned inside any `--pending-edit` flag content, so the heuristic never inspected them.

Conjectured failure path: `pending_done_guard` reads mutation-flag arguments rather than (or in addition to) the response body, and counts any pending-ID mention in mutation text as a completion signal.

## Proposed solution (3 layers)

### Layer 1 — exclude items that this cycle explicitly modified-but-not-completed

Any pending ID that appears in this cycle's:

- `--pending-edit <id>=...` (item rescoped)
- `--pending-gate <id>` (item externally blocked)
- `--pending-ungate <id>` (item unblocked but still open)
- `--pending-reorder ...` (item moved, not closed)

…must be **excluded** from the pending_done_guard scan. The cycle explicitly told the binary "this item is *not* done." Treat that as authoritative.

This single change kills the false positives observed above (`#fvtg`, `#qew8`, `#p635` would no longer be flagged because they appear in this cycle's `--pending-edit` / `--pending-gate` payloads).

### Layer 2 — positive completion signal from the response body

Scan the `<!-- patch:exchange -->` body (and the response text passed to `finalize --stream`) for completion phrasing keyed off pending item IDs:

| Signal | Example match | Interpretation |
|---|---|---|
| `### Re: do [#<id>] …` | `### Re: do [#arsq] [#axid] [#rprd]` | Cycle is responding to a `do <id>` directive |
| `[#<id>] — <verb-of-completion>` | `[#rprd] — decision doc + PRDs amended` | Item appears with a completion verb (amended, implemented, written, fixed, closed, landed, shipped) |
| `[#<id>]` + verifiable artifact | `[#rprd]` near a commit SHA, file path under a committed diff, or "pushed" / "merged" | High-confidence completion |
| Backlog item text echoed with past-tense verb | "Status → Decided 2026-05-20" in same response that mentions `[#x6vd]` | Item-subject achieved |

A pending ID that matches Layer 2 **and** is not excluded by Layer 1 → guard fires "appears to complete but no `--done`" warning.

### Layer 3 — preserve current "explicit override" escape hatches

Both `<!-- no-pending-capture -->` and `pending_done_guard: off` / `pending_done_guard: warn` frontmatter should continue to short-circuit the guard for documents where the heuristic is more trouble than it's worth.

## Test cases (must pass after fix)

1. **Cycle issues `--pending-edit X=...` referencing `[#Y]` in the edit text; response body does not address `[#Y]`** → guard MUST NOT warn on `#Y`. (False-positive regression covered.)
2. **Cycle issues `--pending-gate X`** → guard MUST NOT warn on `#X`. (Gating is explicit "not done.")
3. **Cycle response body contains `### Re: do [#X]` and a commit follows that touches the spec named in pending item `#X`'s text** → guard MUST warn on `#X` if `--done X` was not recorded. (False-negative regression covered.)
4. **Cycle response body contains `[#X] — implemented in commit abc1234`** → guard MUST warn on `#X` if `--done X` was not recorded.
5. **Cycle response body contains `[#X]` as a passing reference (e.g. "rescoping [#X] for clarity")** without completion verbs → guard MUST NOT warn (low-confidence; mention alone is insufficient).
6. **Item appears in both `--pending-edit` (with completion verbs in the edit text) AND in response body completion phrasing** → Layer 1 wins; guard MUST NOT warn (explicit modify-not-done beats heuristic).
7. **Frontmatter `pending_done_guard: off`** → guard MUST NOT warn regardless of body content. (Preserve existing escape hatch.)

## Files likely affected

- `src/agent-doc/.../session_check.rs` (or wherever `pending_done_guard` is implemented) — Layer 1 exclusion + Layer 2 body scan.
- `src/agent-doc/.../finalize.rs` (or write-stream entry point) — must forward the response body and the full set of mutation flags to `session_check` so Layer 1 has them.
- Tests under `src/agent-doc/tests/` — add the 7 test cases above; ideally one fixture-doc per case.
- Spec / runbook update: a short paragraph in `instruction-files/.../session-check.md` (or equivalent) describing the Layer 1 + Layer 2 contract.

## Non-goals

- Auto-issuing `--done` on the user's behalf. The guard exists to flag, not to mutate.
- Cross-document completion detection (a commit in one repo closing a pending in another) — out of scope; current proposal handles only single-document cycles.
- Detecting completion via commit-message ID references alone (without response-body signals) — possible Layer 4 future work.

## Open questions

- Does the guard already have access to the full set of mutation flags issued in the cycle, or does it only see the post-mutation state? If only the latter, Layer 1 requires plumbing changes.
- Should `--pending-add` of a new item that supersedes an existing item (e.g. "supersedes [#X]") be treated as a Layer 2 completion signal for `#X`? Tentatively yes, but mark for confirmation.
- Should the warning surface a positive identification ("looks like #arsq is done — pass `--done arsq` next time") in addition to listing IDs? Improves UX but increases false-confidence risk.
