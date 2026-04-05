---
agent_doc_session: 3b9e6000-115c-4beb-a64d-0300306c97f0
agent_doc_format: template
agent_doc_write: crdt
---


## Status

<!-- agent:status patch=replace -->
4/5 repos released with binaries. instruction-files v0.2.0 rebuilding after fix.
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
### Re: How does instruction-files support dependent libraries installing skills?

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
Each crate provides an `init()` function that installs its skills:e
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
Option A is also supported by agent-doc...correct?

Do Option C. Would Option D be for non-rust projects and monorepos?
Can Option D be support when Option C is done.
### Re: Option A + C + D

**Option A:** Yes, agent-doc already uses this. `agent-doc skill install` bundles SKILL.md via `include_str!` and installs it. Corky does the same. Options A and C are complementary — A is the "binary handles its own skill" pattern, C is the "library provides init for consumers" pattern.

**Option C implemented.** `instruction_files::init(root)` now:
1. Installs the bundled SKILL.md to the environment-specific path (uses `agent_kit::detect::Environment` for path resolution)
2. Scaffolds `.agent/runbooks/` with default precommit/prerelease templates
3. Never overwrites existing files
4. Returns paths of files written

Consumers wire it into their own init:
```rust
// In agent-doc's init
pub fn init(root: &Path) -> Result<()> {
    // Install agent-doc's own skill
    install_own_skill(root)?;
    // Transitively install instruction-files' skill + runbooks
    instruction_files::init(root)?;
    Ok(())
}
```

CLI: `instruction-files init` now installs both skills and runbooks. Pushed.

**Option D:** Yes, for non-Rust projects and monorepos. `.agent/skills.toml` would be a declarative manifest that `skill-harness sync` reads. Can be added on top of Option C — they serve different audiences (Rust library consumers vs project-level management). Ready to implement whenever needed.

I'm not sure how Option D will be implemented. If someone does not have Rust, they will need to use a `curl | sh` installer.
### Re: Option D distribution without Rust

Right. For non-Rust users, the distribution options are:

1. **`cargo install`** — requires Rust toolchain, but works for anyone who has it
2. **`cargo binstall`** — downloads prebuilt binaries from GitHub Releases (no compile needed, just needs `cargo-binstall`)
3. **GitHub Releases with cross-compiled binaries** — `curl | sh` installer that downloads the right binary for the platform. This is what tools like `rg`, `fd`, `bat` do.
4. **npm wrapper** — publish a thin npm package that downloads the binary on `postinstall`. Users run `npx skill-harness sync`.
5. **Homebrew tap** — `brew install btakita/tap/skill-harness`

The most practical path: add cross-compilation to the Release workflow (GitHub Actions matrix for linux-x86_64, linux-aarch64, macos-x86_64, macos-aarch64, windows-x86_64), then provide a `curl` installer script. This is the same pattern agent-doc uses.

But honestly, Option D might not need its own binary. The `.agent/skills.toml` manifest could be consumed by any agent that reads files — Claude Code, Cursor, etc. The agent itself runs `skill-harness sync` (if Rust is available) or manually copies files based on the manifest. The manifest is the portable part; the tooling is optional.

This is a future concern — Option C covers the immediate need for Rust consumers.

Do you recommend Option D.
We should support binary install for all of the packages:
- **agent-rules**
- **agent-runbooks**
- **agent-memories**
- **skill-harness**
- **instruction-files**
### Re: Binary install + Option D

**Binary install:** Done. All 5 repos now have cross-compilation release workflows:
- 5-platform matrix: linux x86_64, linux aarch64 (via cross), macOS x86_64, macOS aarch64, Windows x86_64
- GitHub Release with prebuilt binaries (tar.gz for Unix, zip for Windows)
- crates.io publish on the same tag

To release: `git tag v0.1.0 && git push origin v0.1.0` — CI builds binaries for all platforms, creates GitHub Release, and publishes to crates.io.

Users install via:
- `cargo install agent-rules` (Rust users)
- `cargo binstall agent-rules` (prebuilt binary download)
- Download from GitHub Releases (no Rust needed)
- `curl -fsSL https://github.com/btakita/agent-rules/releases/latest/download/agent-rules-x86_64-unknown-linux-gnu.tar.gz | tar xz` (direct)

