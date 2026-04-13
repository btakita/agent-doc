# Dev Skill — Functional Specification

> Project-agnostic specification for a development and release orchestration skill.
> This document captures the behavioral contract another project can implement to build their own `/dev` skill.

## 1. Overview

The dev skill orchestrates software development workflows — verification, release, status reporting, and changelog generation — across a workspace of multiple projects (submodules, packages, or services). It is a **thin orchestrator**: a declarative skill definition that an AI agent interprets and executes. No dedicated CLI binary is required.

## 2. Architecture

```
User invocation (/dev <project> <command>)
        ↓
Skill definition (SKILL.md)
        ↓
AI Agent (reads SKILL.md, executes steps)
        ↓
Project toolchain (make, cargo, npm, docker, etc.)
        ↓
Publication targets (registries, releases, deploys)
```

### 2.1 Design Principles

| Principle | Description |
|-----------|-------------|
| **Thin orchestrator** | The skill defines *what* to do; the agent decides *how* |
| **Project-type awareness** | Workflows adapt based on detected project type (Rust, Python, JS/TS, etc.) |
| **Documentation-first** | Each project's own docs (CLAUDE.md, README.md, SPEC.md) are the source of truth for project-specific overrides |
| **Fail-fast verification** | Pre-release checklist blocks the release if any check fails |
| **Secret isolation** | Credentials never appear in commands; always resolved via a secret manager |
| **Meta-cognition** | The skill self-updates when it discovers new patterns or corrections |

### 2.2 Scope

The dev skill operates on a **workspace** containing multiple **projects**. A workspace is a directory with projects nested inside (e.g., `src/` subdirectories, monorepo packages, or independent repos).

## 3. Commands

### 3.1 `check` — Pre-commit Verification

**Input:** Project identifier
**Output:** Pass/fail report per check step

**Behavior:**
1. Navigate to the project directory
2. Detect project type (see §4)
3. Run the canonical check command for the project type
4. Run additional checks if available (audit-docs, lint, etc.)
5. Report pass/fail for each step

### 3.2 `release` — Full Release Orchestration

**Input:** Project identifier
**Output:** Published artifacts + verification report

**Behavior:**
1. Run pre-release verification checklist (see §5)
2. Block if any check fails — report which checks failed
3. Detect project type and select the appropriate release workflow (see §6)
4. Execute publication steps in order
5. Verify each publication target
6. Update workspace references (parent pointers, dependency versions)

### 3.3 `status` — Project/Workspace Status

**Input:** Optional project identifier (omit for all projects)
**Output:** Per-project status line

**Status fields:**
- Project name
- Current version
- Working tree state (clean/dirty)
- Push state (pushed / N unpushed commits)
- Tag alignment (tag matches version / drift detected)

### 3.4 `changelog` — Generate Changelog

**Input:** Project identifier
**Output:** Draft changelog entry (not auto-written)

**Behavior:**
1. Find the latest version tag
2. Collect commits since that tag
3. Categorize by type (feature, fix, refactor, docs, chore)
4. Format as a changelog entry matching the project's existing format
5. Present for human review — never auto-write

## 4. Project Type Detection

The skill must detect what kind of project it's operating on. Detection is based on marker files:

| Marker | Project Type |
|--------|-------------|
| `Cargo.toml` + `pyproject.toml` | Rust + Python bindings (Maturin) |
| `Cargo.toml` (no pyproject.toml) | Pure Rust |
| `pyproject.toml` (no Cargo.toml) | Pure Python |
| `package.json` + `Dockerfile` | JS/TS application (containerized) |
| `package.json` (no Dockerfile) | JS/TS library/tool |
| `Makefile` only | Custom build system |

When multiple markers exist, prefer the most specific match. A project's CLAUDE.md can override detected type behavior.

## 5. Pre-Release Verification Checklist

Every release MUST pass ALL checks before proceeding. The skill blocks the release and reports failures.

| # | Check | Description |
|---|-------|-------------|
| 1 | **Tests pass** | Run the project's canonical test command |
| 2 | **Audit docs pass** | Run documentation auditing if available |
| 3 | **README.md is current** | Review against current features/API; flag outdated sections |
| 4 | **VERSIONS.md is current** | Verify the new version has an entry; draft one if missing |
| 5 | **SPEC.md is current** | If present, verify it reflects current architecture; flag drift |
| 6 | **Test coverage** | Bug fixes: regression test exists. Features: basic coverage exists |
| 7 | **No architecture-docs drift** | Cross-check all documentation against actual code structure |
| 8 | **Version bump done** | All version-bearing files have the new version and are in sync |
| 9 | **Clean working tree** | No uncommitted changes |

### 5.1 Architecture-Documentation Drift Detection

During `check` or `release`, compare project documentation against implementation:
- Does the build/dev instructions doc reflect current commands?
- Does the spec match the implemented architecture?
- Does the README describe current features and API?

Flag mismatches and offer to fix before proceeding.

## 6. Release Workflows

Release workflows are parameterized by project type. Each workflow defines:
- **Publication targets** — where artifacts are published
- **Steps** — ordered list of actions
- **Verification** — how to confirm each target succeeded

