---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.13 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 20:41)

**Releases: agent-doc v0.31.6→v0.31.13 + tmux-router v0.3.9 + JB plugin v0.2.44**

**Code shipped:**
- v0.31.10-v0.31.12: ensure_initialized refactor, sync auto-init, cross-process typing (FFI), diff stability, domain ontology
- v0.31.13: diff pipeline (P1-P5), Gson migration, transfer-source metadata, annotation sidecar, cycle logs

**Features (P1-P5):**
- P1: `classify_diff()` — 7 diff types (Approval, SimpleQuestion, BoundaryArtifact, Annotation, StructuralChange, MultiTopic, ContentAddition). Wired into preflight JSON. 13 tests.
- P2: `strip_comments` moved to `component.rs` (shared by binary + eval-runner). Eval diffs match production.
- P3: `annotate_diff()` — `[agent]`/`[user+]`/`[user-]`/`[user~]` format in preflight JSON. 5 tests.
- P4: `agent-doc annotate` — content-source annotation sidecar at `.agent-doc/annotations/<hash>.json`. Cache with SHA256. GC integration. 6 tests.
- P5: `cycles.jsonl` — structured JSONL with git state references (commit_hash + snapshot_hash). Wired into all write paths + commit. 2 tests.
- Transfer-source: `PatchBlock.attrs` field for `<!-- patch:name key=value -->` attributes. 3 tests.

**Bug fixes:**
- JB plugin `\\n` unescape ordering bug -> replaced hand-rolled JSON parser with Gson (v0.2.44)

**SKILL.md + runbooks:**
- Diff-type routing (0b) + runbook
- `---` multi-topic prompt separators (0c) + detection bug fix
- Process discipline: "approval != skip processes" clarification
- Module-harness: ontology-references runbook (project-local paths only)

**Ontology:**
- `directive.md` kernel node (signal that authorizes action, brevity independent of quality expectation)
- README.md Interaction Model section (Directive, Cycle, Diff, Annotation)
- Portability: removed all `~/.claude/philosophy` references from released files

**Process lessons:**
- Precommit runbook step 2 (testing) made mandatory with explicit categories
- "do"/"go" = execute with full rigor, not move fast and skip steps
- Feedback memories: `feedback_precommit_testing.md`, `feedback_approval_not_skip_processes.md`

**Test suite: 450 tests, all passing, clippy clean.**

I created tasks/software/proof.md. I'm unable to get the agent-doc element layout.
I tried `Claim for Tmux Pane` but got an error. Why?
```
Claim failed (exit 1): Error: pane %491 is already claimed by 1ef5f0e7 (file: tasks/agent-doc/agent-doc-bugs.md). Use --force to overwrite.
```

I tried `Sync Tmux Layout` and saw the JB prompt `Sync: --col tasks/agent-doc/agent-doc-bugs.md --col tasks/software/proof.md [focus: tasks/software/proof.md]`.

But none of these created the layout.
### Re: Layout issues with proof.md (HEAD)

Two issues:

**1. Claim error** — "pane %491 is already claimed" means you tried to claim the *current* pane (which runs this bugs session) for a different document. Each document needs its own pane. Claim is for assigning an *existing unused* pane to a document.

**2. File is empty** — `tasks/software/proof.md` is 0 bytes. agent-doc needs frontmatter to recognize a file as a session document:

```yaml
---
agent_doc_format: template
agent_doc_write: crdt
---
```

Without frontmatter, sync sees it as a plain file and won't provision a pane.

**Fix:** Either:
- Run `agent-doc init tasks/software/proof.md` to scaffold frontmatter + components
- Or add frontmatter manually, then run `/agent-doc tasks/software/proof.md` from a new Claude session — sync will auto-provision a pane

The sync output you saw (`--col bugs.md --col proof.md`) was correct — it detected both files in the editor split. But since proof.md had no session UUID, sync couldn't provision a pane for it. Once proof.md has frontmatter, sync will create the 2-column layout automatically on the next tab switch.
<!-- agent:boundary:5da5326a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete. v0.31.13 released.
<!-- /agent:pending -->
