# Environment Spec — Robert Ross

| Field | Value |
|---|---|
| **Name** | Environment Spec |
| **Source** | SPEC.md |
| **Version** | 1.0.0 |
| **Last Updated** | 2026-04-26 |
| **Purpose** | Declarative specification for Rob's Claude Code environment — used for health checks and drift detection |
| **Audience** | Robert Ross, Claude Code |

Declarative specification for Rob's Claude Code environment. Claude reads this file to verify the environment matches intended configuration. Rob reads it for a structured overview of how each component works.

**Usage:** "Read SPEC.md and verify my environment matches" — Claude compares each section against reality and reports drift.

**Scaling clause:** if any section grows beyond ~50 lines, extract it to `specs/<section-name>.md` and replace the section body with a link. Create the `specs/` directory on first extraction.

---

## Workspace Layout

**Expected structure:**

```
~/Documents/Code/
├── CLAUDE.md                           ← workspace rules + preferences
├── SPEC.md                             ← this file
├── robert-ross-workspace-guide.md      ← reference guide (living document)
├── src/                                ← all code projects
│   ├── SessionShare/                   ← submodule: ClaudeScore/SessionShare
│   └── <other-projects>/               ← submodules or local experiments
└── tasks/                              ← planning docs, session files, notes
    ├── claudescore/
    └── build-party/
```

**Rules:**
- Code projects go under `src/` only — never in the root
- Planning docs go under `tasks/`, grouped by project
- `CLAUDE.md`, `SPEC.md`, and the guide stay in the root

**Check:** `ls ~/Documents/Code/`
**Fix:** move misplaced directories into `src/` or `tasks/` as appropriate

---

## CLAUDE.md Rules

The workspace-level `CLAUDE.md` at `~/Documents/Code/CLAUDE.md` should contain these rule sections:

| Section | Purpose |
|---------|---------|
| Workspace Layout | Tells Claude where projects and docs go |
| Project Scaffolding | What happens when Rob says "new project" |
| Planning Docs | What happens when Rob says "plan [topic]" or "notes for [topic]" |
| Adding Existing Repos | What happens when Rob says "add [repo]" |
| My Preferences | Step-by-step explanations, ask before permanent actions, save reminders |

**Check:** `cat ~/Documents/Code/CLAUDE.md`
**Fix:** compare against the canonical rules in `robert-ross-workspace-setup.md` Step 1 and add missing sections

---

## Hooks

Hooks live in `~/.claude/settings.json` under the `hooks` key. Expected hooks after Phase 3 setup:

| Hook | Trigger | What it does |
|------|---------|-------------|
| Auto-scaffold | On new project creation | Creates `CLAUDE.md` from template, initializes git |
| Auto-commit parent | After submodule add/change | Commits the submodule reference in the parent repo |
| Auto-pull | On session start | Pulls latest changes for the workspace |

**Check:** `cat ~/.claude/settings.json | jq '.hooks // empty'`
**Fix:** Brian installs these during the Zoom setup session (Phase 3). If missing, ask Brian.

---

## MCP Plugins

Rob has MCP plugins already configured. The specific inventory is Rob-managed — run the check command to audit current state.

**Check:** `cat ~/.claude/settings.json | jq '.mcpServers | keys'`
**Reconciliation:** Step 10 captures the current plugin list into this spec. Update the table below after running reconciliation.

| Plugin | Purpose | Status |
|--------|---------|--------|
| *(run check to populate)* | | |

---

## Skills

Rob has skills already configured. The specific inventory is Rob-managed — run the check command to audit current state.

**Check:** `ls ~/.claude/skills/ 2>/dev/null || echo "no skills directory"`
**Reconciliation:** Step 10 captures the current skill list into this spec. Update the table below after running reconciliation.

| Skill | Purpose | Status |
|-------|---------|--------|
| *(run check to populate)* | | |

---

## Git Configuration

**Expected state after setup:**

| Setting | Value |
|---------|-------|
| GitHub auth | Personal access token stored via `gh auth` or `.env` |
| Token scope | Contents (read+write), Administration (read+write) |
| Token name | `claude-code-automation` |
| Remote | `~/Documents/Code` is a git repo with GitHub remote |

**Check:** `gh auth status`
**Fix:** re-run Step 5 from the setup plan (requires Brian)

---

## Guide

`robert-ross-workspace-guide.md` should be present at `~/Documents/Code/robert-ross-workspace-guide.md`. It is a living document — Claude updates it when Rob says "update my guide."

**Check:** `test -f ~/Documents/Code/robert-ross-workspace-guide.md && echo "present" || echo "missing"`
**Fix:** copy from the kit: `cp <kit-path>/robert-ross-workspace-guide.md ~/Documents/Code/`

---

## Document Headers

Every markdown file Rob reads or creates should carry a Document Header — a table immediately after the H1 with six fields.

**Format:**

```markdown
# Document Title

| Field | Value |
|---|---|
| **Name** | Human-readable document name |
| **Source** | Filename (e.g., `my-plan.md`) |
| **Version** | Semantic version (start at `1.0.0`) |
| **Last Updated** | Date of last edit (YYYY-MM-DD) |
| **Purpose** | One sentence — what this document is for |
| **Audience** | Who reads it (e.g., "Robert Ross", "Robert Ross, Claude Code") |
```

**Rules:**
- Add a header when creating any new `.md` file in the workspace
- Bump the version when editing an existing file:
  - **Patch** (1.0.0 → 1.0.1): typo fixes, wording tweaks
  - **Minor** (1.0.0 → 1.1.0): new sections, structural changes
  - **Major** (1.0.0 → 2.0.0): rewrite, purpose change
- Update `Last Updated` on every edit
- `Source` is the filename relative to where Rob will find it (not a full path)
- `Audience` is always at least "Robert Ross"

**Excluded from headers:**
- Session files (agent-doc managed)
- Files with YAML frontmatter (headers could conflict)
- Temporary/scratch files

**Check:** all `.md` files in `~/Documents/Code/` (except `tasks/*.md` session files) should have a Document Header table after the H1
**Fix:** ask Claude to add the header: "Add a Document Header to this file"

---

## Codex CLI (future)

Not yet installed. Setup plan: [`codex-setup-plan.md`](codex-setup-plan.md).

**Expected state after setup:**

| Component | Value |
|-----------|-------|
| Binary | `codex` via `npm install -g @openai/codex` |
| Shell alias | `cx` → `cd ~/Documents/Code && codex` |
| Config dir | `~/.codex/` (separate from `~/.claude/`) |
| Instructions | Reads `AGENTS.md` (symlink created in Step 4: `AGENTS.md -> CLAUDE.md`) |

**Check:** `codex --version`
**Status:** pending — complete Claude Code setup first
