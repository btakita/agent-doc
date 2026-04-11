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
### Re: claudescore.md write failure

**Root cause:** `src/session-share` submodule isn't initialized. The directory is missing, so `agent-doc write` can't create the snapshot or sync the file back.

**Fix:**
```bash
git submodule update --init src/session-share
/agent-doc src/session-share/tasks/claudescore.md
```

❯ Please evaluate why the response patch did not occur to /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore.md
### Re: claudescore.md write failure (analysis)

**Root cause:** The `src/session-share` submodule doesn't have a `.agent-doc/` directory initialized.

When `agent-doc write` runs on a file in a submodule:
1. It looks for `.agent-doc/snapshots/` to store the response snapshot
2. It looks for `.agent-doc/patches/` to queue IPC patches
3. If these directories don't exist, the write fails (IPC timeout or error)

The submodule has `.agent/` (agent-doc operational files) but NOT `.agent-doc/` (which agent-doc itself creates and manages).

**Solution:**
```bash
agent-doc init src/session-share
/agent-doc src/session-share/tasks/claudescore.md
```

The first command initializes the `.agent-doc/` directory and config in the submodule. Then agent-doc writes will work.

❯ .agent-doc/ is in agent-loop. Does the claudescore.md snapshot live there?
### Re: Snapshot location for submodule files (HEAD)

**Snapshot location:** Yes, claudescore.md's snapshot will live in `agent-loop/.agent-doc/snapshots/` (computed from the **canonical absolute path** `/home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore.md`).

**Why submodules need `.agent-doc/` initialized:**

1. **Snapshots** → parent repo's `.agent-doc/snapshots/` (hash-based lookup, always centralized)
2. **IPC patches** → **submodule's** `.agent-doc/patches/` (writes the JSON that plugin consumes)
3. **Baselines** → parent repo's `.agent-doc/baselines/` (temp snapshots for diff computation)

When `src/session-share` lacks `.agent-doc/`, the write fails at step 2: no directory to queue the IPC patch. The plugin can't read it → IPC timeout.

**Architecture:** Snapshots centralize (parent), patches localize (submodule). This allows independent submodule sessions while keeping a unified snapshot history in the parent.
<!-- agent:boundary:9f206937 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Run `agent-doc init src/session-share` to create `.agent-doc/patches/` in submodule
- Then `/agent-doc src/session-share/tasks/claudescore.md` will succeed (snapshot writes to parent, patches queue locally)
<!-- /agent:pending -->
