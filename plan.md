---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status: Active Development

Core CLI functional, installed user-wide, dog-fooding via plan.md sessions.

## Decisions Made

| Decision | Outcome |
|----------|---------|
| Tool name | `agent-doc` (not claude-doc — agent-agnostic) |
| Workspace name | `agent-loop` (super project with submodules) |
| Language | Rust (single binary, fast, portable) |
| Standalone vs skill | Standalone CLI tool |
| Session continuity | Fork from current session on first run, resume on subsequent |
| Diff strategy | Send diff + full doc on every run (cheap insurance against compaction) |
| Git integration | Pre-commit user, no post-commit agent (diff gutters), `-b` for branch, `clean` for squash |
| Merge safety | 3-way merge via `git merge-file` if file edited during agent response |
| Tool permissions | Terminal sidecar for MVP (interactive prompts in terminal) |
| Document format | YAML frontmatter + `## User` / `## Assistant` blocks + inline annotations |
| Frontends | CLI (`agent-doc run`), Claude Code skill (`/agent-doc`), JetBrains External Tool |

## Repo Structure

```
agent-loop/                          github.com/btakita/agent-loop (public)
  agent-doc/                         github.com/btakita/agent-doc (public, submodule)
  corky/                             github.com/btakita/corky (public, submodule)
  mail -> ~/work/btakita/mail        private data, gitignored symlink
```

## agent-doc — What's Built

Rust project at `~/work/btakita/agent-doc/`, compiles clean.

### Modules implemented

```
src/
  main.rs           # CLI (clap derive) — run, init, diff, reset, clean, audit-docs
  submit.rs         # Core loop: diff, send, merge-safe write, snapshot, pre-commit
  init.rs           # Scaffold session document with frontmatter
  reset.rs          # Clear session ID + delete snapshot
  diff.rs           # Preview diff (dry run) using similar crate
  clean.rs          # Squash agent-doc git commits
  frontmatter.rs    # YAML frontmatter parse/write/set_session_id
  snapshot.rs       # SHA256 path, load, save, delete
  git.rs            # commit (git add -f), create_branch, squash_session
  config.rs         # ~/.config/agent-doc/config.toml — wired into init/run
  audit_docs.rs     # Audit instruction files (CLAUDE.md, AGENTS.md, SKILL.md)
  agent/
    mod.rs          # Agent trait + config-aware resolve()
    claude.rs       # Claude backend — -p --output-format json --permission-mode acceptEdits
tests/
  test_cli.rs       # CLI integration tests (11 tests)
```

### CLI

```
agent-doc run <file> [-b] [--agent <name>] [--model <model>] [--dry-run] [--no-git]
agent-doc init <file> [title] [--agent <name>]
agent-doc diff <file>
agent-doc reset <file>
agent-doc clean <file>
agent-doc audit-docs
```

## Completed

- [x] Core CLI — run, init, diff, reset, clean, audit-docs
- [x] Config system — `~/.config/agent-doc/config.toml`, agent backends configurable
- [x] Claude backend — `-p --output-format json --permission-mode acceptEdits`
- [x] `CLAUDECODE` env var unset in claude backend (nested session guard)
- [x] `git add -f` for gitignored session docs
- [x] Merge-safe writes via `git merge-file` (3-way merge with conflict markers)
- [x] Pre-commit user changes / no post-commit (diff gutter visibility)
- [x] `--version` flag (clap derive from Cargo.toml)
- [x] README.md diagram aligned
- [x] Tests — 52 tests (41 unit + 11 CLI integration)
- [x] User-wide install via `~/bin/agent-doc`
- [x] SPECS.md for both agent-doc and agent-loop
- [x] Dog-fooding via plan.md sessions
- [x] mdbook documentation (docs/, book.toml, SUMMARY.md, full guide)
- [x] Release infrastructure — CI, release (5-target matrix), PyPI, GitHub Pages workflows
- [x] pyproject.toml (maturin), install.sh, VERSIONS.md
- [x] Cargo.toml release profile (lto=fat, codegen-units=1, panic=abort, strip=true)
- [x] SKILL.md — Claude Code skill for `/agent-doc` invocation
- [x] Rename `submit` → `run` CLI command + all docs
- [x] Skill installed in agent-loop, agent-doc, and corky workspaces
- [x] AGENTS.md + CLAUDE.md symlink convention for agent-loop

