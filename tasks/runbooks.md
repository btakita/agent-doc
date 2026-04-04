---
agent_doc_session: 0f91fe16-548b-403c-9a53-79ebe1a9ed79
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
In AGENTS.md and in the SKILL.md that we control...Please propose any set of instructions that can be extracted as a runbook
### Re: Extractable runbook procedures

After reviewing all SKILL.md files and CLAUDE.md, here are procedures that are currently inline but would benefit from extraction as standalone runbooks (following the `compact-exchange.md` pattern):

**1. Release Orchestration** (`dev/SKILL.md` §release)
The release workflow for each project type (Rust+Maturin, Rust Library, Python, JS/TS) is ~60 lines of procedural steps. Extract as:
- `runbooks/release-rust-maturin.md` — the 11-step crates.io + PyPI + GitHub Release flow
- `runbooks/release-rust-library.md` — crates.io-only flow
- `runbooks/release-python.md` — PyPI-only flow
- `runbooks/release-js-deploy.md` — Docker + VPS deploy flow

**2. Email Send Workflow** (`email/SKILL.md` §send)
The draft → push-to-drafts → wait-for-approval → send flow with SMTP vs gmail-api branching. Extract as:
- `runbooks/email-send.md`

**3. Livestream Secret Audit** (`livestream/SKILL.md` §audit)
The secret scanning procedure with pattern tables and remediation. Extract as:
- `runbooks/livestream-audit.md`

**4. Submodule Commit + Parent Update** (`CLAUDE.md` §Git rules)
The commit-from-within → push → update-parent-pointer procedure. Extract as:
- `runbooks/submodule-commit.md`

**5. Pre-Release Verification Checklist** (`dev/SKILL.md` §Pre-release verification)
The 9-item verification matrix (tests, audit-docs, README, VERSIONS.md, etc.). Extract as:
- `runbooks/pre-release-checklist.md`

**6. Module Harness Audit** (`module-harness/SKILL.md` §audit)
The inventory → coverage check → drift detection flow. Extract as:
- `runbooks/module-harness-audit.md`

**7. Unpublished Tag Recovery** (`dev/SKILL.md` §Unpublished tag recovery + MEMORY.md)
The tag-delete → re-tag → force-push procedure for recovering from failed publishes. Extract as:
- `runbooks/unpublished-tag-recovery.md`

**Recommendations:**
- Items 1, 4, 5, and 7 are the highest-value extractions — they're referenced frequently and easy to get wrong
- Items 2 and 3 are medium-value — used less often but complex enough to benefit from standalone docs
- Item 6 is lower-value — the module-harness binary already encodes most of the logic

Do whatever will save on context size for all agent runs.
I want a % of context saved for a blog post.
### Re: Context-saving runbook extraction

Extracting all 7 runbooks now. Each goes under its skill's `runbooks/` directory. The SKILL.md files will be trimmed to reference the runbook path instead of inlining the procedure.

**Estimated context savings per agent run:**
- dev/SKILL.md: ~120 lines removed (release workflows + checklist + tag recovery)
- livestream/SKILL.md: ~50 lines removed (audit procedure + pattern tables)
- email/SKILL.md: ~25 lines removed (send workflow)
- CLAUDE.md submodule rules: ~5 lines (small but frequently loaded)
- module-harness/SKILL.md: ~15 lines removed

Total: ~215 lines of procedural content moved to on-demand loading.
### Re: Done

All 10 runbooks created and 4 SKILL.md files trimmed:

**Created:**
- `.claude/skills/dev/runbooks/release-rust-maturin.md`
- `.claude/skills/dev/runbooks/release-rust-library.md`
- `.claude/skills/dev/runbooks/release-python.md`
- `.claude/skills/dev/runbooks/release-js-deploy.md`
- `.claude/skills/dev/runbooks/pre-release-checklist.md`
- `.claude/skills/dev/runbooks/unpublished-tag-recovery.md`
- `.claude/skills/dev/runbooks/submodule-commit.md`
- `.claude/skills/email/runbooks/email-send.md`
- `.claude/skills/livestream/runbooks/livestream-audit.md`
- `.claude/skills/module-harness/runbooks/module-harness-audit.md`

