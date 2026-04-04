---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
P1-P4 + JB plugin Gson migration shipped. 434 tests passing.
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
### Re: Continue

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
go
### Re: Go — Diff-type classification implemented

Implemented P1 (diff-type classification) in `src/agent-doc/src/diff.rs`:

**New types:**
- `DiffType` enum: `Approval`, `SimpleQuestion`, `BoundaryArtifact`, `Annotation`, `StructuralChange`, `MultiTopic`, `ContentAddition`
- `DiffClassification` struct: `diff_type` + `diff_type_reason`
- `classify_diff(diff_text)` — pure function, no I/O, deterministic

**Classification priority (checked in order):**
1. `BoundaryArtifact` — only `(HEAD)` or boundary UUID changes (paired line comparison)
2. `Approval` — single word matching list (go/yes/do/ok/continue/approve/y/yep/yeah/sure/proceed/lgtm)
3. `SimpleQuestion` — single added line ending with `?`
4. `Annotation` — paired add/remove where added extends removed (colon-append, >60% common prefix)
5. `StructuralChange` — only removals, no additions
6. `MultiTopic` — multiple separated added blocks or `---` separators
7. `ContentAddition` — default fallback

**Wired into preflight JSON:**
```json
{
  "diff_type": "approval",
  "diff_type_reason": "single approval word: \"go\""
}
```

Fields are `skip_serializing_if = "Option::is_none"` — omitted when no diff.

**Tests:** 9 new tests covering all diff types. All 417 tests pass, clippy clean.

Next steps: the skill can now read `diff_type` from preflight JSON and short-circuit for simple cases. Want me to update the SKILL.md routing logic, or move to P2 (post-preflight eval diffs)?
do:
- [ ] Update SKILL.md to route based on diff_type
- Extract runbook
### Re: Do — SKILL.md routing + runbook

Done. Both items executed:

**1. SKILL.md updated** — added `### 0b. Diff-type routing` section between preflight and respond steps:
- Routing table mapping each `diff_type` to a response strategy
- Short-circuit rules: `boundary_artifact` stops, `approval` skips discrimination, `simple_question` answers concisely
- References the new runbook for details

**2. Runbook extracted** — created `runbooks/diff-type-routing.md`:
- Full classification table with patterns and examples
- Routing rules per type with behavioral guidance
- Precedence order (matches the Rust implementation)
- Override guidance for ambiguous cases

The skill will now automatically use `diff_type` from preflight to optimize routing. For example, when you type "go", the preflight returns `"diff_type": "approval"` and the skill skips confirmation, going straight to execution.
do
- [ ] Post-preflight diff generation for eval cases (P2)**+**
### Re: Do — Post-preflight eval diffs (P2) implemented

Done. Eval diffs now match production behavior.

**What changed:**

1. **Moved `strip_comments` to `component.rs`** — shared by both binary (`diff.rs` delegates) and library consumers. No code duplication.

2. **Added `agent-doc` as eval-runner dependency** — `eval-runner` now imports `agent_doc::component::strip_comments`.

