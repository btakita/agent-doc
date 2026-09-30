---
description: "Interactive markdown session. TRIGGER: user invokes the harness-native agent-doc entrypoint. Requires a markdown session document, installed CLI, and write+commit every cycle."
user-invocable: true
argument-hint: "<file>"
agent-doc-version: "0.34.0"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Invocation

- Claude Code: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`
- Codex: `agent-doc <FILE>`, `agent-doc claim <FILE>`, `agent-doc compact <FILE>`, `agent-doc compact exchange <FILE>`
- OpenCode: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

## Dynamic Context Router

Keep this file as a router, not a duplicate manual. Each entry should point to
the dynamic file that owns its details:

- **Context strategy:** See [`runbooks/dynamic-context.md`](runbooks/dynamic-context.md) for the AGENTS/SKILL pattern, dynamic context service shape, and storage guidance.
- **agent-doc workflow:** See [`.claude/skills/agent-doc/SKILL.md`](.claude/skills/agent-doc/SKILL.md); load its linked runbooks only when that workflow branch is active.
- **Code navigation:** See [`runbooks/code-navigation.md`](runbooks/code-navigation.md); use `tsift` envelopes instead of copying search, diff, log, or transcript content into prompts.
- **Private name hygiene:** See [`runbooks/private-name-hygiene.md`](runbooks/private-name-hygiene.md) before editing public or shared subrepos from this private workspace.
- **Dogfood watch:** See [`runbooks/dogfood-watch.md`](runbooks/dogfood-watch.md) — `bin/agent-doc-dogfood-watch` turns issues in the live agent-doc sessions into backlog + queue items in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Task-specific procedures:** Use the referenced file under [`runbooks/`](runbooks/) rather than expanding procedures inline.
- **Operator memory (all harnesses):** Curated cross-session memories live in [`memory/`](memory/) (index: [`memory/MEMORY.md`](memory/MEMORY.md)); retrieve via `tsift --envelope search '<query>'` or `tsift graph-db --path . --json related '<query>'`. Session-event memory lives in `.tsift/memory.db` (query via `tsift memory status` / `graph-db related`). Claude Code's memory dir symlinks to `memory/`, so treat `memory/` as the single source of truth; other harnesses read it the same way.

**Full workflow:** See [`.claude/skills/agent-doc/SKILL.md`](.claude/skills/agent-doc/SKILL.md) for the complete agent-doc workflow (preflight, respond, persist, pending ops, auto-loop).

**Code navigation:** See [`.claude/skills/tsift/SKILL.md`](.claude/skills/tsift/SKILL.md) and [`runbooks/code-navigation.md`](runbooks/code-navigation.md) for tsift AST-aware search, call graphs, and digest commands.

**Runbooks:** [runbooks/](runbooks/) — dynamic-context, harness-invocation, commit, planning-dispatch, pending-ops, model-tier-gate, compact-exchange, streaming-checkpoints, document-format, code-enforced-directives, architecture-first (agent-doc development/dogfooding only), and more.

<!-- tsift:code-navigation v=0.1.101 -->
## Code Navigation

Use the repository tsift skill at [`.agents/skills/tsift/SKILL.md`](.agents/skills/tsift/SKILL.md) for code search, source reading, call graphs, diffs, logs, and test output.
<!-- /tsift:code-navigation -->
