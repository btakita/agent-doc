---
name: reference_agent_doc_backticks_in_backlog_add_corrupt_item_and_coin_new_id
description: "Backticks inside `agent-doc write --backlog-add` text can truncate the item AND split it into a struck original id plus a live auto-coined id — write backlog text without backticks."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 2c19ab51-2bf2-4bae-b1f8-bc8cfb5608bb
  modified: 2026-07-30T15:29:42.320Z
---

Observed 2026-07-30 on `tasks/software/lazily.md`. A `--backlog-add` whose text contained
escaped backticks — `[#lzrsbenchbudget] lazily-rs \`make check\` never enforces benchmark
budgets...` — landed corrupted:

- The item text lost everything up to and including the backtick span: what survived began
  `[ enforces benchmark budgets on a pruned target/...`.
- The explicit id `#lzrsbenchbudget` did **not** survive on the backlog item. A new
  auto-coined id `#vnmr` was assigned to the surviving fragment.
- The `agent:queue` mirror kept a **struck** `~~do [#lzrsbenchbudget]~~` entry, so the queue
  read as if the item had been resolved. It had not been — nothing was in `agent:done` or the
  `.done.md` archive.

That last part is the dangerous half: a false "handled" signal in the queue for work nobody
did, discovered only because the *next* cycle's closeout named `#vnmr` as the queue head and
the id was unrecognizable.

**How to apply:** never put backticks in `--backlog-add` / `--backlog-edit` text. Use plain
double quotes around command names instead (`"make check"`). The heredoc-fed response body
between `<!-- patch:exchange -->` markers is fine — backticks there are safe; it is the
flag-argument path that corrupts.

**Repair:** there is no `--backlog-remove`, and `--done` on the orphan would record a false
completion. Use `--backlog-edit "<coined-id>=<full correct text>"` to restore the content
under the coined id, and name the original id inside the text so it stays searchable. Verify
by re-reading the item from disk — a silent truncation looks like a successful write.

Same family as [[feedback_no_hallucinate_correct_errors]]: the write reported success.
Related: [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]].