**Trimmed:**
- `dev/SKILL.md` — ~85 lines removed (release workflows, checklist, tag recovery)
- `email/SKILL.md` — ~20 lines removed (send workflow details)
- `livestream/SKILL.md` — ~45 lines removed (audit patterns, deny-list)
- `module-harness/SKILL.md` — ~4 lines removed (audit steps)

Each SKILL.md now has a one-line reference to the runbook. Agents load the runbook only when executing that specific procedure.
### Re: Context savings for blog post

Here are the actual measured savings:

| Skill | Before | After | Saved | % Saved |
|-------|--------|-------|-------|---------|
| dev | 273 lines | 196 lines | 77 lines | 28% |
| livestream | 98 lines | 46 lines | 52 lines | 53% |
| email | 117 lines | 100 lines | 17 lines | 15% |
| module-harness | 113 lines | 110 lines | 3 lines | 3% |
| **Total** | 601 lines | 452 lines | 149 lines | **25%** |

**Key insight for the blog post:** The savings compound per-skill. When `/dev release` runs, it loads 196 lines instead of 273 (28% less), and only reads the specific runbook it needs (e.g., `release-rust-maturin.md` at 28 lines). The agent never loads the Python, JS/TS, or library release workflows it doesn't need.

**Before:** Every `/dev` invocation loaded all 4 release workflows, the full checklist, and tag recovery -- even if the agent was just running `/dev status`.

**After:** `/dev status` loads only the trimmed SKILL.md (196 lines). `/dev release` for a Rust+Maturin project reads SKILL.md (196) + the relevant runbook (~30) = 226 lines vs 273 -- and more importantly, loads only the applicable workflow.

The biggest win is **livestream** at 53% -- the audit pattern tables and deny-list are only needed when running `/livestream audit`, not for `/livestream on` (which is the common case).

Add btakita/agent-runbooks repo as a submodule to agent-loop
### Re: agent-runbooks submodule added

Added `btakita/agent-runbooks` as a submodule at `src/agent-runbooks/`. Current contents:
- `examples/` directory
- `LICENSE`
- `README.md`

The submodule is staged but not committed yet. Should I commit it, or do you want to move the runbook files we created into this repo first?

commit and push first.
### Decisions from agent-doc-bugs session (2026-04-04)

**Naming:** Section name is `## Runbooks` (not Operations). Crate name would be `agent-runbooks`.

**Trigger format:** `` `trigger phrase` — runbooks/filename.md `` (backtick-quoted, relative path)

**Corky extraction candidates:** review-inbox, draft-reply, enrich-contact. Email skill's `email-send.md` should consolidate into corky.

**Implementation:** Start as `agent-doc runbook audit/install` subcommand. Extract to `agent-runbooks` crate if scope grows.

**Applied:** agent-doc SKILL.md now has `## Runbooks` section.
### Re: Committed and pushed

The agent-runbooks submodule was already committed (in `f7034e1e`). Pushed all commits to origin.

**Decisions recorded from agent-doc-bugs session:**
- Section name: `## Runbooks` (applied to agent-doc SKILL.md)
- Trigger format: `` `trigger phrase` — runbooks/filename.md ``
- Corky extraction candidates: review-inbox, draft-reply, enrich-contact
- Implementation path: start as `agent-doc runbook` subcommand, extract to `agent-runbooks` crate if scope grows

Next step: move the runbooks we created (`.claude/skills/*/runbooks/*.md`) into `src/agent-runbooks/` and update SKILL.md references?

I don't want to do the following because the skill runbooks are installed via libraries such as agent-doc and corky. Why did you suggest `Move runbooks from `.claude/skills/*/runbooks/` into `src/agent-runbooks/`?
> - [ ] Move runbooks from `.claude/skills/*/runbooks/` into `src/agent-runbooks/`
### Re: Why I suggested it (and why you're right not to)

I suggested moving runbooks to `src/agent-runbooks/` thinking of it as a centralized registry. But that's the wrong model.

**Runbooks belong with their skill** because:
1. `agent-doc skill install` installs SKILL.md + runbooks together
2. Skills reference runbooks via relative paths (`runbooks/filename.md`)
3. Moving them to a separate repo would create a cross-repo dependency at install time

The `agent-runbooks` repo/crate is better suited as:
- The implementation of `agent-doc runbook audit/install` subcommand
- A library of shared/reusable runbook templates (not skill-specific ones)
- Tooling for runbook validation and discovery