### 6.1 Generic Release Workflow

```
1. Pre-release checklist (§5)
2. Version file sync verification
3. Commit (from within project directory)
4. Tag (semantic version)
5. Push (code + tag)
6. For each publication target:
   a. Publish
   b. Verify publication
7. Rebuild local artifacts (if applicable)
8. Update workspace references
```

### 6.2 Publication Targets

| Target | Publish Command | Verify Command |
|--------|----------------|----------------|
| Crates.io | `cargo publish` | `cargo search <crate>` |
| PyPI | `maturin publish` / `twine upload` | `pip index versions <pkg>` |
| npm | `npm publish` | `npm view <pkg> version` |
| GitHub Release | `gh release create` | `gh release view` |
| Docker registry | `docker push` | `docker manifest inspect` |
| VPS deploy | Project-specific deploy command | Health check endpoint |

### 6.3 Dependency Ordering

When projects depend on each other, releases must follow dependency order. The skill must:
1. Know the dependency graph (configured in SKILL.md or derived from manifests)
2. Publish dependencies before dependents
3. After publishing a dependency, check if dependents need version bumps

## 7. Secret Handling

**Critical requirement:** Credentials MUST NEVER appear as plaintext in commands, logs, or output.

### 7.1 Secret Resolution

Secrets are resolved through a secret manager (e.g., `pass`, `1password-cli`, `vault`). The skill:
1. Wraps publish commands with secret manager subshells: `TOKEN="$(passage path/to/token)" cmd`
2. Maintains a registry of known secret paths per publication target
3. If a secret path is unknown, searches or asks the user — never guesses

### 7.2 Secret Path Registry

The SKILL.md maintains a mapping of publication targets to secret paths:
```
crates.io → pass path/to/cargo-token
PyPI → pass path/to/pypi-token
npm → pass path/to/npm-token
```

This registry is updated via meta-cognition when new paths are discovered.

## 8. Meta-Cognition

The skill self-updates after operations that reveal new information.

### 8.1 Triggers

| Trigger | Action |
|---------|--------|
| Release fails due to missing step | Add step to workflow |
| New project added to workspace | Update project type tables |
| New publication target introduced | Add workflow section |
| Pre-release check catches new issue | Add to checklist |
| Secret path changes | Update secret registry |
| User corrects behavior | Encode correction as rule |

### 8.2 Update Protocol

1. After each operation, reflect: "Did anything unexpected happen?"
2. If yes, edit SKILL.md with the new knowledge
3. Keep updates minimal — facts, not narratives
4. Architecture drift detection runs on every `check` and `release`

## 9. Workspace Integration

### 9.1 Cross-Project Updates

After publishing a project, the skill must update workspace-level references:
- Submodule pointers (git)
- Dependency version pins in consuming projects
- Lock files if applicable

### 9.2 Conventions

- Always operate from within the project directory
- Read project-level documentation before any operation
- Commit project changes from within the project, not the workspace root
- Version bumps are explicit — never auto-bump without user request
- Present changelog entries for review — never auto-write

## 10. Composability — Configuration Hierarchy

The dev skill uses a layered configuration model, analogous to how Claude Code composes `CLAUDE.md` files across directory levels. Each layer can override or extend the one above it.

### 10.1 Resolution Chain

```
SPEC.md (invariant — this file, never overridden)
  ↓
SKILL.md (workspace defaults — workspace-specific config)
  ↓
<project>/DEV.md (project overrides — checked into version control)
  ↓
<project>/DEV.local.md (private overrides — gitignored)
```

Additionally, `<project>/CLAUDE.md` is always read for release-related instructions (legacy compatibility path).

### 10.2 Layer Responsibilities

| Layer | Mutability | Scope | What it controls |
|-------|-----------|-------|-----------------|
| **SPEC.md** | Invariant | All implementations | Command contracts, checklist structure, merge semantics, meta-cognition protocol |
| **SKILL.md** | Workspace-owned | All projects in workspace | Project type tables, dependency graph, secret paths, default verification checks |
| **DEV.md** | Project-owned, shared | Single project | Release steps, extra/skip checks, publication targets, pre/post hooks, custom commands |
| **DEV.local.md** | Project-owned, private | Single project | Secret path overrides, check skipping for local dev, private registry URLs |

### 10.3 DEV.md Format

Per-project `DEV.md` files use YAML frontmatter for structured overrides:

```yaml
---
project_type: rust-maturin
publication_targets:
  - crates.io
  - pypi
  - github-release
extra_checks:
  - name: "WASM build"
    command: "wasm-pack build --target web"
skip_checks:
  - audit-docs
secret_paths:
  crates_io: "org/CARGO_REGISTRY_TOKEN"
  pypi: "org/pypi-token"
pre_release:
  - "make clean && make build"
post_release:
  - "make install"
---

Free-form markdown below frontmatter provides human-readable context:
conventions, gotchas, historical decisions.
```

### 10.4 Merge Semantics

When multiple layers define the same field, the merge strategy depends on the field type:

