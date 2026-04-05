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
### Session Summary (compacted 2026-04-04 20:41) (HEAD)

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
<!-- agent:boundary:90b50300 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete. v0.31.13 released.
<!-- /agent:pending -->
