# Workspace Setup Plan — Robert Ross

| Field | Value |
|---|---|
| **Name** | Workspace Setup Plan |
| **Source** | robert-ross-workspace-setup.md |
| **Version** | 1.0.0 |
| **Last Updated** | 2026-04-26 |
| **Purpose** | Step-by-step executable plan for setting up Rob's Claude Code workspace |
| **Audience** | Robert Ross, Claude Code |

Claude Code: read this file and execute each step in order. Pause after each step to confirm it worked before moving on. Steps marked **(with Brian)** need Brian's help — ask the user to loop Brian in for those.

**Model:** Sonnet is fine for all of these.

---

## Step 1: Update CLAUDE.md with workspace rules

Add these rules to `~/Documents/Code/CLAUDE.md`. Keep any existing content and add this at the top:

```markdown
## Workspace Layout
- All projects go under ~/Documents/Code/src/ (submodules and local experiments)
- Planning docs, session files, and notes go under ~/Documents/Code/tasks/
- Never put code projects directly in ~/Documents/Code/ root

## Project Scaffolding
- When I say "new project", create a folder in ~/Documents/Code/src/
- Ask me if it should be a submodule (its own GitHub repo) or local-only
- Always create a CLAUDE.md inside new projects with the project description
- Commit and push after creating the project structure

## Planning Docs
- When I say "plan [topic]" or "notes for [topic]", create a file under ~/Documents/Code/tasks/
- Group by project: tasks/claudescore/, tasks/build-party/, etc.
- Planning docs are markdown — not code

## Adding Existing Repos
- When I say "add [repo]", clone it as a submodule under ~/Documents/Code/src/
- If I give a GitHub URL or org/repo name, use that directly
- Create a project-level CLAUDE.md if the repo doesn't have one
- After adding, commit the submodule link and push

## Environment Verification
- When verifying my environment, read SPEC.md and check each section
- When something seems off, suggest running the SPEC.md health check

## My Preferences
- Explain what you're doing step by step
- When in doubt, ask before doing something permanent
- Remind me to save (commit) before switching to a different project
```

Confirm: show the updated CLAUDE.md to the user.

## Step 2: Inventory the workspace

List all folders and files at the top level of `~/Documents/Code/`. Show the output to the user so they can see what will be moved.

## Step 3: Migrate to src/ + tasks/ structure

**IMPORTANT: Review with the user before executing.** This step moves existing project directories from `~/Documents/Code/*/` into `~/Documents/Code/src/`. The user must confirm which directories to move and which to leave in place.

Once the user confirms:
- Create `src/` and `tasks/`
- Move confirmed project folders into `src/` (use `git mv` for tracked items, plain `mv` for untracked)
- Update `.gitmodules` paths if there are submodules
- Leave `CLAUDE.md` and config files in the root
- Commit and push

## Step 4: Verify

Show the new directory layout. It should look like:

```
~/Documents/Code/
├── CLAUDE.md
├── src/
│   ├── (projects...)
│   └── ...
└── tasks/
```

## Step 5: Set up GitHub personal access token (with Brian)

Ask Brian to help with this step:

1. github.com → Settings → Developer settings → Personal access tokens → Fine-grained tokens
2. Name: `claude-code-automation`
3. Resource owner: select your org (ClaudeScore) or personal account
4. Repository access: "All repositories"
5. Permissions: Contents (read + write), Administration (read + write)
6. Generate → copy → store for Claude to use

## Step 6: Add existing repos as submodules

After the token is set up, add repos the user specifies. Start with:
- `ClaudeScore/SessionShare`

For each repo:
1. Clone as a submodule under `~/Documents/Code/src/`
2. Create a project-level CLAUDE.md if missing
3. Commit the submodule link in the parent repo
4. Push

## Step 7: Install Claude Code hooks (with Brian)

Ask Brian to help install automation hooks:
- Auto-create CLAUDE.md for new projects
- Auto-commit parent repo after submodule changes
- Auto-pull latest on session start

## Step 8: Install reference files

Copy the guide, spec, and tasks README to Rob's workspace:

```bash
cp robert-ross-workspace-guide.md ~/Documents/Code/
cp example-tasks-readme.md ~/Documents/Code/tasks/README.md
```

**For SPEC.md — reconcile if one already exists:**

1. Check: `test -f ~/Documents/Code/SPEC.md && echo "exists" || echo "new"`
2. If **new**: `cp SPEC.md ~/Documents/Code/`
3. If **exists**: read both files and merge:
   - Keep any existing sections Rob has already customized (plugins, skills, hooks with actual data)
   - Add any new sections from the kit SPEC.md that don't exist yet (Document Headers, Environment Verification)
   - Preserve Rob's existing version numbers — don't downgrade
   - Update the Document Header's `Last Updated` field to today
   - If conflicts exist (same section, different content), prefer Rob's version for populated tables and kit version for empty/placeholder tables

After merging, show the result to Rob for confirmation before overwriting.

Confirm all files are present: `ls ~/Documents/Code/robert-ross-workspace-guide.md ~/Documents/Code/SPEC.md ~/Documents/Code/tasks/README.md`

## Step 9: Explore skills and plugins (optional, self-serve)

This is your step — Brian doesn't manage your skills or plugins. See Section 6 of the workspace guide for full details.

> "What skills do I have installed?"

> "Create a skill called 'meeting-notes' that formats meeting notes with attendees, action items, and next steps."

> "What MCP plugins are available? I want to connect to Figma."

You can always update your guide to match your current setup:

> "Read robert-ross-workspace-guide.md and update it to reflect my current skills, plugins, hooks, and workspace layout."

## Step 10: Reconcile environment against spec

After all previous steps are complete, verify the environment matches the spec:

> "Read SPEC.md and verify my environment matches. Fix anything that's off."

Claude reads each section of `SPEC.md`, runs the check commands, and reports what matches and what doesn't. For any drift it finds, it either fixes the issue automatically or tells you what needs manual attention (like a missing GitHub token).

After reconciliation, Claude updates `SPEC.md` to reflect the actual state of Rob-managed components (plugins, skills) so the spec stays accurate going forward.
