# Context Audit Runbook

Procedure for auditing Claude Code context efficiency. Requires `/context`
output as input.

## Audit Categories

Audit each category from largest overhead to smallest. Run checks in parallel
where possible.

### MCP Servers

Each server loads full tool definitions into context every turn (~15-20k tokens).

- Count configured servers from settings.json
- Flag any with CLI alternatives (Playwright, Google Workspace, GitHub all
  have CLIs that cost zero tokens when idle)
- Report total MCP overhead from /context output

### CLAUDE.md Files

Read all CLAUDE.md files (project root, .claude/, ~/.claude/). Count lines.
Test every rule against five filters:

| Filter | Flag when... |
|--------|-------------|
| Default | Claude already does this without being told |
| Contradiction | Conflicts with another rule in same or different file |
| Redundancy | Repeats something already covered elsewhere |
| Bandaid | Added to fix one specific bad output, not improve outputs generally |
| Vague | Would be interpreted differently each time |

If total lines > 200, check for progressive disclosure opportunities: rules
that only apply to specific tasks should move to reference files with one-line
pointers. Only recommend splitting when actually bloated -- a lean file with
universal context is fine as-is.

### Skills

Scan `.claude/skills/*/SKILL.md`. For each skill:
- Count lines (flag > 200, critical > 500)
- Run the same five filters on instructions
- Check for restated goals, hedging, synonymous instructions

### Settings

Check settings.json for:

| Setting | Flag if | Recommended |
|---------|---------|-------------|
| autocompact_percentage_override | Missing or > 80 | 75 |
| BASH_MAX_OUTPUT_LENGTH (env) | At default (30-50K) | 150000 |

### File Permissions

Check settings.json for `permissions.deny` rules. If missing, check whether
bloat directories exist:

| If this exists... | Should deny... |
|-------------------|---------------|
| package.json | node_modules, dist, build, .next, coverage |
| Cargo.toml | target |
| go.mod | vendor |
| pyproject.toml / requirements.txt | __pycache__, .venv, *.egg-info |

## Scoring

Score starts at 100. Deductions:

| Issue | Points |
|-------|--------|
| CLAUDE.md > 200 lines | -10 |
| CLAUDE.md > 500 lines | -20 |
| Per 5 rules flagged by filters | -5 |
| Contradictions between files | -10 |
| Missing autocompact override | -10 |
| Missing bash output override | -5 |
| Skill > 200 lines | -5 each |
| Skill > 500 lines | -10 each |
| Per MCP server | -3 each |
| No deny rules + bloat dirs exist | -10 |

Floor at 0. Labels: 90-100 CLEAN, 70-89 NEEDS WORK, 50-69 BLOATED, 0-49 CRITICAL.

## Report Format

```
# Context Audit

Score: {N}/100 [{CLEAN|NEEDS WORK|BLOATED|CRITICAL}]

## Context Breakdown (from /context)
{Key numbers from /context output}

## Issues Found

### [{CRITICAL|WARNING|INFO}] {Category}
{What's wrong}
Fix: {One-line actionable fix}

### Rules to Cut
{Each flagged rule: the text, which filter, one-line reason}

### Conflicts
{Contradictions between files, with paths}

## Top 3 Fixes
1. {Highest-impact fix}
2. {Second}
3. {Third}
```

Severity: CRITICAL > 10pts, WARNING 5-10pts, INFO < 5pts.

## Offer to Fix

After the report, offer:
- Show cleaned-up CLAUDE.md with flagged rules removed
- Add missing settings.json configs
- Add permissions.deny rules for build artifacts
- Show which skills to compress

Auto-apply settings.json and permissions.deny (safe, reversible).
Show diffs for CLAUDE.md and skills -- let the user confirm before modifying
instruction files.
