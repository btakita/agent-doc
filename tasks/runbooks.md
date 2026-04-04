---
agent_doc_session: 0f91fe16-548b-403c-9a53-79ebe1a9ed79
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Session Summary (HEAD)

**Runbook extraction session** (2026-04-04)

### Completed
- Extracted 14 runbooks from 4 SKILL.md files (dev, email, livestream, module-harness)
- Token savings: ~2,235t (34%) per skill invocation
- Created 3 corky/email runbooks: review-inbox, draft-reply, enrich-contact
- Added btakita/agent-runbooks submodule at src/agent-runbooks/
- Implemented agent-doc notify subcommand (src/notify.rs, 8 tests passing, uncommitted)
- Saved feedback_distribution_aware_reorg.md to auto memory

### Key Decisions
- Runbooks co-locate with skills (distribution-aware, not centralized)
- Auto memory = guardrails (always-loaded); runbooks = procedures (on-demand)
- Notify format: blockquote with --source and --affects flags
- No contexts/ directory; scoped domains in .agent directory instead

### Transferred to agent-doc-bugs
- Review, commit, and release agent-doc with notify subcommand
<!-- agent:boundary:0fb3f3c3 -->
<!-- /agent:exchange -->a

## Pending / Not Built

<!-- agent:pending patch=replace -->
All runbooks session items complete. Remaining work transferred to agent-doc-bugs.
<!-- /agent:pending -->
