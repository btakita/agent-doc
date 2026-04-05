---
agent_doc_session: null
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Task created. Scope defined.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Scope: instruction-files expansion

**Goal:** Expand instruction-files from a narrow AGENTS.md/CLAUDE.md auditor into a comprehensive instruction file management crate that covers all development support files.

**Current state:** Discovers AGENTS.md, README.md, SPEC.md, CLAUDE.md, SKILL.md. Audits for staleness, line budget, actionable content.

**Target state:**

1. **Discovery scope expansion:**
   - `.agent/runbooks/*.md` (precommit, prerelease, etc.)
   - `.claude/skills/**/runbooks/*.md` (skill-specific runbooks)
   - Module harness doc comments (via module-harness dependency)

2. **Bundled default runbooks:**
   - `precommit.md` — standard precommit checklist
   - `prerelease.md` — standard prerelease checklist
   - Projects using instruction-files get these automatically

3. **Dependencies:**
   - `existence-lang` crate — ontology term validation
   - `module-harness` crate — module spec auditing

4. **Context invariant:**
   - All referenced context must be in the package's repo or a declared dependency's repo
   - No machine-local paths (`~/`, absolute paths outside repo)
   - Audit validates this invariant

5. **Shared skill:**
   - `/instruction-files audit` — run full audit
   - Runbook: `runbooks/instruction-files-audit.md`

**Transfer from agent-doc-bugs session (2026-04-04):**
- Precommit/prerelease runbooks created and refined
- Ontology references runbook (project-local only)
- Code-enforced directives runbook
- Portability invariant: no `~/` paths in released files
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Add `.agent/runbooks/*.md` to discovery glob patterns
- [ ] Bundle default precommit.md and prerelease.md
- [ ] Add existence-lang dependency for ontology validation
- [ ] Add module-harness dependency for spec auditing
- [ ] Add context invariant audit (no machine-local paths)
- [ ] Create shared skill + runbook
<!-- /agent:pending -->
