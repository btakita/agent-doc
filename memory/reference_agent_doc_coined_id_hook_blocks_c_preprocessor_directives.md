---
name: reference_agent_doc_coined_id_hook_blocks_c_preprocessor_directives
description: Both historical coined-id PreToolUse guard defects are fixed — the C-preprocessor false positive and the fail-open ledger hole; neither workaround is needed any more.
metadata: 
  node_type: memory
  type: reference
  originSessionId: 8f0beb9b-0211-4d94-8c3f-9d86d1e04b69
  modified: 2026-08-01T21:21:08.461Z
---

Two defects in agent-doc's `coined-id-pre-tool-use` guard, **both now fixed**.
Keep this note only as history — if either symptom reappears, it is a
regression, not the known state.

**1. C-preprocessor false positive — fixed** (`e5a0af2fd`, 2026-07-31, another
session). `#include` / `#ifndef` / `#define` / `#endif` were read as untracked
coined ids, so no C-family file could be written with the Write tool.
`coined_id_scan_text` now blanks the leading `#` on directive lines when the
target extension is C-family. **The shell-heredoc workaround is obsolete.**

**2. The guard failed OPEN — fixed** (`4f3f4d2c4`, 2026-08-01).
`handle_pretooluse` mapped `None` to `Allow`, and five conditions funnelled into
that one `None`: no pane scope, no project root, a registry that would not open,
a registered document that would not read, a document that would not parse. The
last three are transient, so the guard silently switched itself off under
registry/document contention. Observed live: two consecutive Write calls carrying
an untracked id were permitted and one landed on disk.

`DocumentIds` now splits it — `Ungoverned` allows, `Known` decides, and
`Unavailable` **denies** after a bounded retry, but only for text that actually
carries an id-shaped token (untagged writes and C headers still pass, or every
write during a document rewrite would be refused).

**The durable lesson, independent of these two bugs:** a guard whose "I could
not check" path and its "nothing to check" path share a return value will fail
in the permissive direction, and it will do so exactly under load. Worth looking
for elsewhere in agent-doc.

Related: [[reference_agent_doc_backticks_in_backlog_add_corrupt_item_and_coin_new_id]],
[[reference_agent_doc_recovery_index]],
[[feedback_concurrent_agent_doc_refactor_session_build_coordination]]
