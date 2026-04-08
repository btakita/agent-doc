---
description: General development and release orchestration for agent-loop submodules
user-invocable: true
argument-hint: "<submodule> <command> [args]"
---

# dev

Development and release orchestration for agent-loop workspace submodules. Handles check, release, status, and changelog across Rust, Python, and JS/TS projects.

## Invocation

```
/dev <submodule> check        — run checks (clippy, test, audit-docs)
/dev <submodule> release      — orchestrate a full release
/dev <submodule> status       — show version, dirty state, unpushed commits
/dev status                   — show status across all submodules
/dev <submodule> changelog    — generate changelog since last tag
```

## Commands

### `check` — Pre-commit Verification

Run the full verification suite for a submodule before committing or releasing.

**Steps:**
1. `cd src/<submodule>`
2. Run `make check` if Makefile exists, otherwise `cargo clippy && cargo test` (Rust) or `bun test` (JS/TS)
3. Run `make precommit` if available (includes audit-docs)
4. Report pass/fail for each step

### `release` — Full Release Orchestration

Orchestrate a complete release for a submodule. This is the primary workflow — it reads the submodule's CLAUDE.md for project-specific instructions and follows them.

**Pre-release checklist:** See `runbooks/pre-release-checklist.md`

**Release workflow by project type:**
- Rust + Maturin (agent-doc, corky): See `runbooks/release-rust-maturin.md`
- Rust Library (instruction-files, tmux-router, agent-kit, tagpath, lazily-rs): See `runbooks/release-rust-library.md`
- Python (lazily-py): See `runbooks/release-python.md`
- JS/TS Apps (briantakita.me-dev, brookebrodack-dev, presence): See `runbooks/release-js-deploy.md`

**Secret handling (CRITICAL):** NEVER inline tokens. Always use `pass` subshell. Common paths: `pass btak/CARGO_REGISTRY_TOKEN` (crates.io), `pass pypi/publish_token` (PyPI).

**Unpublished tag recovery:** See `runbooks/unpublished-tag-recovery.md`

**Cargo path dependencies:** See `runbooks/cargo-path-deps.md` — use `.cargo/config.toml` patch for local dev, crate versions in `Cargo.toml`.

### `status` — Workspace Status

Show the state of one or all submodules.

**Per-submodule output:**
```
<submodule>  v<version>  [clean|dirty]  [pushed|+N unpushed]  [tag matches|tag drift: v<tag> != v<cargo>]
```

**When no submodule specified:** Show status for all submodules under `src/`.

### `changelog` — Generate Changelog

Generate a changelog entry from git history since the last tag.

**Steps:**
1. Find the latest tag: `git describe --tags --abbrev=0`
2. Get commits since: `git log <tag>..HEAD --oneline`
3. Group by type (feat, fix, refactor, docs, chore) based on commit message prefixes
4. Draft a VERSIONS.md entry in the project's existing format
5. Present for review — do not auto-write

## Composability — Configuration Hierarchy

The dev skill uses a layered configuration model, mirroring how Claude Code composes `CLAUDE.md` files across directories. Deeper layers override shallower ones.

### Resolution Chain

```
SPEC.md (invariant — behavioral contract, never overridden)
  ↓
.claude/skills/dev/SKILL.md (workspace defaults — this file)
  ↓
src/<project>/DEV.md (project overrides — checked in)
  ↓
src/<project>/DEV.local.md (private overrides — gitignored)
```

### What Each Layer Controls

| Layer | Scope | Examples |
|-------|-------|---------|
| **SPEC.md** | Invariant behavior | Command contracts, checklist structure, meta-cognition protocol |
| **SKILL.md** | Workspace defaults | Project type tables, dependency graph, secret paths, default checks |
| **DEV.md** | Per-project overrides | Custom release steps, extra checks, publication targets, pre/post hooks |
| **DEV.local.md** | Private per-project | Secret path overrides, skip checks for local dev, private registry URLs |

### DEV.md Format

Per-project `DEV.md` files use YAML frontmatter for structured overrides:

