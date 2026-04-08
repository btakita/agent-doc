# agent-loop

Workspace for AI agent tooling and correspondence.

## Projects

| Submodule | Description |
|-----------|-------------|
| `corky/` | Correspondence kit — email sync, drafts, contacts |
| `agent-doc/` | Interactive document sessions with AI agents |

## Setup

```sh
git clone --recurse-submodules git@github.com:btakita/agent-loop.git
cd agent-loop
```

## Structure

```
agent-loop/
  corky/         # email tooling (public)
  agent-doc/     # document sessions (public)
  mail/          # correspondence data (private, symlink or submodule)
```

## Agent Runbooks (`.agent/runbooks/`)

On-demand agent procedures — step-by-step checklists that are read only when triggered, keeping AGENTS.md/CLAUDE.md lean.

```
.agent/runbooks/
├── precommit.md     # Steps before committing
├── prerelease.md    # Steps before publishing a release
└── linkedin-post.md # LinkedIn post workflow
```

**Why not inline in CLAUDE.md?** Instruction files (CLAUDE.md, AGENTS.md) are loaded into context on every conversation — every token counts. Runbooks are read on demand at trigger points (e.g., "before releasing, read `.agent/runbooks/prerelease.md`"), costing zero tokens until needed.

**Cross-harness compatible.** The `.agent/` namespace is vendor-neutral. Any agent tool (Claude Code, Cursor, Codex, OpenCode) can reference `.agent/runbooks/` without adopting another vendor's convention. Each harness just needs a one-liner in its instruction file pointing to the runbook.
