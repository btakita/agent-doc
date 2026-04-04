---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.12 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 16:29)

**Releases: agent-doc v0.31.6→v0.31.12 + tmux-router v0.3.9**

**Code shipped this session:**
- v0.31.10: ensure_initialized (UUID + snapshot + git), sync auto-init, cross-process typing (FFI), diff stability, IPC errors, granola runbook
- v0.31.11: sync resolve_file auto-init, SPEC.md 8.5 Binding invariant, domain ontology, module docs
- v0.31.12: refactored ensure_initialized into 3 functions (ensure_session_uuid, ensure_snapshot, ensure_git_tracked), renamed auto_start_no_wait → provision_pane, 8 new tests

**Design decisions:**
- Binding invariant: document drives pane resolution (find existing OR provision new, NEVER commandeer)
- Domain ontology: Binding, Reconciliation, Provisioning, Initialization (README.md + module docs)
- Spec-first workflow: SPEC.md before module-harness (avoid codifying bugs as spec)
- Module-harness redesign: default=audit, seed=bootstrap, apply=spec-first (SKILL.md updated)
- **Test suite: 25 snapshot tests (17 existing + 8 new), 645+ total**

There should be a way for the user to break up prompts even within a continuous diff. I'll see if `---` works. Perhaps we can add to the SKILL.md.

---

The agent-doc transfer source document path should be recorded in the patch. So the agent can read the transfer source for additional context if necessary.

---

### Transfer from eval-runner session (2026-04-04)

**Context:** During eval-runner development, several agent-doc feature designs emerged from analyzing eval failures and discussing deterministic preprocessing.

#### 1. Diff-type classification in preflight
Deterministic regex/heuristic classification of user diffs before the LLM sees them:
- `SimpleQuestion` — single added line ending with `?`
- `Approval` — single word matching approval list ("go", "yes", "do")
- `Annotation` — edits within agent content (colon-appended, inline)
- `StructuralChange` — deletions, reorgs, moves
- `MultiTopic` — multiple distinct items
- `BoundaryArtifact` — `(HEAD)`, boundary UUID changes
- `CommentOnly` — already handled (stripped)

Preflight JSON would include `"diff_type": "simple_question"` + `"diff_type_reason"`. Skill routes based on type, skipping discrimination rules for simple cases.

#### 2. Content-source annotation (sidecar approach)
Separate annotation layer at `.agent-doc/annotations/<hash>.json` with character-level source ranges. Markdown stays pristine. Editor reads sidecar for coloring (user=green, agent=neutral).

- Sidecar is a **cache, not state** — always reconstructable from (snapshot + current file)
- `agent-doc annotate <file>` — generates/regenerates the sidecar
- `agent-doc annotate --history <file>` — uses git blame for full history attribution
- Prior art: https://github.com/iainc/Markdown-Annotations (iA Writer)

#### 3. Annotated diff format
Preprocessed diff with source boundaries for the LLM:
```
[agent] 1. Window 0 missing — pane at index 2.
[user+] : Fixed, moved to 0
```
Markers: `[agent]`, `[user+]`, `[user-]`, `[user~]`
Implementation: `agent-doc annotate-diff` command, included in preflight JSON as `annotated_diff`.

#### 4. Reproducible operation logs
Every agent-doc cycle produces a log entry referencing git state (not inline content):
- Commit hash + snapshot hash = enough to reconstruct any cycle
- `agent-doc replay --from <commit>` for deterministic replay
- Logs should be parseable + agent-executable for playback

#### 5. Multi-topic response runbook
Created at `.claude/skills/agent-doc/runbooks/multi-topic-response.md`. Documents detection, response pattern (`### Re:` per topic), checkpoint timing, and when NOT to split.

#### 6. Post-preflight eval diffs
Eval cases should use post-preflight diffs (comments already stripped) to match what the model actually sees in production. Validated: `mixed-valid-and-comment` case went from 0.30 to 0.90 after this fix.
### Re: Prompt separators with `---`

Good idea. `---` (horizontal rule) is a natural markdown separator. The skill could detect `---` in the diff as a topic boundary and treat each section as a separate prompt. Works well with multi-topic response pattern (`### Re:` per section). Worth adding to SKILL.md under "Multi-topic prompts."

One consideration: `---` in YAML frontmatter context means something different. Only treat `---` as a prompt separator in the exchange body, not at the document top.

### Re: Transfer source path in patch

Agreed. Recording the source document path lets the receiving agent read the source for context. Could be a patch metadata field:

```markdown
<!-- patch:exchange transfer-source="tasks/software/eval-runner.md" -->
```

The binary would preserve this in component metadata, and the skill would check for it during preflight.

### Re: Transfer from eval-runner session

Good batch of designs. Priority ordering:

1. **Diff-type classification** — highest leverage. Simplifies skill routing, reduces token waste on simple approvals/questions.
2. **Post-preflight eval diffs** — quick win, already validated (0.30→0.90). Should land with or before diff-type classification.
3. **Annotated diff format** — natural extension of diff-type. `[agent]`/`[user+]` markers give the LLM better change context.
4. **Content-source annotation sidecar** — enables editor coloring. "Cache, not state" design is right. Can land independently.
5. **Reproducible operation logs** — important for debugging but lower urgency than diff pipeline.
6. **Multi-topic response runbook** — already created, just add `---` separator convention.

continue
### Re: Continue (HEAD)

Continuing from the priority ordering. Here's an implementation plan for the top items:

**1. Diff-type classification (P1)**

The implementation path:
- Add a `classify_diff` function in `src/agent-doc/src/diff.rs` (or new `classify.rs` module)
- Regex-based heuristics on the unified diff output:
  - Single `+` line ending with `?` → `SimpleQuestion`
  - Single `+` line matching `/^(go|yes|do|ok|continue|approved?)$/i` → `Approval`
  - Changes within agent content lines (`:` appended, inline edits) → `Annotation`
  - Only `-` lines or reordered hunks → `StructuralChange`
  - Multiple unrelated `+` blocks separated by context → `MultiTopic`
  - Only `(HEAD)` or boundary UUID changes → `BoundaryArtifact`
- Add `"diff_type"` and `"diff_type_reason"` to preflight JSON output
- Skill reads `diff_type` and short-circuits for simple cases (approvals skip discrimination, questions get direct answers)

**2. Post-preflight eval diffs (P2)**

Quick win — the eval harness should run preflight's comment-stripping logic before computing diffs. This way eval diffs match production diffs exactly. Already validated with the mixed-valid-and-comment case (0.30→0.90).

Implementation: extract the comment-stripping logic into a shared function callable from both preflight and the eval harness.

**3. `---` separator convention**

Simple SKILL.md addition. The skill detects `---` on its own line within the exchange diff as a topic boundary. Each section between separators becomes a `### Re:` response block.

Want me to start implementing any of these? I'd suggest starting with diff-type classification since it's highest leverage, or the `---` separator since it's the quickest to land.
<!-- agent:boundary:88302ba9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement diff-type classification in preflight (P1)
- [ ] Post-preflight diff generation for eval cases (P2)
- [ ] Implement annotated diff format in preflight (P3)
- [ ] Implement content-source annotation sidecar (P4)
- [ ] Design reproducible operation log format (P5)
- [ ] Add `---` prompt separator convention to SKILL.md
- [ ] Add transfer-source metadata to patch format
<!-- /agent:pending -->