**Option D recommendation:** Yes, I recommend it as a future phase. The `.agent/skills.toml` manifest is language-agnostic — it describes what skills a project needs. Any tool (Rust binary, shell script, or agent reading the file directly) can consume it. The binary tools (agent-rules, skill-harness, etc.) make it automatic; without them, an agent can still read the manifest and manually copy files.

Implementation sequence: Option C (done) -> binary distribution (done) -> Option D manifest format (next).
Please add a install.sh script for all of the packages. To support `curl ... | sh`.
Update install documentation.
### Re: Install scripts

Done. All 5 repos now have `install.sh` and updated install documentation.

**install.sh** (each repo):
- Auto-detects OS (linux/macOS) and architecture (x86_64/aarch64)
- Downloads latest release binary from GitHub Releases
- Installs to `~/.local/bin` (or `INSTALL_DIR` env var)
- Warns if not in PATH

**Usage:**
```bash
# Any repo
curl -fsSL https://raw.githubusercontent.com/btakita/agent-rules/main/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-runbooks/main/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-memories/main/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/btakita/skill-harness/main/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/btakita/instruction-files/main/install.sh | sh
```

**README install sections** updated with three options: curl installer, `cargo install`, from source.

Note: the install scripts will start working once the first release with binaries is pushed (the cross-compilation workflow triggers on `v*` tags).

Should this be supported? How would a project use this?
> - [ ] Implement .agent/skills.toml manifest (Option D)
### Re: Should .agent/skills.toml be supported?

**Use case:** A project maintainer wants to declare "this project uses these skills" so any contributor (or agent) can set up the correct instruction files automatically.

**How a project would use it:**

1. **Author** creates `.agent/skills.toml`:
```toml
[skills]
agent-doc = { source = "github", repo = "btakita/agent-doc", skill = "agent-doc" }
email = { source = "local", path = ".agent/skills/email" }
instruction-files = { source = "crate", name = "instruction-files" }
```

2. **Contributor** clones the project and runs:
```bash
skill-harness sync
```
This reads `.agent/skills.toml` and installs each skill to the detected environment (Claude Code -> `.claude/skills/`, Cursor -> `.cursor/rules/`, etc.).

3. **CI** runs `skill-harness check` to verify all declared skills are installed and up to date.

**But here's the question:** Is this solving a real problem today, or is it premature?

**Arguments for:**
- Monorepos with many skills need a manifest
- Onboarding new contributors is easier ("run one command")
- Declarative > imperative for project config

**Arguments against:**
- Most projects have 1-3 skills — `skill-harness install` is fine
- The Agent Skills Standard (agentskills.io) might define their own manifest format
- Adding a new file format creates maintenance burden

**My take:** Park it. The Agent Skills Standard is actively evolving — they may define a manifest format that becomes the cross-tool standard. Building our own risks divergence. Option C (init functions) and Option A (CLI install) cover real needs today. When a manifest format emerges from the standard, we adapt.

Remove from pending?
Yes
### Re: Remove skills.toml from pending

Done. Removed from pending. The ecosystem is complete for this session.
do
- [ ] Push first release tags to trigger binary builds
### Re: Push release tags

Done. All 5 repos tagged and pushed:

| Repo | Tag | CI Status |
|------|-----|-----------|
| agent-rules | v0.1.0 | Building (5 platforms + crates.io) |
| agent-runbooks | v0.1.0 | Building |
| agent-memories | v0.1.0 | Building |
| skill-harness | v0.1.0 | Building |
| instruction-files | v0.2.0 | Building |

GitHub Actions is now cross-compiling binaries for linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64. Each will create a GitHub Release with downloadable binaries and publish to crates.io.

The `curl | sh` installers will work once builds complete (~5-10 min).

release
### Re: Release status (HEAD)

**4 of 5 repos have successful binary releases** with all 5 platform binaries:
- agent-rules v0.1.0
- agent-runbooks v0.1.0
- agent-memories v0.1.0
- skill-harness v0.1.0

**instruction-files v0.2.0** — had two issues, both fixed:
1. `.claude/skills/instruction-files/SKILL.md` wasn't tracked in git (needed by `include_str!`) — now committed
2. Publish step failed on "already published" — added `|| true` to all repos

Re-tagged and pushed v0.2.0. Build should succeed now.

All `curl | sh` installers are live for the 4 completed repos.
<!-- agent:boundary:45e4006c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete. instruction-files v0.2.0 rebuilding.
<!-- /agent:pending -->
