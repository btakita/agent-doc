---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.14 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-05 00:48)

**Releases: agent-doc v0.31.13 + v0.31.14**

**v0.31.13 (diff pipeline):** P1-P5 features (diff-type classification, eval diffs, annotated diff, annotation sidecar, cycle logs), transfer-source metadata, JB plugin Gson migration (v0.2.44), SKILL.md routing + runbooks, directive ontology node.

**v0.31.14 (Binding invariant):** claim provisions new pane when occupied, sync auto-scaffolds empty .md files (template + snapshot + commit), transfer merges pending component, SPEC §7.10 + §8.5 updated, code-enforced-directives runbook.

**Bugs fixed:**
- JB plugin \\n unescape ordering -> Gson migration
- Sync scaffold ordering (before ensure_initialized)
- Sync scaffold commit (snapshot before commit)
- classify_diff --- separator detection (contiguous block)

**Process improvements:**
- Precommit runbook: mandatory testing with categories (negative, boundary, integration)
- Approval != skip processes (CLAUDE.md, SKILL.md, runbook, ontology)
- Code-enforced directives runbook: invariants in binary > tests > SPEC > memory
- Module-harness ontology-references runbook (project-local only)
- Prerelease runbook: ontology + portability checks

**Ontology:**
- directive.md kernel node
- README.md Interaction Model (Directive, Cycle, Diff, Annotation)
- Removed all ~/. claude/philosophy from released files

**Architecture decisions:**
- instruction-files expansion: agent-memories + agent-runbooks specs (task transferred)
- Committed context > machine-local memory

**Test suite: 458 tests, all passing, clippy clean.**

I did a transfer from tasks/recruit/robert-ross-granola.md to tasks/software/granola.md
The file was created with a template. But it was not committed. Why?
### Re: granola.md not committed after transfer (HEAD)

The file has `agent_doc_session: null` and custom content ("Granola Adapter") — this wasn't created by sync's auto-scaffold (which generates a UUID). It appears the transfer was done by the skill (via Write tool) rather than `agent-doc transfer` CLI command.

Same issue as instruction-files.md: when the skill creates a new target file manually instead of using `agent-doc transfer`, the file bypasses the agent-doc pipeline and doesn't get committed.

**Immediate fix:**
```bash
agent-doc init tasks/software/granola.md  # assign UUID + snapshot
agent-doc commit tasks/software/granola.md  # commit to git
```

But `agent-doc init` only works for non-existent files. Since the file exists with content, I'll commit it directly:

Let me commit it now.
<!-- agent:boundary:a32252d4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Commit tasks/software/granola.md
<!-- /agent:pending -->
