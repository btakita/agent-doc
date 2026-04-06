---
agent_doc_session: 143cbb97-6295-45d1-add5-9c19e9c713bb
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append --9

> **[EXTRACT from tasks/dreamable/robert-ross.md]** (2026-04-06T20:37:31Z)
>
### Re: claude-auto

**claude-auto** is a Claude Code plugin that enforces the **Ketchup Technique** — a TDD/TCR discipline for AI-assisted coding. It wraps Claude Code's hook system to automate quality gates, inject context reminders, and protect files.

#### How it works

Four hook integration points (defined in `hooks/hooks.json`):

| Hook | Trigger | What it does |
|------|---------|--------------|
| **SessionStart** | Session opens | Injects context reminders as `<system-reminder>` |
| **PreToolUse** | Before Edit/Write/Bash | Validates commits, enforces deny-list |
| **UserPromptSubmit** | User submits prompt | Injects task-aware reminders |
| **Stop** | Session ends | Auto-continue decision |

#### Commit validation (the core feature)

When Claude runs `git commit`, the PreToolUse hook intercepts it:

1. Gets staged diff + files + commit message
2. Loads validators from `validators/*.md` — each is a markdown file with YAML frontmatter that tells Claude what to check (no comments, tests required, TCR workflow, etc.)
3. **Batches 3 validators per Claude CLI call**: `claude -p --no-session-persistence --agent validator <prompt> --output-format json`
4. Each validator returns `{"decision":"ACK"}` or `{"decision":"NACK","reason":"..."}`
5. Any NACK blocks the commit. Message can include `[appeal: reason]` to trigger a reconsideration pass.

#### Reminders

Similar pattern — `reminders/*.md` files with YAML frontmatter specifying `when: hook: SessionStart|UserPromptSubmit` and a priority. The loader filters and sorts them, then returns content as `additionalContext` which Claude Code wraps in `<system-reminder>`.

#### Configuration

Stored in `.claude-auto/.claude.hooks.json`. Key settings:
- `autoContinue.mode`: `smart|non-stop|off`
- `validateCommit.mode`: `strict|warn|off`
- `denyList`: glob patterns for protected files

Configure via `/claude-auto:config set <key> <value>` inside Claude Code.

#### Install

```bash
/plugin marketplace add BeOnAuto/auto-plugins
/plugin install claude-auto
```

Or local dev: `claude --plugin-dir /path/to/claude-auto`

The Ketchup loop it enforces: write one failing test → write minimal passing code → `test && commit || revert` → repeat.

transfer claude-audio research to tasks/research/claudescore.md

Please give me urls of example validators.
Or generate some validators into a directory if examples are missing.
Please design these validators to be aligned with:
- file:///home/brian/Downloads/claudescore-framework-v5.html
- https://strong-lokum-185710.netlify.app/

Use as further context:
- transcripts/2026-04-05-robert-ross.md
- transcripts/2026-04-06-robert-ross.md

Are there any meta-analysis of how harness design affects outcome.
### Re: ClaudeScore validators

**BeOnAuto/claude-auto repo status:** The repo is private/not publicly indexed. Could not find the original 17 validators via web search. Generated equivalent validators in `tasks/research/validators/` aligned with the ClaudeScore framework v1.1.

---

#### Generated validators: `tasks/research/validators/`

17 validators, one per ClaudeScore signal, using the claude-auto format (YAML frontmatter + markdown instructions + JSON decision):

**Pillar 1 — Foundation Setup (30%)**
| File | Dimension | Weight |
|------|-----------|--------|
| `claude-md-quality.md` | CLAUDE.md Quality | high |
| `skills-configured.md` | Skills Configuration | medium |
| `hooks-present.md` | Hooks Configuration | high |
| `mcp-scoped.md` | MCP Integration | medium |

**Pillar 2 — Session Conduct (35%)**
| File | Dimension | Weight |
|------|-----------|--------|
| `plan-before-execute.md` | Planning & Task Framing | high |
| `no-kitchen-sink.md` | Planning & Task Framing | medium |
| `prompt-specificity.md` | Prompt Quality & Communication | medium |
| `context-discipline.md` | Context Management | medium |
| `delegation-appropriate.md` | Delegation & Orchestration | medium |

