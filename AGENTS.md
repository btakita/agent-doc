# agent-loop

Workspace for AI agent tooling and correspondence.

## Layout

All software project repos live under `src/`. Root contains only agent-loop operational files.

```
agent-loop/
├── src/           # All software project submodules (17+)
├── mail/          # Email data (private, submodule)
├── philosophy/    # Ontology context (submodule)
├── resume/        # Content (submodule)
├── tasks/         # Session documents (not a submodule)
├── bin/           # Build output
└── tmp/           # Scratch (gitignored)
```

**Key submodules in `src/`:**
- **src/corky/** — Correspondence kit. See `src/corky/CLAUDE.md`.
- **src/agent-doc/** — Interactive document sessions. See `src/agent-doc/CLAUDE.md`.
- **src/instruction-files/** — Shared audit crate.
- **src/presence/** — Presence-AI monorepo.
- **src/cloudflare/** — Cloudflare infra monorepo.
- **src/existence-lang/** — Existence language (specs + source).

**Git rules for submodules:**
- Commit submodule changes FROM WITHIN the submodule directory, not the parent.
- After pushing submodule changes, update the parent's submodule pointer: `git add <submodule> && git commit`.
- Never stage files across repo boundaries in a single commit.

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
- **Follow all runbooks and checklists rigorously.** "Approval" signals like "do", "go", "yes" mean "execute with full quality processes" — NOT "move fast and skip steps." Every runbook step applies regardless of how the task was initiated.

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
- **No Confident Hallucination:** If you don't know, say "I don't know." If you speculate, label it as speculation. Never present unverified claims as facts.

## Security — Secret Handling

**NEVER inline cleartext secrets in commands.** This user livestreams — tool call output is visible to viewers.

- Use `pass <path>` via subshell: `CARGO_REGISTRY_TOKEN="$(passage crates.io/token)" cargo publish`
- Use `--token "$(passage ...)"` when CLIs accept token flags
- If a command requires a secret, always wrap it with `pass` or reference an env var that's set in the shell profile
- Never echo, print, or log secrets — not even in error messages
- If you're unsure whether a value is secret, treat it as secret
- **Docker mounts:** Never mount the entire `agent-loop/` directory into a Docker container — it contains sensitive files (`mail/`, credentials). Mount only the specific subdirectories needed (e.g., `src/agent-doc` + `src/tmux-router` for manylinux builds).

## Runbooks

On-demand procedures in `.agent/runbooks/`. Follow these when performing the corresponding task:

- **Precommit**: `.agent/runbooks/precommit.md`
- **Prerelease**: `.agent/runbooks/prerelease.md`
- **LinkedIn posts**: `.agent/runbooks/linkedin-post.md`

## Conventions

- Each submodule has its own CLAUDE.md with project-specific instructions.
- This root AGENTS.md covers cross-cutting concerns only.
- `mail/` is private data — never commit real names or email content to public repos.
- Use `yyyymmdd/short-description` for branch names.
- Use `make check` (clippy + test) before committing in Rust projects.
- Use `make precommit` when available (includes audit-docs).
