---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. JetBrains perf: GC 3.9GB blob, VCS refresh debounce 500ms, plugin timing instrumentation v0.2.42, state diagnostics logging. `agent-doc clean --archive`. Plugin version check in make precommit. Boundary (HEAD) bug: transfer now commits target so transferred headings are in git HEAD. All genuinely new headings get (HEAD). Perf issue was ephemeral (restart fixed).*
<!-- agent:boundary:155eb096 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Add `git::commit(target)` to transfer command
<!-- /agent:pending -->
