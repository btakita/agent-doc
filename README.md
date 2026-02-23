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
