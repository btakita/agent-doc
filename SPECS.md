# agent-loop Workspace Specification

## 1. Overview

agent-loop is a super-project workspace containing AI agent tooling as git submodules.

## 2. Structure

```
agent-loop/
  agent-doc/       # Submodule: interactive document sessions (github.com/btakita/agent-doc)
  corky/           # Submodule: email sync & collaboration (github.com/btakita/corky)
  mail/            # Symlink to private data (~/.../mail), gitignored
  plan.md          # Planning documents, gitignored
  CLAUDE.md        # Cross-cutting agent instructions
  SPECS.md         # This file
```

## 3. Conventions

- Each submodule has its own CLAUDE.md and SPECS.md with project-specific instructions
- Root CLAUDE.md covers cross-cutting concerns only
- `mail/` is private data — never commit real names or email content to public repos
- Submodules are pinned to specific commits; update with `git submodule update`

## 4. Submodule Specs

- **agent-doc**: See `agent-doc/SPECS.md`
- **corky**: See `corky/SPECS.md`
