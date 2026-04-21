# Components

Components are bounded, named, re-renderable regions in a document — similar to web components or React components. They provide a way for agents and scripts to update specific sections of a document without touching the rest.

## Syntax

Components use paired HTML comment markers:

```markdown
<!-- agent:status -->
| Field | Value |
|-------|-------|
| build | passing |
<!-- /agent:status -->
```

The opening marker `<!-- agent:NAME -->` and closing marker `<!-- /agent:NAME -->` define the component boundary. Everything between the markers is the component's content.

### Why paired markers?

A single marker (`<!-- agent:x -->`) has no boundary — after the first render inserts content, the next render can't distinguish the marker from rendered data. Paired markers create an unambiguous boundary. The closing marker makes re-rendering idempotent: `agent-doc patch` always knows exactly what to replace.

### Naming rules

Component names must match `[a-zA-Z0-9][a-zA-Z0-9-]*` — start with alphanumeric, followed by alphanumerics or hyphens.

Valid: `status`, `build-log`, `session2`
Invalid: `-start`, `_name`, `with spaces`

### Nesting

Components can nest. Inner components are parsed independently:

```markdown
<!-- agent:dashboard -->
# System Overview

<!-- agent:status -->
All systems operational.
<!-- /agent:status -->

<!-- agent:metrics -->
CPU: 42%
<!-- /agent:metrics -->
<!-- /agent:dashboard -->
```

Patching `status` or `metrics` only affects that inner component. Patching `dashboard` replaces everything between its markers (including the inner components).

## Patching components

The `agent-doc patch` command replaces a component's content:

```bash
# Replace from argument
agent-doc patch dashboard.md status "build: failing"

# Replace from stdin
echo "build: passing" | agent-doc patch dashboard.md status

# Replace from a script
curl -s https://api.example.com/status | agent-doc patch dashboard.md status
```

The markers are preserved — only the content between them changes.

## Component configuration

Configure component behavior in `.agent-doc/components.toml` at the project root:

```toml
[log]
mode = "append"
timestamp = true
max_entries = 100

[status]
mode = "replace"       # default

[metrics]
pre_patch = "scripts/validate-metrics.sh"
post_patch = "scripts/notify-update.sh"
```

### Modes

| Mode | Behavior |
|------|----------|
| `replace` | Full content replacement (default) |
| `append` | New content added at the bottom of existing content |
| `prepend` | New content added at the top of existing content |

### Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `mode` | string | `"replace"` | Patch mode |
| `timestamp` | bool | `false` | Auto-prefix entries with ISO timestamp |
| `max_entries` | int | `0` | Auto-trim old entries in append/prepend modes (0 = unlimited) |
| `pre_patch` | string | none | Shell command to transform content before patching |
| `post_patch` | string | none | Shell command to run after patching (fire-and-forget) |

## Shell hooks

Hooks let you transform content or trigger side effects when a component is patched.

### pre_patch

Runs before the content is written. Receives the new content on stdin, outputs transformed content on stdout:

```toml
[status]
pre_patch = "scripts/validate-status.sh"
```

```bash
#!/bin/bash
# scripts/validate-status.sh
# Transform content before it's written to the component

# Read incoming content from stdin
content=$(cat)

# Validate or transform
if echo "$content" | jq . > /dev/null 2>&1; then
    # Valid JSON — format it as a markdown table
    echo "$content" | jq -r 'to_entries[] | "| \(.key) | \(.value) |"'
else
    # Pass through unchanged
    echo "$content"
fi
```

Environment variables available:
- `COMPONENT` — component name (e.g., `status`)
- `FILE` — path to the document being patched

If the hook exits non-zero, the patch is aborted.

### post_patch

Runs after the content is written. Fire-and-forget — output is inherited (prints to terminal), exit code is logged but doesn't affect the patch:

```toml
[metrics]
post_patch = "scripts/notify-update.sh"
```

```bash
#!/bin/bash
# scripts/notify-update.sh
echo "Component '$COMPONENT' updated in $FILE"
# Could trigger a webhook, send a notification, etc.
```

## Components vs comments

Regular HTML comments are a user scratchpad — they're stripped during diff comparison and never trigger agent responses:

```markdown
<!-- This is a regular comment — invisible to the agent -->
```

Component markers **look like** comments but are structural:

```markdown
<!-- agent:status -->
This content is managed by the agent or scripts
<!-- /agent:status -->
```

The diff engine preserves component markers while stripping regular comments. This means:
- Adding/removing regular comments does **not** trigger a response
- Changing content inside a component **does** trigger a response
- The markers themselves are never modified by `patch`