## What's Next

### Immediate

1. Tag v0.1.0 and release (CI, crates.io, PyPI, GitHub Releases)
2. More test coverage — frontmatter parsing, diff computation, snapshot round-trip

### agent-doc

1. More test coverage — frontmatter parsing, diff computation, snapshot round-trip
2. Codex frontend skill (same pattern as Claude skill)

### agent-loop

1. Ensure `corky init` works from agent-loop root (mail/ symlink resolution)

### Future

- Streaming responses
- Document-only permission flow (no terminal)
- Desktop notifications on completion
- Multi-agent documents (sections routed to different agents)
- Email transport for async human-agent loops (bridge with corky)

## User


Let's clean up this plan.md file. I want to see your realtime chain of thought in the Claude Code terminal.

## Assistant

Cleaned up plan.md:

- **Updated Decisions table** — git integration row now says "Pre-commit user, no post-commit agent (diff gutters)", added Merge safety and Frontends rows
- **Updated modules list** — `submit.rs` comment now mentions merge-safe writes
- **Updated Completed** — added all recent work (merge-safe writes, pre-commit flow, --version, mdbook docs, release infrastructure, SKILL.md, Cargo.toml release profile)
- **Updated What's Next** — added "Immediate" section (skill install, build/release, CLAUDE.md setup), trimmed editor docs (already done in mdbook)
- **Trimmed conversation history** — removed ~300 lines of resolved User/Assistant exchanges, left a clean `## User` block for the next message

**Still pending from last request:** The skill installation and build/commit need the `.claude/` directory permissions. Run this to unblock:

```bash
mkdir -p ~/work/btakita/agent-loop/.claude/skills/agent-doc
mkdir -p ~/work/btakita/agent-loop/agent-doc/.claude/skills/agent-doc
mkdir -p ~/work/btakita/agent-loop/corky/.claude/skills/agent-doc
cp ~/work/btakita/agent-loop/agent-doc/SKILL.md ~/work/btakita/agent-loop/.claude/skills/agent-doc/
cp ~/work/btakita/agent-loop/agent-doc/SKILL.md ~/work/btakita/agent-loop/agent-doc/.claude/skills/agent-doc/
cp ~/work/btakita/agent-loop/agent-doc/SKILL.md ~/work/btakita/agent-loop/corky/.claude/skills/agent-doc/
```

## User

Do I need to run plan.md from the Claude Code terminal to see realtime updates?

## Assistant



Right now, yes — `agent-doc submit plan.md` runs via `claude -p` (pipe mode), which is non-interactive. It sends the prompt, waits for the full response, then writes it to the file. No streaming to the terminal.

There are two ways to get realtime feedback:

**1. Claude Code terminal (current):** You type your edits in the editor, then switch to the Claude Code terminal and paste/type the file path. Claude processes it in the current interactive session — you see the streaming response in real-time, AND it writes back to the document. This is what the `/agent-doc` skill is designed for once installed.

**2. Pipe mode with progress (current `agent-doc submit`):** You only see `eprintln!` status messages ("Submitting to claude...", "Merge successful"), not the actual response streaming. The response appears all at once when the file is written.

**The skill (`/agent-doc plan.md`) is the solution** — it runs inside the interactive Claude Code session, so you get streaming AND file writes. But it requires the `.claude/skills/` directories to be created first.

I also notice from the diff that the previous submit produced merge conflict markers (`<<<<<<< agent-response` / `||||||| original` / `>>>>>>> your-edits`). This means you edited the file while the agent was responding and the merge had a conflict in the same region. The cleanup you did (removing the markers and keeping your version) was the right fix — but it confirms the merge-safe flow is working as designed.

**Bottom line:** Once you run the skill install commands, you can use `/agent-doc plan.md` in the Claude Code terminal for realtime streaming + document writes. No need for a separate terminal or External Tool.

## User

Rename agent-doc submit to agent-doc run.

## Assistant

Good call — `run` is shorter and more intuitive (mirrors `cargo run`, `npm run`).

