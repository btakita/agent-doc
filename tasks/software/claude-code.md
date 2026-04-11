---
agent_doc_session: 8c07766e-51f7-416e-93cc-7e102087d3da
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ How can someone look in the claude code history to find the exact session that contained a topic?
❯ How can someone look in the claude code history to find the exact session that contained a topic?

### Re: Finding a session by topic (HEAD)

Sessions are stored as JSONL files in `~/.claude/projects/<encoded-project-path>/`.

**Direct grep:**
```bash
grep -rl "your topic" ~/.claude/projects/-home-brian-work-btakita-agent-loop/*.jsonl
# Returns file paths — filename is the session UUID
```

**Inspect a session:**
```bash
cat ~/.claude/projects/<project>/<session-uuid>.jsonl \
  | jq -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text' 2>/dev/null \
  | head -100
```

**Path encoding:** `/home/brian/work/btakita/agent-loop` → `-home-brian-work-btakita-agent-loop` (slashes become hyphens, leading `-` added).

**Semantic search:** Use `/mem-search` (claude-mem skill) for cross-session topic search — better than exact string matching when you don't know the exact phrasing.
<!-- agent:boundary:2e9d577e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
