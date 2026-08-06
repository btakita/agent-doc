# Tasks Directory

| Field | Value |
|---|---|
| **Name** | Tasks Directory Guide |
| **Source** | tasks/README.md |
| **Version** | 1.0.0 |
| **Last Updated** | 2026-04-26 |
| **Purpose** | Explains what goes in the tasks/ folder, organization, and cleanup prompts |
| **Audience** | Robert Ross |

This folder holds planning docs, session notes, and working artifacts for your projects. It's where you think and plan — `src/` is where the code lives.

## What goes here

| Type | Example | When to create |
|------|---------|----------------|
| **Plans** | `tasks/claudescore/scoring-dashboard-plan.md` | Starting a multi-step project |
| **Session notes** | `tasks/claudescore/api-design-notes.md` | Capturing decisions from a working session |
| **Research** | `tasks/build-party/competitor-analysis.md` | Investigating options before committing |
| **Meeting notes** | `tasks/meetings/2026-04-25-kickoff.md` | Recording action items from a meeting |
| **Checklists** | `tasks/launch-checklist.md` | Tracking one-time setup or release steps |

## What does NOT go here

- Code, configs, or anything that runs — those go in `src/`
- Files Claude generates as part of running a plan (those go in the project under `src/`)
- Permanent documentation (that belongs in the project's own README or docs/)

## Organization

Group by project when the task relates to a specific codebase:

```
tasks/
├── claudescore/
│   ├── scoring-dashboard-plan.md
│   └── api-design-notes.md
├── build-party/
│   ├── launch-plan.md
│   └── competitor-analysis.md
└── meetings/
    └── 2026-04-25-kickoff.md
```

Standalone tasks (not tied to a project) go at the top level of `tasks/`.

## Cleanup

Tasks go stale. Periodically ask Claude to tidy up:

> "Review my tasks/ folder — which plans are done, which are stale, and which are still active?"

Claude will read each file, check completion status, and suggest:
- **Archive** completed plans (move to `tasks/archive/`)
- **Delete** stale research that's no longer relevant
- **Update** plans that have drifted from reality

For a quick purge:

> "Move all completed tasks to tasks/archive/"

> "Delete tasks older than 3 months that I haven't touched"

## Creating your first task

> "Create a plan for [your next project]"

Claude creates the file here and you're off.