**Changes needed:**
- `src/main.rs` — rename `Commands::Submit` to `Commands::Run` with `#[command(name = "run")]`, keep `submit` as a hidden alias
- Docs: SKILL.md, README.md, SPECS.md, commands.md, plan.md — update `submit` → `run`
- `submit.rs` stays as-is (internal module name, no user-facing impact)

Ready to implement.

## User

Implement the `run` command. No need for backward compatibility...because I'm the only one using it.

But I also want to submit this as a claude plugin so other people can use it.

Does the crate need to be released to crates.io or pypi first?

## Assistant

Renamed `submit` → `run`. All 52 tests pass, release rebuilt. `agent-doc run plan.md` works now.

**Distribution:** No, you don't need crates.io/PyPI to distribute as a Claude Code skill. The SKILL.md is self-contained. Users just need:

1. The `agent-doc` binary on PATH
2. SKILL.md in `.claude/skills/agent-doc/`

For the **binary itself**, publishing makes install easy for others:
- `cargo install agent-doc` — crates.io (Rust users)
- `pip install agent-doc` — PyPI (everyone, via maturin binary wheels)
- `curl | sh` — install.sh from GitHub Releases

The GitHub Actions workflows already handle all three on tag push. Just: `git tag v0.1.0 && git push --tags`.

## User

audit docs. What else is outstanding?

## Assistant

**`agent-doc audit-docs` results** (run from agent-doc/):

| File | Issue |
|------|-------|
| `CLAUDE.md:19-34` | Large code block (14 lines) without imperative context |
| `.claude/skills/agent-doc/SKILL.md` | Stale — older than src/main.rs |
| `CLAUDE.md` | Stale — older than src/main.rs |
| `README.md` | Stale — older than src/main.rs |

Line budget: 302/1000 ✓

**Outstanding work:**

| # | Item | Scope |
|---|------|-------|
| 1 | **Commit `submit` → `run` rename** — main.rs, test_cli.rs, SKILL.md unstaged | agent-doc |
| 2 | **Update 60+ doc references from `submit` to `run`** — README.md, SPECS.md, SKILL.md, CLAUDE.md, VERSIONS.md, all mdbook docs, plan.md static sections | agent-doc |
| 3 | **Update plan.md static sections** — CLI block, Frontends row, modules list still say "submit"; Completed missing "submit→run rename" | plan.md |
| 4 | **Install skill in corky** — `.claude/skills/agent-doc/SKILL.md` not in corky workspace | corky |
| 5 | **Update CLAUDE.md module layout** — missing `audit_docs.rs` | agent-doc |
| 6 | **Push** — agent-loop 26 commits ahead, agent-doc 2 ahead | both |
| 7 | **Tag v0.1.0** — release workflows ready, never triggered | agent-doc |
| 8 | **CLAUDE.local.md for agent-loop** — listed as immediate, not created | agent-loop |

**Recommendation:** Do #2 first (bulk `submit`→`run` across all docs), then #1 (commit everything), then #3 (update plan.md static sections), then #6 (push). Items 4, 7, 8 can wait.

## User

Do 2...then 1...then 3...then 6.

Then 4.
 
agent-loop should be on a branch with the commits. with main branch reset to origin/main.

Hold of on 7 & 8. Could you explain 8 a bit more?

## User

Did we do 5 yet? Explain 8 more.

## Assistant

**#5 — Yes, done.** Added `audit_docs.rs` to CLAUDE.md module layout as part of the bulk submit→run commit (`dbc91f9`).

**#8 — CLAUDE.local.md for agent-loop:**

`CLAUDE.local.md` is a gitignored personal override file. The convention (from corky):

- `CLAUDE.md` — committed, shared instructions everyone sees
- `CLAUDE.local.md` — gitignored, personal preferences that don't belong in the repo

For agent-loop, you'd use it for:
- Local file paths (e.g. where `mail/` symlink points)
- Personal agent preferences (preferred model, custom backends)
- Workflow preferences (e.g. "always use bun", "commit style")
- System-specific build quirks

It's completely optional. The existing `CLAUDE.md` works fine without one. Skip it unless you have something specific to put there.

## User

Let's Rename CLAUDE.md to AGENTS.md and symlink CLAUDE.md to AGENTS.md. Add AGENTS.override.md to .gitignore.

touch CLAUDE.local.md...might as well. Make note that we are dogfooding the agent-doc cli and skill.

