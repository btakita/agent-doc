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
### Re: slash command parsing from diff

Yes — implemented in this session (now in the compacted archive). `parse_slash_commands(diff)` in `diff.rs` scans user-added lines for slash commands. Guards exclude code fences, blockquotes, context lines, and removed lines.

Preflight returns `slash_commands: [...]`. SKILL.md step 0b dispatches each via the `Skill` tool before generating a response. Built-ins like `/clear` that can't route through `Skill` are logged and skipped.

The `compact exchange` trigger you just used was processed via this path.
release
### Re: release — v0.31.18 (HEAD)

Released.

| Target | Status |
|--------|--------|
| crates.io | ✓ `agent-doc v0.31.18` |
| PyPI | ✓ `agent_doc-0.31.18-py3-none-manylinux_2_43_x86_64.whl` (required `--auditwheel skip` — glibc 2.43 not yet in auditwheel policy) |
| GitHub Release | ✓ `agent-doc-v0.31.18-linux-x86_64` binary attached |
| `~/.cargo/bin/agent-doc` | ✓ v0.31.18 installed |

**Changes in this release:**
- Partial compact `--keep N`
- Slash command dispatch from diff
- Dedupe stale patch cleanup
- JB plugin startup dedup guard (snapshot-timestamp comparison, v0.2.48)
- Cross-session pane swap fix
- PromptPoller FFI CRDT merge

**Note on PyPI:** The `--compatibility manylinux_2_43 --auditwheel skip` flags were needed because glibc 2.43 (CachyOS) isn't in auditwheel's policy table yet. Previous versions (0.31.17) also missed PyPI for this reason — latest on PyPI was 0.31.16.
<!-- agent:boundary:3c19c5a0 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Install JB plugin v0.2.48 (snapshot-timestamp dedup guard)
<!-- /agent:pending -->
