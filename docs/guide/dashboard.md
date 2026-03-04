# Dashboard-as-Document

A dashboard is a markdown document with agent-maintained components that display live data. Instead of a separate dashboard UI, the document **is** the dashboard — editable in any text editor, version-controlled with git, and updated by scripts or agents via `agent-doc patch`.

## Quick start

### 1. Create the dashboard document

```markdown
---
session: null
---

# Project Dashboard

## Status

<!-- agent:status -->
| Service | State |
|---------|-------|
| api     | unknown |
| worker  | unknown |
<!-- /agent:status -->

## Recent Activity

<!-- agent:log -->
<!-- /agent:log -->
```

### 2. Configure components

Create `.agent-doc/components.toml`:

```toml
[status]
mode = "replace"

[log]
mode = "append"
timestamp = true
max_entries = 50
```

### 3. Update components from scripts

```bash
# Update the status table
agent-doc patch dashboard.md status "$(cat <<'EOF'
| Service | State |
|---------|-------|
| api     | healthy |
| worker  | healthy |
EOF
)"

# Append to the log
agent-doc patch dashboard.md log "Deployment completed successfully"
```

The status component gets replaced entirely. The log component appends with a timestamp:

```markdown
<!-- agent:log -->
[2026-03-04T18:30:00Z] Deployment completed successfully
<!-- /agent:log -->
```

### 4. Auto-update with watch

Start the watch daemon to auto-submit when the dashboard changes:

```bash
agent-doc watch
```

Now when external scripts update components via `patch`, the watch daemon detects the file change and can trigger `agent-doc run` to let the agent respond to the new data.

## End-to-end flow

```
External script                agent-doc               Agent
     |                            |                      |
     |-- patch status ----------->|                      |
     |                            |-- (file changed) --->|
     |                            |   (watch detects)    |
     |                            |-- run (diff+send) -->|
     |                            |                      |-- responds
     |                            |<-- patch log --------|
     |                            |   (agent updates)    |
```

1. An external script calls `agent-doc patch` to update a component
2. The watch daemon detects the file change
3. Watch triggers `agent-doc run` which diffs and sends to the agent
4. The agent sees the change ("status went from unknown to healthy") and can respond — updating the log, adding analysis, or patching other components

## Dashboard with multiple components

A real-world dashboard might look like:

```markdown
---
session: null
---

# Build Monitor

<!-- agent:summary -->
**Last updated:** never
<!-- /agent:summary -->

## Build Status

<!-- agent:builds -->
No builds yet.
<!-- /agent:builds -->

## Test Results

<!-- agent:tests -->
No test results.
<!-- /agent:tests -->

## Activity Log

<!-- agent:log -->
<!-- /agent:log -->
```

With `.agent-doc/components.toml`:

```toml
[summary]
mode = "replace"

[builds]
mode = "replace"
post_patch = "scripts/check-failures.sh"

[tests]
mode = "replace"

[log]
mode = "append"
timestamp = true
max_entries = 200
```

Update from CI:

```bash
# After a build completes
agent-doc patch monitor.md builds "$(./scripts/format-builds.sh)"

# After tests run
agent-doc patch monitor.md tests "$(./scripts/format-tests.sh)"

# Log the event
agent-doc patch monitor.md log "Build #${BUILD_ID} completed: ${STATUS}"
```

## User interaction with dashboards

Dashboards are still documents — users can write in them. Add a `## User` block or annotate inline. The agent responds to the diff like any session document.

```markdown
## User

Why did build #42 fail? Can you analyze the test results component?
```

The agent sees the full dashboard (all components) plus the user's question, and can respond in context.

## Loop prevention

When the watch daemon is running, a patch can trigger a run, which might patch again, creating a cycle. Watch prevents unbounded loops:

- **Bounded cycles** (default 3): After 3 consecutive agent-triggered re-submits with no external change, watch pauses that file
- **Convergence detection**: If the agent's response produces the same content as last time (hash match), the cycle stops
- **Configurable**: `agent-doc watch --max-cycles 5 --debounce 1000`

## Tips

- **Reference other files**: Dashboards can reference other documents — use relative paths from the project root
- **Inline annotations**: Edit within component content to ask questions — the diff captures your edits
- **Comments are private**: `<!-- regular comments -->` are never sent to the agent. Use them for notes.
- **Snapshots reset on rename**: Moving a file resets the diff baseline (snapshots are keyed by canonical path)
- **Git integration**: `agent-doc commit dashboard.md` commits the current state with a timestamp
