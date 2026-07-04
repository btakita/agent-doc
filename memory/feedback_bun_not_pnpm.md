---
name: Use bun for briantakita.me builds
description: Build briantakita.me-dev with bun, not pnpm — pnpm shows workspace warnings and is not the correct package manager
type: feedback
---

Use `bun` instead of `pnpm` when building briantakita.me-dev.

**Why:** pnpm shows workspace field warnings and is not the correct package manager for this project. The project uses bun.

**How to apply:** When building `src/briantakita.me-dev`, use `cd src/briantakita.me-dev && bun run build` or `cd src/briantakita.me-dev/app/briantakita.me && bun run build`. Never use `pnpm build`.