```yaml
---
project_type: rust-maturin          # override auto-detection
publication_targets:
  - crates.io
  - pypi
  - github-release
extra_checks:
  - name: "WASM build"
    command: "wasm-pack build --target web"
skip_checks:
  - audit-docs                       # project doesn't use audit-docs yet
secret_paths:
  crates_io: "btak/CARGO_REGISTRY_TOKEN"
  pypi: "pypi/token"
pre_release:
  - "make clean && make build"       # custom pre-release step
post_release:
  - "make install"                   # custom post-release step
---

# Project-specific release notes

Any markdown content below the frontmatter is treated as human-readable
context for the agent — conventions, gotchas, historical decisions.
```

### Merge Semantics

| Field | Merge Strategy |
|-------|---------------|
| `project_type` | Last wins (deepest layer) |
| `publication_targets` | Replace (deepest layer's list wins) |
| `extra_checks` | Append (accumulate from all layers) |
| `skip_checks` | Append (accumulate from all layers) |
| `secret_paths` | Deep merge (per-target, deepest wins) |
| `pre_release` / `post_release` | Append (run in order: SKILL.md → DEV.md → DEV.local.md) |

### Reading the Chain

Before any `/dev` operation on a project, the agent MUST:
1. Read this SKILL.md (workspace defaults)
2. Check for `src/<project>/DEV.md` — read if present
3. Check for `src/<project>/DEV.local.md` — read if present
4. Also read `src/<project>/CLAUDE.md` for any release-related instructions
5. Merge configurations using the semantics above
6. The merged config drives the operation

### Fallback Behavior

If no `DEV.md` exists for a project, the agent falls back to:
1. Auto-detect project type from marker files (§4 in SPEC.md)
2. Use SKILL.md defaults for that project type
3. Read CLAUDE.md for any project-specific overrides (legacy path)

This ensures backward compatibility — existing projects work without adding DEV.md files.

## Meta-Cognition — Self-Update

The `/dev` skill should evolve as the workspace evolves. After any operation that reveals new patterns, update this SKILL.md:

**When to self-update:**
- A release fails due to a missing step → add the step to the relevant workflow
- A new submodule is added → update project type tables and dependency ordering
- A new publication target is introduced (e.g., npm, Docker) → add a workflow section
- A pre-release check catches something new → add it to the checklist
- A secret path changes or a new one is discovered → update the secret handling section
- The user corrects the skill's behavior → encode the correction as a rule

**What to update:**
- Pre-release checklist (new checks, changed commands)
- Project type classification (new submodules, changed build systems)
- Dependency ordering (new inter-submodule dependencies)
- Secret paths (new tokens, changed `pass` paths)
- Verification commands (new post-publish checks)
- Conventions (new patterns learned from user feedback)

**How to update:**
1. After completing a `/dev` operation, reflect: "Did anything unexpected happen? Did I discover something not in SKILL.md?"
2. If yes, edit this file directly with the new knowledge
3. Keep updates minimal — add the fact, not a narrative

**Architecture-documentation drift detection:**
During any `/dev check` or `/dev release`, compare the submodule's actual structure against its documentation:
- Does CLAUDE.md reflect current build commands?
- Does SPEC.md match the implemented architecture?
- Does README.md describe current features/API?
- Flag any drift and offer to fix it before proceeding

## Conventions

- Always `cd` into the submodule directory before running commands
- Read the submodule's `CLAUDE.md` for project-specific overrides before any operation
- Commit submodule changes from within the submodule, never from the parent
- After any submodule push, update the parent's submodule pointer
- Use `make check` as the canonical verification command when a Makefile exists
- Version bumps are manual — never auto-bump without explicit user request

## Runbooks

When executing these operations, read and follow the linked runbook:

- `pre-release checklist` — [runbooks/pre-release-checklist.md](runbooks/pre-release-checklist.md)
- `release rust+maturin` — [runbooks/release-rust-maturin.md](runbooks/release-rust-maturin.md)
- `release rust library` — [runbooks/release-rust-library.md](runbooks/release-rust-library.md)
- `release python` — [runbooks/release-python.md](runbooks/release-python.md)
- `release js/ts` — [runbooks/release-js-deploy.md](runbooks/release-js-deploy.md)
- `unpublished tag recovery` — [runbooks/unpublished-tag-recovery.md](runbooks/unpublished-tag-recovery.md)
- `submodule commit` — [runbooks/submodule-commit.md](runbooks/submodule-commit.md)