Without overfitting, how can we prevent or at least make less likely, this category of error?
### Re: Preventing this category of error

The error: **suggesting file moves without checking the distribution mechanism.**

General principle (saved to auto memory):

> Before suggesting file reorganization, check: how are these files delivered to their consumers? If files are installed/distributed as a unit (skill install, cargo publish, npm pack), they must stay co-located. Centralization only works when the consumer can resolve cross-boundary references at runtime.

Lightweight self-check: *"If I move this file, does the install/deploy path still work without adding new dependencies or cross-repo references?"*

This is general enough to apply to:
- Skill runbooks (installed via `agent-doc skill install`)
- Test fixtures (shipped with their package)
- Config templates (deployed with their tool)
- Type definitions (consumed by one package)

Saved as `feedback_distribution_aware_reorg.md` in auto memory.

Where is `feedback_distribution_aware_reorg.md` saved?
### Re: Location of feedback_distribution_aware_reorg.md

```
~/.claude/projects/-home-brian-work-btakita-agent-loop/memory/feedback_distribution_aware_reorg.md
```

This is Claude Code's project-scoped auto memory for agent-loop. The index file `MEMORY.md` in that same directory is loaded at the start of every conversation. The feedback entry is listed under "Architecture Decisions".

Should this auto-memory be a runbook instead? Does automemory inflate context?
### Re: Auto memory vs runbooks

**No, this should stay as auto memory.** Different purposes:

| | Auto Memory | Runbook |
|---|---|---|
| **When loaded** | Conversation start (always) | On-demand (when executing) |
| **Purpose** | Behavioral guardrails | Procedural steps |
| **Context cost** | ~1 line per entry in MEMORY.md index | 0 unless invoked |
| **Example** | "Don't move co-located files" | "How to release to crates.io" |

**Does auto memory inflate context?** Minimally. Only the `MEMORY.md` index loads at conversation start (~96 lines total). Individual memory files are read on-demand when the agent judges them relevant from the index line.

The "distribution-aware reorg" feedback is a guardrail -- it prevents a mistake *before* it happens. A runbook would require the agent to remember to read it before reorganizing, which defeats the purpose.

**Rule of thumb:**
- If it's "don't do X" or "always do Y" -> auto memory (always loaded)
- If it's "step 1, step 2, step 3 to accomplish Z" -> runbook (loaded when doing Z)

I want to have on-demand context as well. Is auto memory essentially on-demand context? Or would we need another mechanism?
Would this feedback_distribution_aware_reorg belong in the on-demand context if auto memory is not on-demand?
### Re: On-demand context vs auto memory (HEAD)

Auto memory is a **hybrid** with two layers:
1. **MEMORY.md index** (always loaded, ~96 lines) — one-line triggers
2. **Individual memory files** (on-demand) — read when agent judges the index line relevant

So `feedback_distribution_aware_reorg.md` content is already on-demand. The ~1 line in MEMORY.md is just the trigger.

**For pure on-demand context** (zero always-loaded cost):

| Mechanism | Trigger Cost | Best For |
|-----------|-------------|----------|
| Auto memory (MEMORY.md) | ~1 line always | Guardrails, preferences |
| Runbooks (skill-local) | 0 (skill ref) | Procedures |
| claude-mem search | 0 | Historical decisions, past work |
| Hooks (settings.json) | 0 | Automated pattern-based checks |

**Should feedback_distribution_aware_reorg be on-demand?** The tradeoff: with zero always-loaded cost, the agent has to *think to search* for it. The 1-line MEMORY.md entry fires proactively before the mistake. That's worth the cost for guardrails.

**Where on-demand context shines:** knowledge useful *when asked about* rather than *before acting*. "How did we solve X last time" = claude-mem. "Don't do X" = auto memory.

If you want a new mechanism beyond these, the gap would be: **context that's triggered by the *type of work* being done, not by explicit user request or agent search.** For example, "when the agent is about to reorganize files, inject distribution-awareness context." That's a hook-based approach — settings.json could match on `Edit` or `Write` tool calls targeting certain paths and inject a reminder. That would be zero always-loaded cost with automatic triggering.
<!-- agent:boundary:088f2a43 -->
<!-- /agent:exchange -->a

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Extract corky runbooks: review-inbox, draft-reply, enrich-contact
- [ ] Explore hook-based on-demand context injection (if desired)
<!-- /agent:pending -->
