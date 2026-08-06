---
name: reference_agent_doc_folded_queue_head_swallows_pasted_ids
description: "An editor paste can fold N `do [#id]` queue lines into one head; the extra lines become inert Freeform residue the mirror can never heal — fixed in 0.35.120"
metadata:
  node_type: memory
  type: reference
---

Pasting one `do [#id]` per line into `agent:queue` can arrive from the editor
with the bullet of every line **but the first** replaced by continuation indent:

```
- [#recommendedungatereview-b1cb]
  [#recommendedungatereview-9g0v]
  [#recommendedungatereview-zw66]
```

Observed 2026-08-03 in `tasks/agent-doc/agent-doc-bugs2.md`, committed as
`b48c6d6cf`: sixteen operator-authored ids collapsed into one head.

**Why it never heals itself — three reinforcing effects:**

1. `document_queue::parse` keeps line 1 as a `Prompt` and demotes lines 2..N to
   `QueueEntry::Freeform`. Freeform renders back verbatim (nothing is lost on
   disk) but is **not an addressable queue head**: it can never drain and
   `queue consume` / strike cannot reach it.
2. `turn_intent_checkpointed.active_queue_heads` therefore lists only the FIRST
   id. Draining/striking that head marks the whole block done after running one
   item.
3. `queue_entry_reference_ids` is multi-id aware (`#provauth2`), so the
   backlog→queue mirror sees all N ids as **already present** and never
   re-emits them as their own prompts. The fold is self-perpetuating.

**agent-doc did not create the fold** — proven by hashes: at 03:40:51Z the
editor buffer already held the folded text (`len=34539`), and the binary's write
at 03:41:04Z was a 50-byte response-cell update (`len=34538`). The fold arrived
from the editor/paste. `parse`→`render` round-trips `- do [#id]` lines
byte-identically, so the renderer is not the culprit either.

**Also proven:** `op_log` recorded **zero** per-item ops for a 16-item operator
queue paste, so reconciliation had only whole-document text to work from — the
case for [[project_agent_doc_percell_crdt_merge_intent]].

**Fixed in 0.35.120** (`heal_folded_id_per_line_prompts`, called from preflight
queue maintenance): re-segments the block into one prompt per id, **verbatim**
so an operator `do ` prefix survives, and emits a
`queue_folded_head_recovered` preflight warning. Fenced `---` prompts and
genuine multi-line prose continuations are left alone.

**Diagnosing state:** `state_events.payload_json` in `.agent-doc/state.db` holds
full document snapshots per fact (`document_baseline_checkpointed`,
`response_captured`) with timestamps — the fastest way to bracket when a
document shape changed. `grep -rl` over `.agent-doc/` gives FALSE NEGATIVES
(see [[reference_rtk_diff_filter_reports_identical_for_differing_files]]); scan
with python3.

**Related unfixed hazard:** a free-text queue head is struck by `#ftstrike` when
the response merely **echoes** it as `> **Queue prompt:**`. Nothing verifies the
work happened, so a cycle can strike a head without doing it (observed same day
on `haiven-dev.md`, `fc30a73` -> `4c0a270`, 34s apart).
