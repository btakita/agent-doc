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
### Re: granola.md not committed after transfer

The file has `agent_doc_session: null` and custom content ("Granola Adapter") — this wasn't created by sync's auto-scaffold (which generates a UUID). It appears the transfer was done by the skill (via Write tool) rather than `agent-doc transfer` CLI command.

Same issue as instruction-files.md: when the skill creates a new target file manually instead of using `agent-doc transfer`, the file bypasses the agent-doc pipeline and doesn't get committed.

**Immediate fix:**
```bash
agent-doc init tasks/software/granola.md  # assign UUID + snapshot
agent-doc commit tasks/software/granola.md  # commit to git
```

But `agent-doc init` only works for non-existent files. Since the file exists with content, I'll commit it directly:

Let me commit it now.

Should `agent-doc init` work on created files? What about `agent-doc claim` or `agent-doc sync`?
### Re: Should init/claim/sync handle existing files? (HEAD)

Current behavior:

| Command | Empty file | File with content, no UUID | File with UUID |
|---------|-----------|---------------------------|----------------|
| `init` | Creates new (works) | **Bails** ("file already exists") | Bails |
| `claim` | Scaffolds + UUID (works) | Assigns UUID + scaffolds components (works) | Updates registry (works) |
| `sync` | Auto-scaffolds (v0.31.14) | **Treats as Unmanaged** | Resolves as Registered (works) |

**The gap:** Both `init` and `sync` fail for "file with content but no UUID." This is granola.md's exact scenario — meaningful content, template frontmatter, but `agent_doc_session: null`.

**What should happen:**
- **`init`**: Should work on existing files that have no UUID. Instead of bailing, detect "file exists but has no session UUID" and assign one. The `--force` flag could override for full re-scaffold.
- **`sync`**: Should handle files with frontmatter but no UUID (not just empty files). The condition should be `fm.session.is_none()` regardless of whether content exists.
- **`claim`**: Already handles this correctly.

The fix for `sync` is straightforward: after the empty-file scaffold block, add a check for files with `agent_doc_format` but `agent_doc_session: null` — assign UUID and commit. This is what `ensure_initialized()` already does, but it only runs when `fm.format.is_some()` AND the file is non-empty.

Actually, looking again: `ensure_session_uuid()` (called by `ensure_initialized()`) checks `fm.format.is_none()` and returns early. For granola.md which has `agent_doc_format: template` but `agent_doc_session: null`, `ensure_session_uuid` SHOULD assign a UUID. Let me verify — I don't know why it didn't.

I should investigate this rather than speculate. Want me to dig into why `ensure_session_uuid` didn't fire for granola.md?
<!-- agent:boundary:7637c6cc -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Investigate why ensure_session_uuid didn't assign UUID to granola.md
- [ ] Fix init to work on existing files with no UUID
<!-- /agent:pending -->
