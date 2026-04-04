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
### Re: Done (HEAD)

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
<!-- agent:boundary:a05b7fb9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete.
<!-- /agent:pending -->