Is there anything else we should before merging in the PR, releasing the next version to crates, pypi, claude plugins & then tagging?

## Assistant

**Pre-release checklist:**

| # | Item | Status |
|---|------|--------|
| 1 | Tests (52 pass) | done |
| 2 | Clippy clean | done |
| 3 | audit-docs | 1 warning (large code block in CLAUDE.md) — not blocking |
| 4 | submit→run rename in code + docs | done, pushed to agent-doc origin |
| 5 | Cargo.toml metadata (name, description, license, repository, readme) | done |
| 6 | plan.md static sections still say "submit" in a few places | cosmetic, fix now or after |
| 7 | Commit AGENTS.md rename + .gitignore in agent-loop | **needs commit** |
| 8 | Merge `agent-doc-session` branch into agent-loop main | **needs merge** |
| 9 | CI workflows (`.github/workflows/`) | **need to verify** |

**Re: "claude plugins"** — There's no Claude Code plugin marketplace yet. Distribution is manual: users install the binary + copy SKILL.md into `.claude/skills/agent-doc/`. The README already documents all install methods (cargo install, pip install, curl|sh).

**Suggested order:**
1. Fix plan.md static sections (#6)
2. Commit AGENTS.md rename (#7)
3. Merge branch into main (#8)
4. Verify CI (#9)
5. `git tag v0.1.0 && git push --tags` — triggers release workflows
6. After CI: `cargo publish` (crates.io) and PyPI publishes automatically via maturin workflow

Want me to proceed with 1-4?

## User

It should be CLAUDE.local.md.

Proceed with 1-4. Make sure we squash the commits when merging into main.

## User

Make sure that you use AGENTS.override.md and CLAUDE.local.md for the personal preferences.

## Assistant

All done. Here's what happened:

**#6 — Plan.md static sections fixed:** Updated all remaining "submit" → "run" in Decisions table, modules list, CLI block, Completed section. Updated What's Next.

**#7 — Committed:** AGENTS.md rename (canonical, CLAUDE.md → symlink), `.gitignore` updated with `AGENTS.override.md`, CLAUDE.local.md created with dogfooding note, submodules updated, SKILL.md + SPECS.md installed.

**#8 — Squash merged:** 32 commits from `agent-doc-session` squashed into 1 commit on main:
```
c68e6e0 agent-doc session: audit-docs, submit→run rename, AGENTS.md convention
```

**#9 — CI verified:** All 4 workflows correct:
- `ci.yml` — tests + clippy on push/PR
- `release.yml` — 5-target binary builds on `v*` tag
- `pypi.yml` — maturin wheels + sdist on release publish
- `book.yml` — mdbook to GitHub Pages on main push

**Pushed** main to origin.

**Ready to release.** Next steps:
1. `cd agent-doc && git tag v0.1.0 && git push --tags` — triggers release + pypi workflows
2. Verify CI passes on GitHub
3. `cargo publish` (if not automated) for crates.io

## User

Release it!

## Assistant

**v0.1.0 released!**

Tagged `v0.1.0` and pushed. Release workflow completed — all 5 targets built successfully:

| Target | Status |
|--------|--------|
| x86_64-unknown-linux-gnu | built (55s) |
| aarch64-unknown-linux-gnu | built (1m45s) |
| aarch64-apple-darwin | built (1m4s) |
| x86_64-apple-darwin | built (46s) |
| x86_64-pc-windows-msvc | built (2m15s) |

GitHub Release: https://github.com/btakita/agent-doc/releases/tag/v0.1.0

**PyPI workflow didn't trigger** — GitHub blocks workflow-on-workflow triggers when the release is created by `github-actions[bot]` (anti-recursion protection). Two options:
1. Manual: run the PyPI workflow from the Actions tab
2. Fix: use a PAT in the release workflow's `GITHUB_TOKEN` so the `release: published` event triggers downstream workflows

**crates.io** also needs manual `cargo publish` (no automation for that in CI).

**Book/docs** failed because GitHub Pages isn't enabled yet — go to Settings > Pages > Source: GitHub Actions.

Set up...go ahead and fetch.

## User

Let's have the publishing as a step. Perhaps in the Makefile for agent-doc and corky.
