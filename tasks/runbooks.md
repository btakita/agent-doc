---
agent_doc_session: 0f91fe16-548b-403c-9a53-79ebe1a9ed79
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Session Summary

**Runbook extraction session** (2026-04-04)

### Completed
- Extracted 14 runbooks from 4 SKILL.md files (dev, email, livestream, module-harness)
- Token savings: ~2,235t (34%) per skill invocation
- Created 3 corky/email runbooks: review-inbox, draft-reply, enrich-contact
- Added btakita/agent-runbooks submodule at src/agent-runbooks/
- Implemented agent-doc notify subcommand (src/notify.rs, 8 tests passing, uncommitted)
- Saved feedback_distribution_aware_reorg.md to auto memory

### Key Decisions
- Runbooks co-locate with skills (distribution-aware, not centralized)
- Auto memory = guardrails (always-loaded); runbooks = procedures (on-demand)
- Notify format: blockquote with --source and --affects flags
- No contexts/ directory; scoped domains in .agent directory instead

### Transferred to agent-doc-bugs
- Review, commit, and release agent-doc with notify subcommand

Is there any deterministic cli functionality that should be added to agent-runbooks?
### Re: Deterministic CLI for agent-runbooks

Yes -- four commands that belong in the binary:

| Command | Purpose | Priority |
|---------|---------|----------|
| `audit <skill-dir>` | Validate SKILL.md runbook refs exist, find orphans | High |
| `list <skill-dir>` | List runbooks with metadata (name, trigger, lines) | Medium |
| `validate <file>` | Check runbook format (title, steps, refs) | Medium |
| `install <skill-dir> --from <template>` | Copy template into skill's runbooks/ | Low |

`audit` is highest priority -- it catches drift between SKILL.md references and actual runbook files. Could integrate with `make precommit` to prevent broken references.

All are deterministic (fixed inputs -> fixed outputs) and CI-runnable. Could start as `agent-doc runbook audit/list/validate` subcommands, then extract to standalone `agent-runbooks` crate if scope grows.

Should we make the project directory configurable? It will default to `.agent/runbooks`. We can't guarantee that `.agent/` is present.
### Re: Configurable project directory

Yes, configurable. Resolution chain:

1. `--dir <path>` CLI flag (explicit)
2. Walk up from CWD for `.agent/runbooks/` (project-local)
3. Walk up for `.claude/skills/*/runbooks/` (skill-local, current pattern)
4. Fallback: error

This way projects without `.agent/` still work via the `.claude/skills/` path. A `--dir` flag handles edge cases. Config file (`[runbooks] dir = "..."` in `.agent/config.toml`) can come later if needed.

For v1: walk-up search + `--dir` override. No config file needed yet.

Implement
```
| Command | Purpose | Priority |
|---------|---------|----------|
| `audit <skill-dir>` | Validate SKILL.md runbook refs exist, find orphans | High |
| `list <skill-dir>` | List runbooks with metadata (name, trigger, lines) | Medium |
| `validate <file>` | Check runbook format (title, steps, refs) | Medium |
| `install <skill-dir> --from <template>` | Copy template into skill's runbooks/ | Low |
```
### Re: Implementing agent-runbooks CLI

Launching worktree agent to implement all 4 commands in `src/agent-runbooks/`. This is a new Rust binary crate.
### Re: agent-runbooks implemented

All 4 commands implemented in `src/agent-runbooks/` (Rust binary crate). 15 tests passing.

**Commands:**
- `audit <skill-dir>` -- cross-references SKILL.md runbook refs against actual files, reports missing/orphans
- `list <skill-dir>` -- lists runbooks with title, line count, ref status (`--json` for machine output)
- `validate <file>` -- checks format (title, steps section, numbered steps, broken links)
- `install <skill-dir> --from <template>` -- copies template, optional `--add-ref` to update SKILL.md

**Live test against our skills:**
```
$ agent-runbooks audit .claude/skills/dev
STATUS     PATH                                     TRIGGER
orphan     runbooks/pre-release-checklist.md        
orphan     runbooks/release-rust-maturin.md         
...  (7 orphans -- SKILL.md uses prose refs not trigger format)

$ agent-runbooks list .claude/skills/agent-doc
FILENAME               TITLE              LINES  REF
compact-exchange.md    Compact Exchange   26     yes
cleanup.md             Cleanup Runbook    50     yes

$ agent-runbooks validate .claude/skills/dev/runbooks/release-rust-maturin.md
OK: no issues found
```

The audit correctly flags orphans because the dev SKILL.md uses prose references (`See runbooks/...`) rather than the trigger format (`` `phrase` -- runbooks/file.md ``). The SKILL.md files should be updated to use the trigger format for full audit coverage.

Not yet committed to the agent-runbooks submodule -- changes in working tree.
do
- [ ] Commit agent-runbooks to submodule
### Re: Committed and pushed

agent-runbooks committed inside the submodule and pushed:
- `c702cd9` -- feat: implement audit, list, validate, install CLI commands (1,263 lines, 15 tests)
- Parent pointer updated in agent-loop: `ad9e6c35`

do
- [ ] Update SKILL.md files to use trigger format for audit coverage
- Add module-harness
- Add ontology
### Re: SKILL.md trigger format updated -- all audits pass (HEAD)

Updated 4 SKILL.md files with `## Runbooks` sections using trigger format:

```
=== agent-doc === (already had it)
ok  runbooks/compact-exchange.md    compact exchange
ok  runbooks/cleanup.md             cleanup

=== dev === (7 runbooks)
ok  runbooks/pre-release-checklist.md    pre-release checklist
ok  runbooks/release-rust-maturin.md     release rust+maturin
ok  runbooks/release-rust-library.md     release rust library
ok  runbooks/release-python.md           release python
ok  runbooks/release-js-deploy.md        release js/ts
ok  runbooks/unpublished-tag-recovery.md unpublished tag recovery
ok  runbooks/submodule-commit.md         submodule commit

=== email === (4 runbooks)
ok  runbooks/email-send.md       send email
ok  runbooks/review-inbox.md     review inbox
ok  runbooks/draft-reply.md      draft reply
ok  runbooks/enrich-contact.md   enrich contact

=== livestream === (1 runbook)
ok  runbooks/livestream-audit.md  audit secrets

=== module-harness === (1 runbook)
ok  runbooks/module-harness-audit.md  audit harness
```

**15 runbooks, 5 skills, all passing audit.** Zero missing, zero orphans.

**Re: ontology (existence skill):** The existence skill currently has no runbooks. The operations it supports (lookup, search, new, lint, scope) are all single commands -- no multi-step procedures that would benefit from runbook extraction. If ontology authoring workflows become more complex, runbooks could be added later.
<!-- agent:boundary:aedeb146 -->
<!-- /agent:exchange -->a

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete. 5 skills audited, 15 runbooks referenced, zero drift.
<!-- /agent:pending -->