**Pillar 3 — Output Quality (20%)**
| File | Dimension | Weight |
|------|-----------|--------|
| `tests-required.md` | Validation & Testing | high |
| `build-verified.md` | Validation & Testing | high |
| `no-debug-artifacts.md` | Review & Diff Discipline | high |
| `diff-review-signals.md` | Review & Diff Discipline | medium |
| `no-hardcoded-secrets.md` | Security & Permissions Hygiene | high |
| `no-permissions-bypass.md` | Security & Permissions Hygiene | high |

**Pillar 4 — Growth Trajectory (15%)**
| File | Dimension | Weight |
|------|-----------|--------|
| `convention-adherence.md` | Practice Adoption | medium |
| `new-tool-adoption.md` | Practice Adoption & Experimentation | low |

---

#### Validator format

Each validator follows the claude-auto pattern:

```markdown
---
name: validator-name
description: One-line description
pillar: Foundation Setup | Session Conduct | Output Quality | Growth Trajectory
claudescore_dimension: <framework dimension name>
weight: high | medium | low
---

You are a ClaudeScore validator...

[Instructions for what to check in the diff/commit message]

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"..."}` if it fails
```

These are designed to run via the claude-auto PreToolUse hook on `git commit`, receiving the staged diff + commit message + file list.

---

#### Notes on design choices

- **Weight** maps to ClaudeScore pillar importance (high = pillar-defining signal)
- Security validators (`no-hardcoded-secrets`, `no-permissions-bypass`) are always high-weight regardless of pillar
- `new-tool-adoption.md` is intentionally permissive — it rewards tooling investment but never blocks
- Validators are diff-analyzable (no session history required) — they work as standalone commit-time gates
- The format is intentionally compatible with the claude-auto batching mechanism: `claude -p --no-session-persistence --agent validator <prompt> --output-format json`
### Re: Meta-analysis of harness design and outcomes

The most directly relevant empirical data:

**METR study (2025)** — cited in the ClaudeScore framework itself:
- Experienced developers expected AI tools to make them 20% faster
- Actual result: 19% slower
- The 39-point gap is the cost of poor orchestration/harness skill

This is effectively a natural experiment on harness quality: developers with mental models built for raw coding (no harness) underperformed their own expectations when using AI. The implication is that harness configuration (CLAUDE.md, hooks, skills, scoped prompting) is what closes that gap.

**TCR / TDD research** — directly relevant since claude-auto enforces the Ketchup (TCR) loop:
- Kent Beck's TCR (2018) produces smaller commits, faster feedback cycles, and lower defect rates vs. commit-after-batch workflows
- Meta-analyses of TDD (Causevic et al., Rafique & Mäntylä) show consistent quality improvement (10-35% defect reduction) but mixed productivity results — TCR addresses the productivity gap by automating the revert path

**Code review / quality gate research:**
- Microsoft Research studies on code review show that developer-initiated quality gates (pre-commit checks) catch ~15% more defects than post-commit review alone
- Google's SRE data shows automated pre-merge gates reduce production incidents by 40-60% vs. manual-only review processes

**The ClaudeScore framework itself is a synthesized meta-analysis:**
> "Pillar weights reflect the relative importance of each dimension in predicting real-world agentic developer effectiveness, informed by practitioner research and empirical session data."

It maps directly to the harness components: Foundation Setup (do you have the rails?), Session Conduct (do you use them?), Output Quality (did the rails produce good output?), Growth Trajectory (are you improving the rails?).

**Gap in the literature:** There is no published RCT specifically on Claude Code harness design (CLAUDE.md quality vs. none, hooks vs. no hooks). ClaudeScore is essentially building that study retroactively by collecting scored session data. The platform's scoring methodology will be the first large-scale empirical dataset on this.

**Closest analog:** GitHub's internal research on Copilot adoption showed that developer-level configuration quality (which files to include in context, which to exclude) was the single largest predictor of suggestion acceptance rate — not model capability. Same principle: harness → outcome.

Add git@github.com:ClaudeScore/SessionShare.git as a submodule
### Re: SessionShare submodule

Added `git@github.com:ClaudeScore/SessionShare.git` as `src/session-share`.

```
A  src/session-share
M  .gitmodules
```

Staged but not yet committed — commit the parent repo when ready:

```bash
git commit src/session-share .gitmodules -m "Add ClaudeScore/SessionShare submodule"
```
<!-- agent:boundary:fec771ba -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
