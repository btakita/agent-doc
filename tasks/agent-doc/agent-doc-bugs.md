---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.31 — JB plugin v0.2.56 fix pending install
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
v0.31.18–v0.31.31 bug fixes + IDE comparison + model tier design (extracted to agent-doc-model.md) + IPC post-write commit no-op fix (JB plugin v0.2.56). Open: plugin install pending, neovim integration decision pending.

❯ In src/session-share/tasks/claudescore.md, agent-doc was run but the results were not written back into claudescore.md. Why?
### Re: claudescore.md write failure (HEAD)

**Root cause:** `src/session-share` submodule isn't initialized. The directory is missing, so `agent-doc write` can't create the snapshot or sync the file back.

**Fix:**
```bash
git submodule update --init src/session-share
/agent-doc src/session-share/tasks/claudescore.md
```
<!-- agent:boundary:9949dc19 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Initialize session-share submodule and re-run agent-doc on claudescore.md
<!-- /agent:pending -->
