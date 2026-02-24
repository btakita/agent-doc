# agent-loop

Workspace for AI agent tooling and correspondence.

## Submodules

- **corky/** — Correspondence kit (email sync, drafts, contacts). See `corky/CLAUDE.md`.
- **agent-doc/** — Interactive document sessions with AI agents. See `agent-doc/CLAUDE.md`.

## Workflow Orchestration

### 1. Plan Mode Default
- Enter plan mode for any non-trivial task (3+ steps or architectural decisions).
- If something goes sideways, STOP and re-plan immediately — don't keep pushing.
- Use plan mode for verification steps, not just building.
- Write detailed specs upfront to reduce ambiguity.

### 2. Subagent Strategy
- Use subagents liberally to keep main context window clean.
- Offload research, exploration, and parallel analysis to subagents.
- For complex problems, throw more compute at it via subagents.
- One task per subagent for focused execution.

### 3. Self-Improvement Loop
- After any correction from the user: update auto memory with the pattern.
- Write rules for yourself that prevent the same mistake.
- Ruthlessly iterate on these lessons until mistake rate drops.
- Review lessons at session start for relevant project.

### 4. Verification Before Done
- Never mark a task complete without proving it works.
- Diff behavior between main and your changes when relevant.
- Ask yourself: "Would a staff engineer approve this?"
- Run tests, check logs, demonstrate correctness.

### 5. Demand Elegance (Balanced)
- For non-trivial changes: pause and ask "is there a more elegant way?"
- If a fix feels hacky: "Knowing everything I know now, implement the elegant solution."
- Skip this for simple, obvious fixes — don't over-engineer.
- Challenge your own work before presenting it.

### 6. Autonomous Bug Fixing
- When given a bug report: just fix it. Don't ask for hand-holding.
- Point at logs, errors, failing tests — then resolve them.
- Zero context switching required from the user.
- Go fix failing CI tests without being told how.

## Task Management

1. **Plan First:** Write plan to `plan.md` with checkable items.
2. **Verify Plan:** Check in before starting implementation.
3. **Track Progress:** Mark items complete as you go.
4. **Explain Changes:** High-level summary at each step.
5. **Document Results:** Add review section to `plan.md`.
6. **Capture Lessons:** Update auto memory after corrections.

## Core Principles

- **Simplicity First:** Make every change as simple as possible. Minimal code impact.
- **No Laziness:** Find root causes. No temporary fixes. Senior developer standards.
- **Minimal Impact:** Changes should only touch what's necessary. Avoid introducing bugs.

## Conventions

- Each submodule has its own CLAUDE.md with project-specific instructions.
- This root AGENTS.md covers cross-cutting concerns only.
- `mail/` is private data — never commit real names or email content to public repos.
- Use `yyyymmdd/short-description` for branch names.
- Use `make check` (clippy + test) before committing in Rust projects.
- Use `make precommit` when available (includes audit-docs).