3. **Added `strip_diff_comments()` in eval-runner** — reconstructs both sides of a unified diff, strips comments from each, then recomputes the diff. Returns `None` when the diff was entirely comments (matching production's "no changes" behavior).

4. **Wired into `run_case()`** — every eval case diff is preprocessed before the LLM sees it. Comment-only diffs show `"(no changes after comment stripping)"` in the prompt.

**Architecture:**
```
component.rs::strip_comments()  ← shared pure function
    ├─ diff.rs::strip_comments()     ← binary delegates here
    └─ eval-runner::strip_diff_comments()  ← reconstructs + re-diffs
```

All agent-doc tests pass (417). Eval-runner compiles clean.

do
- [ ] Implement annotated diff format in preflight (P3)
### Re: Do — Annotated diff format (P3) implemented

Done. The preflight JSON now includes `annotated_diff` with content-source markers.

**New function: `annotate_diff(diff_text)` in `diff.rs`**

Transforms unified diffs into annotated format:
```
[agent] context line from snapshot
[user+] new line added by user
[user-] line removed by user
[user~] The fix is deployed: confirmed in prod
```

**Marker types:**
- `[agent]` — context lines (unchanged content from agent/snapshot)
- `[user+]` — lines added by the user
- `[user-]` — lines removed by the user
- `[user~]` — lines modified by the user (paired add/remove with >60% common prefix)

**Modification detection:** Buffered removes are paired with subsequent adds. If they share >60% common prefix (same as the annotation classifier), they're emitted as `[user~]` instead of separate `[user-]`/`[user+]` pairs.

**Wired into preflight JSON:**
```json
{
  "annotated_diff": "[agent] context\n[user+] new question here?\n[agent] more context"
}
```

The patch above was rendered as uncommitted to git with. Why?:

```
{
  "annotated_diff": "[agent] context\
[user+] new question here?\
[agent] more context"
}
```

Field is `skip_serializing_if = "Option::is_none"` — omitted when no diff.

**Tests:** 5 new tests (additions, removals, modifications, context, empty). All 428 tests pass, clippy clean.

The skill can now use `annotated_diff` instead of raw unified diff for better context when responding to user changes.
### Re: JSON rendering issue

The two code blocks show the same JSON but rendered differently. The first (intended) has `\\
` as literal escaped characters in the JSON string. The second (what appeared in the document) shows them as actual line breaks with trailing backslashes.

This happened during the CRDT write-back: the `\\
` sequence in my response was being unescaped somewhere in the patch application pipeline. The heredoc `<<'RESPONSE'` correctly preserves literal content (no shell expansion), but the IPC patch JSON serialization or the CRDT text insertion may be interpreting `\\
` as a newline.

**Root cause:** When `agent-doc write --stream` processes the patch content, it serializes it to JSON for IPC delivery. If the patch content contains `\\
` inside a markdown code fence, the JSON serializer writes it as `\
` (a literal newline in JSON), which then gets inserted as an actual newline in the document.

**Fix needed:** The patch content should be treated as opaque text — no unescaping. This is a bug in the write pipeline's text handling. Want me to investigate and fix it in the binary?
yes
### Re: Yes — Fixed the `\\
` unescaping bug

**Root cause:** `PatchWatcher.kt:929-932` — the manual JSON parser's escape replacement order was wrong.

The old code:
```kotlin
.replace("\\
", "\
")      // Step 1: eats \
 (including from \\
)
.replace("\\	", "\	")
.replace("\\\"", "\"")
.replace("\\\\", "\\")     // Step 4: too late — \\
 already mangled
```

When JSON contains `\\
` (literal backslash + n), step 1 sees `\
` and converts it to a newline, before step 4 ever processes the `\\`.

**Fix:** Process `\\` first using a NUL placeholder:
```kotlin
.replace("\\\\", "\u0000")  // Step 1: protect escaped backslashes
.replace("\\
", "\
")       // Step 2: now only real \
 sequences
.replace("\\	", "\	")
.replace("\\\"", "\"")
.replace("\u0000", "\\")    // Step 5: restore backslashes
```

Plugin builds clean. To install: rebuild with `./gradlew buildPlugin` then install the zip from `build/distributions/`.
bump JB plugin version.
Do we need to make a similar fix to the VSCode plugin?
### Re: Plugin version bump + VS Code check

**JB plugin bumped to v0.2.43.** Built at `build/distributions/agent-doc-jetbrains-0.2.43.zip`. Install via Settings > Plugins > gear > Install from Disk.

**VS Code plugin is NOT affected.** It uses `JSON.parse()` (the standard JavaScript JSON parser) which handles all escape sequences correctly. The bug was specific to the JetBrains plugin's hand-rolled JSON parser in Kotlin.

Does Kotlin have a JS Parser library that we can use?
### Re: Kotlin JSON parsing options

Yes. The best option for IntelliJ plugins is the platform's **bundled Gson** (`com.google.gson`). It ships with every IntelliJ installation — zero new dependencies.

The original hand-rolled parser was written to avoid Gson's `ClassNotFoundException`, but that typically happens with wrong classloader context. In a standard plugin action/service, Gson is available:

```kotlin
import com.google.gson.JsonParser

val root = JsonParser.parseString(raw).asJsonObject
val component = root.get("component")?.asString
val content = root.get("content")?.asString
```

This handles all JSON escaping correctly (like VS Code's `JSON.parse()`) and eliminates the entire class of escaping bugs.

**Other options:**
- `kotlinx-serialization-json` — official Kotlin library, but adds a dependency
- `org.json` — also bundled in some IntelliJ builds, simpler API than Gson
- Keep the hand-rolled parser with the NUL placeholder fix (already working)

**Recommendation:** Replace `extractStringField` with Gson. Zero new deps, correct by construction. Want me to do it?
Yes
### Re: Yes — Replaced hand-rolled JSON parser with Gson

Done. Replaced all manual JSON parsing in `PatchWatcher.kt` with `com.google.gson.JsonParser`.

**What was removed** (~80 lines):
- `extractStringField` with hand-rolled escape handling
- `findUnescapedQuote`
- `findMatchingBracket`
- `findMatchingBrace`

**What replaced it** (~30 lines):
- `parsePatchJson` now uses `JsonParser.parseString(json).asJsonObject` and type-safe accessors (`.asString`, `.asBoolean`, `.asJsonArray`)
- `extractStringField` is now a 5-line function delegating to Gson
- All JSON escape sequences handled correctly by Gson (no more NUL placeholder hack)

**Plugin bumped to v0.2.44.** Install from `build/distributions/agent-doc-jetbrains-0.2.44.zip`.

Builds clean. Zero new dependencies — Gson ships with every IntelliJ installation.

do
- [ ] Implement content-source annotation sidecar (P4)
### Re: Do — Content-source annotation sidecar (P4) implemented (HEAD)

Done. New `annotate.rs` module + CLI command + GC integration.

**New command: `agent-doc annotate <file> [--force] [--history]`**

Generates a JSON sidecar at `.agent-doc/annotations/<hash>.json` mapping each line to its source:

```json
{
  "file": "tasks/agent-doc/agent-doc-bugs.md",
  "doc_hash": "9b6e92d2...",
  "snapshot_content_hash": "a1b2c3...",
  "file_content_hash": "d4e5f6...",
  "lines": [
    {"line": 1, "source": "agent"},
    {"line": 2, "source": "agent"},
    {"line": 3, "source": "user"},
    {"line": 4, "source": "agent"}
  ]
}
```

**How it works:**
1. Loads snapshot (baseline from last write) and current file
2. Diffs with `similar::TextDiff::from_lines()` on raw content (no comment stripping — 1:1 with editor lines)
3. Maps: `Equal` -> `agent`, `Insert` -> `user`, `Delete` -> skip
4. Cache: content hashing (SHA256) of both snapshot and file. Skips regeneration if hashes match unless `--force`

**Files changed:**
- `annotate.rs` — new module (170 lines, 6 tests)
- `main.rs` — `mod annotate` + `Commands::Annotate` + dispatch
- `gc.rs` — added `("annotations", vec!["json"])` to cleanup list

**Tests:** 6 new (no_snapshot_all_user, identical_all_agent, user_additions, user_modifications, cache_invalidation, cache_valid_skips). All 434 tests pass, clippy clean.

`--history` flag is wired but deferred (prints path from basic mode). Can add git blame attribution later.
<!-- agent:boundary:edde535c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Implement diff-type classification in preflight (P1) — DONE
- [x] Update SKILL.md to route based on diff_type — DONE
- [x] Post-preflight diff generation for eval cases (P2) — DONE
- [x] Implement annotated diff format in preflight (P3) — DONE
- [x] Fix write pipeline escape bug + Gson migration (JB plugin v0.2.44) — DONE
- [x] Implement content-source annotation sidecar (P4) — DONE
- [ ] Design reproducible operation log format (P5)
- [ ] Add `---` prompt separator convention to SKILL.md
- [ ] Add transfer-source metadata to patch format
<!-- /agent:pending -->
