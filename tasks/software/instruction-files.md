---
agent_doc_session: 3b9e6000-115c-4beb-a64d-0300306c97f0
agent_doc_format: template
agent_doc_write: crdt
---


## Status

<!-- agent:status patch=replace -->
Ecosystem complete. Discussing skill installation design for dependent libraries.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary: instruction-files ecosystem build

**Scope:** Expanded instruction-files from a narrow AGENTS.md/CLAUDE.md auditor into a comprehensive ecosystem of 6 Rust crates for managing AI agent instruction files.

#### Repos Created
- **btakita/agent-rules** — Convention for prescribed policy (CLAUDE.md, .cursorrules, AGENTS.md). CLI: `validate`, `lint`. 37 tests.
- **btakita/agent-runbooks** — Convention for on-demand procedures. CLI: `audit`, `list`, `validate`, `install`. Bundled precommit/prerelease defaults. 30 tests.
- **btakita/agent-memories** — Convention for committed memories (learned lessons). CLI: `validate`, `list`, `write`. Memory types: user/feedback/project/reference. 15 tests.
- **btakita/skill-harness** — Lifecycle management for AI agent skills (was agent-skills, renamed to avoid conflict with official Agent Skills Standard at agentskills.io). CLI: `install`, `check`, `uninstall`, `list`. Self-skill for meta-management. 9 tests.

#### Restructured
- **agent-kit** v0.4.0 — Slimmed to core: Environment detection (Claude Code, OpenCode, Codex, Cursor, Generic), shared audit primitives (context_invariant, line_budget, staleness, discovery), hooks (HookRegistry, transports). skill.rs moved to skill-harness. 53 tests.
- **instruction-files** v0.2.0 — Now a thin orchestrator CLI (`audit`, `init`, `list`) that calls into domain crates. All validation logic redistributed. Optional ontology validation (`[term:Name]` annotations) and spec audit (SPEC.md structure). 33 tests.

#### Architecture
```
agent-kit (detect + audit_common + hooks)
    ^
    |--- agent-rules (rules validation)
    |--- agent-runbooks (runbook validation + init)
    |--- agent-memories (memory validation + write)
    |--- skill-harness (skill lifecycle)
    ^
instruction-files (orchestrator CLI)
```

#### Key Decisions
- **Harness pattern** (Spec/Agentic Contracts/Evals) applied to all repos — consistent markdown convention
- **agent-skills renamed to skill-harness** — official Agent Skills Standard (agentskills.io) owns the `agent-skills` namespace
- **Hooks stay in agent-kit** — they're session coordination infrastructure, not a content type
- **Runbooks and memories use `.agent/` universally** — tool-agnostic. Skills and rules map to tool-specific directories.
- **instruction-files is an orchestrator, not the engine** — domain crates own validation, instruction-files coordinates

#### Published to crates.io
All 6 crates published. CI + Release GitHub Actions on all repos. CARGO_REGISTRY_TOKEN secrets configured.

#### Open Items
None from this session. Future work: deeper ontology/spec integration, cross-tool portability testing, memory write integration with Claude Code auto-memory.

How does instruction-files support dependent libraries installing it's skills?
### Re: How does instruction-files support dependent libraries installing skills? (HEAD)

Currently it doesn't — there's a gap. Here's the current state and the design space:

**What exists today:**
- `skill-harness install` installs a single SKILL.md to the correct environment path
- `agent-doc skill install` and `corky skill install` bundle their own SKILL.md via `include_str!` and use skill-harness (formerly agent-kit) to install
- `instruction-files init` scaffolds `.agent/runbooks/` with bundled defaults

**The gap:** When a library like `agent-doc` depends on `instruction-files`, there's no mechanism for instruction-files to say "I come with these skills, install them."

**Design options:**

#### Option A: Bundled skills via `include_str!` (current pattern)
Each crate bundles its own SKILL.md and provides an `install` command:
```bash
agent-doc skill install      # installs agent-doc skill
corky skill install           # installs corky skill
skill-harness install email --file ./skill/SKILL.md
```
**Pro:** Simple, explicit. **Con:** No transitive installation.

#### Option B: Skill manifest in Cargo.toml metadata
```toml
[package.metadata.skills]
instruction-files-audit = { path = ".claude/skills/instruction-files/SKILL.md" }
```
A `skill-harness install-deps` command walks the dependency tree and installs skills from all deps.
**Pro:** Automatic. **Con:** Cargo metadata is stringly-typed, no standard.

#### Option C: Post-install hook via `init()`
Each crate provides an `init()` function that installs its skills:
```rust
// In instruction-files lib.rs
pub fn init(root: &Path) -> Result<()> {
    init_runbooks(root)?;
    install_skill(root, include_str!("../skill/SKILL.md"))?;
    Ok(())
}
```
Consumers call `instruction_files::init(root)` during their own init.
**Pro:** Composable, Rust-native. **Con:** Manual wiring.

#### Option D: `.agent/skills.toml` manifest
A project-level manifest lists skills to install:
```toml
[skills]
instruction-files = { source = "crate", version = "0.2" }
agent-doc = { source = "crate", version = "0.31" }
email = { source = "local", path = ".agent/skills/email" }
```
`skill-harness sync` reads this and installs/updates all listed skills.
**Pro:** Declarative, version-controlled. **Con:** New file format to maintain.

**My recommendation:** Start with **Option C** (init function) for Rust crate consumers, and **Option D** (skills.toml manifest) for project-level management. Option C is what agent-doc already does. Option D is what's needed for cross-tool portability.

Want me to implement one of these?
<!-- agent:boundary:abf8bac0 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Design skill installation for dependent libraries
- [ ] Implement init() pattern or skills.toml manifest
<!-- /agent:pending -->