| Field | Strategy | Behavior |
|-------|----------|----------|
| `project_type` | **Last wins** | Deepest layer's value overrides |
| `publication_targets` | **Replace** | Deepest layer's list replaces entirely |
| `extra_checks` | **Append** | Accumulate from all layers |
| `skip_checks` | **Append** | Accumulate from all layers |
| `secret_paths` | **Deep merge** | Per-target key, deepest layer wins per key |
| `pre_release` | **Append** | Run in order: SKILL.md → DEV.md → DEV.local.md |
| `post_release` | **Append** | Run in order: SKILL.md → DEV.md → DEV.local.md |

### 10.5 Resolution Protocol

Before any operation, the agent MUST:
1. Read SKILL.md (workspace defaults)
2. Check for `<project>/DEV.md` — read if present
3. Check for `<project>/DEV.local.md` — read if present
4. Read `<project>/CLAUDE.md` for release-related instructions
5. Merge configurations using the semantics in §10.4
6. The merged configuration drives the operation

### 10.6 Fallback Behavior

If no `DEV.md` exists for a project:
1. Auto-detect project type from marker files (§4)
2. Use SKILL.md defaults for that project type
3. Read CLAUDE.md for any project-specific overrides

This ensures backward compatibility — projects work without DEV.md files.

### 10.7 Design Rationale

This mirrors Claude Code's own composability model:

| Claude Code | Dev Skill |
|------------|-----------|
| `CLAUDE.md` (project root) | `SKILL.md` (workspace) |
| `CLAUDE.md` (subdirectory) | `DEV.md` (per-project) |
| `CLAUDE.local.md` | `DEV.local.md` |
| `AGENTS.overrides.md` | (future: `DEV.overrides.md` for cross-cutting overrides) |

The key insight: **the skill is not a monolith**. It's a composition of layers, each owned by the appropriate scope. Workspace admins control SKILL.md. Project maintainers control DEV.md. Individual developers control DEV.local.md.

## 11. SPEC Evolution — Human-Driven, AI-Assisted

The SPEC.md is a living document. Its evolution is **human-driven** — the human decides what the skill should do, the AI assists with implementation and refinement. This section defines the process for evolving, customizing, and sharing SPECs.

### 11.1 Bootstrapping a New Dev Skill

To create a `/dev` skill for a new workspace:

1. **Start from an existing SPEC** — copy or reference a published SPEC.md (e.g., this one)
2. **Strip workspace-specific content** — the SPEC should contain only invariant behavior
3. **Create your SKILL.md** — adapt the SPEC to your workspace (project tables, secret paths, dependency graph)
4. **Iterate via conversation** — use agent-doc or direct editing to refine the SPEC and SKILL.md
5. **Add per-project DEV.md files** as needed for project-specific overrides

The AI assists by:
- Detecting project types and suggesting initial SKILL.md content
- Reviewing SPEC changes for completeness and consistency
- Identifying gaps when a release fails due to missing steps
- Proposing checklist additions based on observed patterns

But the **human makes the decisions**: what checks to require, which targets to publish to, what the release workflow should look like.

### 11.2 Borrowing from Other Skill Repos

SPEC.md files are designed to be **borrowed, forked, and remixed**:

- **Copy sections** — take the pre-release checklist from one SPEC, the composability model from another
- **Reference upstream** — keep a comment pointing to the source SPEC for future updates
- **Diverge freely** — there's no obligation to stay in sync. Each workspace's SPEC reflects its own needs

**Discovery pattern:** When starting a new skill, the AI should:
1. Search for published SPEC.md files in similar projects
2. Present relevant sections the human might want to adopt
3. Let the human cherry-pick sections to include
4. Never auto-merge entire SPECs — the human curates

### 11.3 SPEC Sections as Modules

Each numbered section in the SPEC is a **composable module**. A workspace can adopt some and skip others:

| Section | Required? | When to adopt |
|---------|-----------|---------------|
| §3 Commands | Yes | Always — defines the skill's interface |
| §4 Project Type Detection | Yes | Always — needed for multi-project workspaces |
| §5 Pre-Release Checklist | Yes | If using `release` command |
| §6 Release Workflows | Yes | If using `release` command |
| §7 Secret Handling | Yes | If publishing to registries |
| §8 Meta-Cognition | Recommended | For skills that should learn |
| §9 Workspace Integration | Optional | For multi-project workspaces with dependencies |
| §10 Composability | Optional | For workspaces that need per-project overrides |
| §11 SPEC Evolution | Optional | For workspaces that expect the SPEC to evolve |

### 11.4 Version History in SPEC

When modifying the SPEC, document changes so borrowers can track evolution:

```markdown
<!-- spec-changelog
- v2: Added §11 SPEC Evolution (human-driven process)
- v1: Initial specification
-->
```

This is a comment (not rendered) but provides lineage for anyone who forks the SPEC.

### 11.5 Anti-Patterns

- **AI-generated SPEC without human review** — the AI proposes, the human disposes
- **Monolithic SPEC that tries to cover everything** — keep it focused on your workspace's actual needs
- **Blind upstream merge** — always review borrowed sections before adopting
- **SPEC drift from practice** — if the team stops following a SPEC section, remove it rather than leaving dead weight
