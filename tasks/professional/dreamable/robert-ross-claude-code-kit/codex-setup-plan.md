# Codex CLI Setup Plan — Robert Ross

Follow-up to the Claude Code workspace setup. Adds OpenAI's Codex CLI as a second agent terminal alongside Claude Code. Only proceed after Rob has a working Claude Code setup (all steps in `robert-ross-workspace-setup.md` complete).

**Prereqs:** working Claude Code environment, `~/Documents/Code/` workspace structure in place.

---

## Step 1: Install Codex CLI

```bash
npm install -g @openai/codex
```

Confirm: `codex --version` prints a version number.

## Step 2: Authenticate

```bash
codex auth
```

Follow the prompts to log in with OpenAI credentials. Rob needs an OpenAI API key or organization access.

## Step 3: Shell alias

Add to `~/.zshrc`:

```bash
alias cx='cd ~/Documents/Code && codex'
```

Then `source ~/.zshrc`. Now `cx` launches Codex in the workspace, parallel to `cc` for Claude Code.

## Step 4: Create AGENTS.md for Codex

Codex reads `AGENTS.md` for project instructions (equivalent to Claude Code's `CLAUDE.md`). Rob's workspace has `CLAUDE.md` but not `AGENTS.md`, so create a symlink:

```bash
cd ~/Documents/Code
ln -s CLAUDE.md AGENTS.md
git add AGENTS.md
git commit -m "Add AGENTS.md symlink for Codex compatibility"
git push
```

Confirm: `ls -la ~/Documents/Code/AGENTS.md` shows `AGENTS.md -> CLAUDE.md`, and `codex` in the workspace picks up the instructions.

## Step 5: Codex-specific config (optional)

If Rob wants Codex-specific settings:

- Config lives in `~/.codex/` (separate from `~/.claude/`)
- Hooks: `.codex/hooks.json`
- No MCP plugin system in Codex — tool access is handled differently

These are power-user concerns. Skip unless Rob has a specific need.

## Step 6: Dual-tool workflow

Both tools work in the same workspace without conflict:
- `cc` — Claude Code (Anthropic)
- `cx` — Codex (OpenAI)
- Same instructions via `AGENTS.md -> CLAUDE.md` symlink, same `src/` + `tasks/` structure
- Git state is shared — commits from either tool are visible to both

**Note:** if Rob runs both simultaneously in the same repo, coordinate to avoid merge conflicts on the same files.
