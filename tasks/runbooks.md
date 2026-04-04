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
### Re: Extractable runbook procedures (HEAD)

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
<!-- agent:boundary:211d395c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
