---
name: project_provauth_series_status
description: "provauth1/2/3 status — what shipped, what remains (provauth3b blocked on crdtauth6)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 0f26ded8-69de-4ad0-8cad-796cdcceb05b
---

agent-doc **#provauth (operator provenance authority) series** status as of 2026-06-26:

- **#provauth1** (#f693) DONE — per-component authorship merge (operator cells theirs-win clean), submodule `3c9dd556`. The reframe stuck: `crdt.rs` has no yrs transaction origins, so provenance lives at the per-cell **merge boundary** (ours=binary, theirs=operator), not per-transaction tagging.
- **#provauth2** (#q9w3) DONE — multi-id dedup (`cd8f3522`) + operator-delete tombstone (`08df743c`). (q9w3 was never archived to `.done.md` by the churning session that "closed" it — it lingered only in a queue head; reaped 2026-06-26.)
- **#provauth3 binary-origin half** (#fpyh) DONE — shipped **0.34.57** (`296ffe51`). Replaces the manual-cleanup recovery guess + the `❯` prompt-prefix mis-stamp on compaction summaries with a **recorded binary-origin** check (`recent_exchange_compaction_timestamp` marker + new `line_is_binary_authored_compact_summary`). Fixed the exact bug this session opened on (post-`/clear` stale snapshot tripping `guard_no_stale_snapshot_reset_drift` + falsely INTERRUPTing session-check on the `❯`-stamped Session Summary).
- **#provauth3b** (gated) — operator-origin half (retire `content_ours`/stale-editor resurrection heuristics in favor of operator-origin checks). **Blocked on #crdtauth6** (the live operator-origin delta seam): origin is not "known" for operator edits until that lands, so it can't be faked with more content heuristics.

**Don't redo provauth1/2** — verify in HEAD (`git merge-base --is-ancestor 3c9dd556 HEAD`) before re-implementing. Related: [[project_agent_doc_percell_crdt_merge_intent]].
