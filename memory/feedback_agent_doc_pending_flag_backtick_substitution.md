---
name: feedback_agent_doc_pending_flag_backtick_substitution
description: backticks in agent-doc --pending-add/--pending-edit text are eaten by zsh command substitution and silently strip content
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 08bdde9b-eeac-4a3b-b18a-6dd07ee901ee
---

When passing `--pending-add` / `--pending-edit` (and any text arg) to `agent-doc` through Bash, **never wrap code refs in backticks** inside a double-quoted argument. zsh evaluates `` `...` `` as command substitution BEFORE agent-doc runs, so segments like `` `agent-doc queue sync` `` execute as commands (errors: `command not found`) and are stripped from the value — the backlog item lands with silent gaps (`a -tagged backlog`, `or manual  does`).

**Why:** the closing heredoc for the response body is single-quoted (`<<'RESPONSE'`) so it's safe, but `--pending-add "..."` flag values are normal double-quoted shell words and undergo backtick substitution.

**How to apply:** in pending/review text use plain words or single quotes for inline code (e.g. `'agent-doc queue sync <FILE>'`), not backticks. If backticks are required, single-quote the whole flag value (watch for embedded single quotes). Repair a corrupted item with `agent-doc write <FILE> --pending-edit "<id>=<corrected text>"`, then commit via `agent-doc preflight <FILE>` (preflight's step-2 commit absorbs agent-owned pending drift; `write --commit` alone errors `empty response` with no response body). Related: [[feedback_no_cleartext_secrets]].
