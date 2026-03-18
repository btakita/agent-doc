---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange -->

### Re: Development & Release Orchestration Skill

Got it — **general development and release orchestration**. Given the GSD review and your existing skill patterns, here's my take:

**Why home-grown makes sense:**
- GSD's 37 commands / 41 workflows / 15 agents is massive overhead for your workspace. You'd be fighting its opinions more than using them.
- Your agent-doc template mode already provides the state management (exchange/pending/status components) that GSD uses `.planning/` for.
- Your submodule-heavy workspace (17+ repos) needs orchestration that understands cross-repo boundaries — GSD doesn't.

**What a `dev` skill could look like:**

```
/dev <submodule> [command]
  check     — run make check (clippy + test), report failures
  release   — version bump → build → publish (crates.io/PyPI/GitHub) → tag → update parent pointer
  status    — show dirty submodules, unpushed commits, version drift
  changelog — generate changelog from git log since last tag
```

**Key design choices:**
1. **Thin orchestrator** (borrowed from GSD) — the skill SKILL.md defines the workflow, Claude does the execution. No separate CLI binary needed initially.
2. **Release checklist as workflow, not config** — each submodule's CLAUDE.md already documents release steps. The skill reads those and follows them.
3. **Cross-repo awareness** — after publishing a submodule, automatically `git add <submodule> && git commit` in the parent.
4. **Password-store integration** — `pass` for crates.io/PyPI tokens, never inline.

**What to borrow from GSD:**
- Verification step after each release target (did crates.io publish succeed? did the GitHub release create?)
- Codebase mapping for unfamiliar submodules
- The "thin orchestrator" principle — keep the skill definition declarative, let the agent handle execution

We need to add a few more verifications:
- audit docs
- Is README.md up to date?
- Is VERSIONS.md up to date?
- Were tests written for the feature or bug fix?

**What to skip from GSD:**
- Multi-runtime support (you're Claude Code only)
- The `.planning/` state directory (agent-doc handles this)
- Their 15 specialized agents (overkill — subagents on demand is cleaner)

**Next steps — want me to:**
1. Draft a `SKILL.md` for `/dev` based on this design?
2. Start with just the `release` subcommand (highest value, most error-prone manually)?
3. Something else?

1 and 2.
<!-- /agent:exchange -->
