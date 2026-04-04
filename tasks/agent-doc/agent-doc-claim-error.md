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
*Compacted. Features: commit-on-claim, auto-setup untracked files, --diff-only flag, skill runbook bundling. Conventions: `.ai/runbooks/` (project root, vendor-neutral), `runbooks/` (skill-scoped). v0.31.5 released (cargo + GitHub). Open: PyPI manylinux build, `.ai/runbooks` blog post.*

Should the `compact exchange` runbook also include summarizing the document in the current uncommitted state?
### Re: Compact exchange and uncommitted state (HEAD)

The runbook's step 2 already covers summarizing exchange content (decisions, facts, open items). The question is whether it should also capture **uncommitted user input** — text written after the last boundary marker that hasn't been responded to yet.

**Current behavior:** `agent-doc compact` archives everything inside `&lt;!-- agent:exchange --&gt;`, including uncommitted user input. The summary (passed via `--message`) is written by the skill, which sees the full exchange before compacting.

**Should the runbook explicitly mention this?** Yes — it's a subtle point. If the user has pending questions/instructions in the exchange when they request compaction, those should be:
1. Acknowledged in the summary (so they're not silently dropped)
2. Or preserved as active items after the archive pointer

This is a documentation-only change to the runbook — adding a note to step 2: "If the exchange contains unanswered user input, note it as an open item in the summary or address it before compacting."

The binary behavior is already correct (archives everything). The skill just needs to be mindful of pending user content.
<!-- agent:boundary:3a1cb9d1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release v0.31.5
- [ ] PyPI publish (needs manylinux Docker build)
- [ ] Update compact-exchange runbook: note about preserving uncommitted user input
<!-- /agent:pending -->
