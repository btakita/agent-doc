---
name: reference-agent-doc-cross-document-backlog-add-trips-coined-id-guard
description: agent-doc's coined-id guard resolves ids against the CURRENT document only, so an id filed via --backlog-add-to into ANOTHER document is reported untracked by both session-check and the PreToolUse Bash hook.
metadata:
  type: reference
---

`agent-doc write --backlog-add-to <OTHER_DOC> "id=<x> ..."` files the item correctly in
`<OTHER_DOC>`'s `agent:backlog`, but the coined-id guard only resolves ids against the
document the cycle is closing. Both enforcement points then fire falsely:

- `session-check` warns `response coins new id(s) #<x> that are not in agent:backlog,
  agent:queue, agent:done, or agent:review` and hints to file a backlog item — which you
  already did, in the target document.
- The `coined-id-pre-tool-use` PreToolUse Bash hook **blocks the commit outright**:
  `[agent-doc] blocked: this Bash would write coined id(s) #<x> into commit message, but
  they are not tracked ...`. There is no bypass argument.

Observed 2026-09-26 on `tasks/agent-doc/agent-doc-bugs.md` closing an `#actionable-review`
drain that filed `#ckyonnx` into `tasks/software/corky.md`. `grep -c ckyonnx
tasks/software/corky.md` returned 1 and the item was a normal `- [ ] [#ckyonnx]` entry
under `<!-- agent:backlog priority queue -->`, so the id WAS tracked.

**Workaround:** write the id without the `#` sigil in the commit message (`file ckyonnx -
...`). The guard matches on `#id`, so the bare word passes and the commit lands. Do not
drop the cross-document item to satisfy the hook.

**Fix direction:** the guard must resolve an id against every document the cycle mutated,
not just the cycle's own document — `--backlog-add-to` targets are already known to the
write path.

Related: [[reference-agent-doc-coined-id-hook-blocks-c-preprocessor-directives]],
[[reference-agent-doc-backlog-queue-attribute-required-for-any-mirror]]
