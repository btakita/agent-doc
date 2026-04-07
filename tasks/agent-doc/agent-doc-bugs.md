---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.16 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Session: agent-doc v0.31.16 release; cross-session pane swap fix; session-drift detection; partial compact (--keep N); dynamic plugin command discovery; Refresh Agent Doc Environment JB action (v0.2.45); CRDT boundary-synthesis duplicate bug; plugin v0.2.46 stale patch cleanup; v0.2.47 version bump + runbook; v0.2.48 snapshot-timestamp dedup guard (correct fix); compact-exchange runbook tightened (standalone imperative only); xbow.md false-positive compaction resolved; claudescore.md content confirmed intact. Open: install plugin v0.2.48.

Does agent-doc have a feature to parse and directly run agent harness commands that begin with a `/`?
### Re: slash command parsing from diff (HEAD)

Yes — implemented in this session (now in the compacted archive). `parse_slash_commands(diff)` in `diff.rs` scans user-added lines for slash commands. Guards exclude code fences, blockquotes, context lines, and removed lines.

Preflight returns `slash_commands: [...]`. SKILL.md step 0b dispatches each via the `Skill` tool before generating a response. Built-ins like `/clear` that can't route through `Skill` are logged and skipped.

The `compact exchange` trigger you just used was processed via this path.
<!-- agent:boundary:e11ff685 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Install plugin v0.2.48 (snapshot-timestamp dedup guard)
<!-- /agent:pending -->
