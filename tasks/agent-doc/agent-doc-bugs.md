---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**v0.33.x bug sessions — 2026-04-19**

**Bugs fixed in 0.33.2:**
- Stream replace truncation: --stream path now uses append
- Enter/q stdin not submitted after session restart: supervisor stdin wiring fixed
- Submodule path resolution: JB/VS Code plugins resolve paths relative to agent-doc project root
- is_busy guard blocked tmux-send (0.2.61)
- IPC auto-degradation (#d2d4): 500ms→1000ms timeout, repeated failures degrade to disk
- IPC timeout pending mutation: upgraded to CRDT merge in timeout path

**Bugs fixed in 0.33.3:**
- Orphan recovery dedup: fingerprint_lines prevents duplicate patchback
- IPC fallback patch file race (write.rs): pre-write fallback before socket send; sidecar timeout → success:false → disk fallback
- IDE buffer overwrite (#s80z): reposition handler was missing reloadFromDisk before repositioning

**Bugs fixed in 0.33.4:**
- PromptPoller.autoSaveTrackedFiles() (#mpjv): stale buffer overwrote CRDT-merged content 51s post-commit. Fixed with VFS refresh + modstamp check before auto-save. BulkFileListener added for sub-1.5s reactive merge
- Resync kills active agents (#2ea0): WrongSession handler now join-pane relocates active AGENT_PROCESSES instead of killing
- PluginLifecycleListener --session flag (#8ndp): passes current tmux session to resync so all panes get relocation mode
- Config.toml auto-update: resolve_target_session rewrites tmux_session when configured session is stale

**Bugs fixed in 0.33.5:**
- SIGSEGV PC=0x0 on cargo install (#mpjv IDEA crash): cargo install overwrote mmap'd .so. Fixed with: (1) mtime-based FFI reload in AgentDocLib.get(), (2) versioned cdylib install (libagent_doc-<version>.so + libagent_doc.so symlink via atomic rename), (3) lockfile-tracked GC (agent-doc gc-libs, JVM shutdown hook, VS Code deactivate())

**Decisions:**
- IPC timeout → CRDT merge (not atomic_write) — preserves all concurrent changes
- mtime-based FFI reload + versioned cdylib + lockfile GC as .so overwrite fix (not pure Option 3 nor pure Option 4 — hybrid)
- Promotion heuristic: agent proposals at end of response → --pending-add same cycle (added to pending-ops.md)
- skill install does NOT sync runbooks/ — only SKILL.md propagates. #y2gr filed
- No local maturin publish — PyPI flows through release:published CI automatically
- Config.toml auto-updates tmux_session when configured session dies
- Build on every version bump — enforced

**v0.33.5 status at compact:**
- Pre-release checks: 1158 tests pass, release binary built, cargo install done, JB plugin 0.2.67 built
- **At manual testing gate — NOT yet published to crates.io or GitHub**
- Required: verify IDEA does not crash when cargo install runs during active session; new lib version logged

❯ In src/session-share/tasks/claudescore.md, I'm getting a prompt prefix for the response patchback. See `❯ |-------------|--------|----------|--------|-------|` and `❯ | tree-sitter | 76.0   | **76.0** | 80.0   | 0     |`
❯ Why?
❯ ### Re: normalize_user_prompts prefixing agent response table rows — opus-4-6 (HEAD)

**Root cause: snapshot lag + naive diff heuristic.**

`normalize_user_prompts_in_exchange` (write.rs:351) diffs `snapshot → baseline` and marks `Insert` lines as "user-added." When the snapshot lags behind the file (from any of the IPC bugs we've been fixing: sidecar timeout, stale buffer overwrite, fallback race), previous agent responses appear as "Insert" in the diff. Table rows from those responses then match lines in `content_user_region` and get the prompt prefix.

**Evidence trail:**
- claudescore.md has had prompt-prefixed table rows since its first commit (14 lines). Count grew progressively: 14 → 39 → 45 → 55 as each cycle accumulated more misclassified rows.
- Compacts at commits 87df754/dac6658 briefly cleared the damage (0 prefixed table rows), but it re-accumulated in subsequent cycles.
- The prefixed rows are all inside `### Re:` response blocks — clearly agent content, not user input.

**Why the safety rail didn't catch it:** `MAX_NORMALIZE_USER_LINES = 50` only trips when 50+ lines would be prefixed in a single cycle. Each cycle only adds a few table rows (the delta between stale snapshot and current file), so the threshold is never hit.

**The algorithm's blind spot:** it has exclusions for blank lines, code fences, `<!-- -->` comments, and lines already starting with the prompt prefix, but no concept of "agent response blocks." Lines inside `### Re:` headers are indistinguishable from user content by the current heuristic.

**Fix options:**

1. **Agent-response-block tracking** — in the snapshot-to-baseline diff, skip lines that fall inside a `### Re:` block (from heading to next heading or prompt-prefix line). Most robust; directly targets the semantic gap.

2. **Exclude markdown table rows** (`|...|` pattern) — simple regex filter. Quick fix but users could legitimately type tables.

❯ Can we use a streaming AST instead?

3. **Cross-reference the response patches** — when normalize runs, the agent response content is available (in the patches passed to apply_patches). Subtract those lines from `user_added`. Requires threading the patch content through to normalize.

4. **Boundary history awareness** — instead of one boundary, track all historical boundary positions in the snapshot. Content between consecutive boundaries is agent content from prior cycles, so exclude from `user_added`.

**Recommendation:** Option 1 (agent-response-block tracking) as the primary fix. Low risk: `### Re:` headings are a structural convention already enforced by the SKILL.md. Lines between `### Re:` and the next `### Re:`/prompt-prefix/boundary are agent content by definition.

❯ Option 1 looks good. Please address my prompt about using a streaming AST on Option 2. Is Option 1 still recommended? Make sure we don't hardcode h3 `###`. Are agent-doc responses **always** h3? We should probably not hard-code anyways.

**Immediate mitigation for claudescore.md:** strip the existing prompt prefixes from agent response table rows in the committed content. A sed pass or targeted git filter would clean up the 55 affected lines.

❯ No need
### Re: streaming AST, heading levels, and final recommendation — opus-4-6

#### Streaming AST for Option 2

A streaming markdown AST (tree-sitter-markdown) would give us proper structural awareness — we'd know whether a `|` line is a table row vs. inside a code block vs. a list item. But it's overkill for this problem:

- **We already track code fence state** as a mini state machine (backtick/tilde open/close). Extending that to "inside agent response block" is ~10 lines, not a new dependency.
- **tree-sitter-markdown adds a C dependency** (tree-sitter runtime + grammar). agent-doc is pure Rust today. The normalize function runs on every write — adding a parse step that allocates a full AST per call is heavier than a line-by-line state machine.
- **The AST doesn't solve the root problem.** Even with a perfect AST, we'd still need to distinguish "user-added table row" from "agent-added table row." The AST tells us it's a table — it doesn't tell us who wrote it. The fix is about authorship tracking, not syntax recognition.

If agent-doc ever needs full markdown awareness for other features (section-level diffing, smart compaction, outline extraction beyond the current regex-based approach), tree-sitter-markdown would be worth adding then. For this specific bug, it's not the right tool.

#### Heading levels — no hardcoding

Agent-doc responses are NOT always h3:
- **Template mode:** `### Re:` by convention, but h4-h6 for sub-sections within responses
- **Inline mode:** `## Assistant` (h2)
- **Future/custom:** nothing prevents other levels

The fix should be **heading-level agnostic.** Detect agent response regions by:
- **Start:** any markdown heading (`#{1,6} `) that is NOT prefixed with `❯` and is inside the exchange
- **End:** next `❯`-prefixed line, next heading at same or higher level, or boundary marker

This is the same pattern as the existing code-fence state machine — just tracking heading context instead of fence delimiters. No string matching on "Re:" or "###" needed. The structural insight: in the exchange, unprefixed headings are agent response markers. Users write inline text, blockquotes, and annotations — not standalone headings.

#### Final recommendation

**Option 1 remains recommended**, with the heading-level-agnostic approach above. Implementation sketch in `normalize_user_prompts_in_exchange`:

```rust
// Track whether we're inside an agent response block.
// Agent responses start at headings (not ❯-prefixed) and end at the next
// ❯-prefixed line, boundary marker, or heading at same/higher level.
let mut in_agent_block = false;
let mut agent_heading_level = 0usize;

// In the diff loop (building user_added):
if change.tag() == ChangeTag::Insert {
    let trimmed = line.trim();
    if let Some(level) = heading_level(trimmed) {
        if !trimmed.starts_with('❯') {
            in_agent_block = true;
            agent_heading_level = level;
            continue; // heading itself is agent content
        }
    }
    if in_agent_block {
        if trimmed.starts_with('❯') || trimmed.starts_with("<!--") {
            in_agent_block = false;
        } else {
            continue; // skip — this line is inside an agent response
        }
    }
    // existing filters: blank, fence, already-prefixed...
    user_added.insert(line.to_string());
}
```

Where `heading_level(s)` returns `Some(n)` if the line starts with 1-6 `#` chars followed by a space, else `None`.

This catches all the table rows from the claudescore bug (they're inside `### Re:` blocks) without any hardcoded heading levels or `Re:` text matching.
❯ do Option 1. Add tests
### Re: Option 1 implemented — agent-response-block tracking — opus-4-6

**What changed in `normalize_user_prompts_in_exchange`:**

Added a `heading_level` helper and agent-response-block state machine to the diff loop. The logic:

- **Insert heading** (not `❯`-prefixed, not inside code fence): starts agent block — heading and all subsequent Insert lines are excluded from `user_added`
- **Equal heading**: ends agent block — lines after a heading already in the snapshot are treated as potential user input
- **`❯` or `<!-- -->` line**: ends agent block

This is heading-level agnostic — works for h1-h6, no hardcoded `###` or `Re:` matching. Headings inside code fences are ignored (gated on `!in_baseline_fence`).

**Tests added (5 new, 1 updated):**

| Test | What it covers |
|------|---------------|
| `agent_table_rows_not_prefixed` | Core bug: stale snapshot + table rows inside `### Re:` block |
| `agent_subheadings_not_prefixed` | `### Re:` + `#### Details` — all agent content |
| `user_text_after_equal_heading` | Heading in snapshot (Equal), user text after it gets `❯` |
| `agent_block_ends_at_prompt` | Agent block terminates at `❯` line, user text resumes |
| `heading_in_fence_not_agent_block` | Heading inside code fence doesn't trigger agent-block tracking |
| `heading_treated_as_agent_content` | Updated: standalone headings are agent markers, not user input |

**All 1163 tests pass.** Both binaries rebuilt (`cargo install --path .` + `cargo build --release`).

**Behavioral change:** standalone headings in the exchange (not `❯`-prefixed) are now treated as agent response markers. Users who type raw headings as input won't get the `❯` prefix — but this is the documented convention ("users write inline text, blockquotes, and annotations — not standalone headings").
I `agent-doc on src/session-share/tasks/claudescore.md` ignored my inline prompts. Why?
See
> This is wrong. Do not lower expert scores. We will recalibrate later. The scorer seems wrong. Remember that superpowers is a library that is installed into a harness. Please explain the rationele of 5 or 3. I'm inclined to think the score should be 8-9.

> We may need to broaden the gate to update the definition of the rubric.
### Re: why claudescore inline prompts were ignored — opus-4-6

Traced the git history. Here's what happened:

**Timeline:**
- 17:34 — agent wrote `### Re: #jmh4` response (commit 0ab62e2)
- ~17:35–17:51 — you added inline corrections (raw text between numbered items)
- 17:51–17:52 — two preflight commits (63e4603, 0efe3e0) with boundary/normalize cleanup but **no new agent response**

**Root cause: the Claude session for claudescore.md was no longer active.** The preflight's commit step captured your corrections into the snapshot, but no agent response cycle executed. Evidence:
1. No new `### Re:` block after the corrections — just normalization commits
2. The corrections lack `❯` prefix — no `agent-doc write` cycle ran to normalize them
3. 17-minute gap between agent response (17:34) and next commits (17:51), suggesting the session ended

**Secondary concern — structural detection:** Even if the session HAD been active, raw text inline corrections inside an agent response block have no structural markers (`❯`, `>`, heading). The annotated diff would tag them as `[user+]`, but the model might deprioritize them vs. other changes.

How do we prevent the model from deprioritizing inline prompts?

**Recommendation:** Use blockquote `>` syntax for inline corrections inside agent responses. This makes them structurally distinct from agent content and ensures the agent recognizes them as annotations requiring a response, regardless of session state at the time of editing.
### Re: preventing model deprioritization of inline prompts — opus-4-6

The problem: `[user+]` lines buried among many `[agent]` lines in the annotated diff get lost in the noise. The model sees a wall of agent context with a few user additions mixed in. Three layers of defense:

#### 1. Binary-side: surface annotations in preflight JSON

Add an `inline_annotations` field to the preflight output that extracts user additions appearing between agent lines:

```json
{
  "inline_annotations": [
    "This is wrong. Do not lower expert scores...",
    "We may need to broaden the gate..."
  ],
  "has_inline_annotations": true
}
```

This makes annotations impossible to miss — they're in structured JSON, not buried in the diff. The preflight already knows which lines are `[user+]` vs `[agent]`, so the detection is straightforward: any `[user+]` line that appears between two `[agent]` lines (or between an `[agent]` line and the exchange boundary) is an inline annotation.

#### 2. Skill-side: priority instruction

Add to SKILL.md respond step:

> If the preflight output contains `inline_annotations`, address each one before generating new content. These are user corrections or questions inserted inside previous agent responses — they require direct response.

Even without the binary enhancement, the skill could instruct: "After reading the annotated diff, scan for `[user+]` lines between `[agent]` lines. Treat these as high-priority corrections."

#### 3. User convention: blockquote `>` syntax

Blockquotes get `[user+] >` tags in the annotated diff — structurally distinct from agent prose. This is the belt-and-suspenders layer that works regardless of binary/skill changes.

**Recommendation:** implement #1 (binary) + #2 (skill) as a tracked pending item. #3 is already the documented convention.
❯ Implement #1 + #2. Add tests
### Re: inline annotation surfacing — #1 binary + #2 skill implemented — opus-4-6

#### #1 Binary: `inline_annotations` in preflight JSON

Added `extract_inline_annotations()` to `diff.rs`. Algorithm: scan annotated diff for `[user+]`/`[user~]` lines that have at least one `[agent]` line after them. These are user edits sandwiched inside agent response blocks — not new input at the end.

New field in `PreflightOutput`:
```json
{
  "inline_annotations": [
    "This is wrong. Do not lower expert scores.",
    "We may need to broaden the gate."
  ]
}
```

Omitted from JSON when empty (consistent with other Vec fields).

#### #2 Skill: priority instruction in SKILL.md

Added to the Respond step:

> **Inline annotation priority:** if the preflight output contains `inline_annotations` (non-empty), address each one before generating new content.

#### Tests added (9 new)

| Test | Location | What it covers |
|------|----------|---------------|
| `inline_annotations_user_addition_between_agent_lines` | diff.rs | Core case: `[user+]` between `[agent]` lines |
| `inline_annotations_user_modification_between_agent_lines` | diff.rs | `[user~]` between `[agent]` lines |
| `inline_annotations_user_addition_at_end_is_not_inline` | diff.rs | `[user+]` at end = regular input, not annotation |
| `inline_annotations_multiple` | diff.rs | Multiple annotations, end input excluded |
| `inline_annotations_skips_blank_user_lines` | diff.rs | Blank user lines ignored |
| `inline_annotations_empty_annotated_diff` | diff.rs | Empty input returns empty |
| `inline_annotations_claudescore_table_scenario` | diff.rs | Reproduces the original claudescore bug pattern |
| `preflight_output_includes_inline_annotations` | preflight.rs | JSON serialization with annotations |
| `preflight_output_omits_inline_annotations_when_empty` | preflight.rs | JSON omits empty field |

**All 1172 tests pass.** Both binaries rebuilt (`cargo install --path .` + `cargo build --release`).
❯ bump version and install for testing
### Re: 0.33.6 bump + inline_annotations false positives — sonnet-4-6

Bumped to **0.33.6**, VERSIONS.md updated, both binaries installed.

#### False positives in inline_annotations this cycle

Two items appeared in `inline_annotations` that weren't real user annotations:

1. **`### Re: ... (HEAD)`** — classified as `[user~]` because `agent-doc commit` adds `(HEAD)` to the heading. Has `[agent]` lines after it → falsely flagged.

2. **`bump version and install for testing`** — the real user prompt, but it was followed by `[agent] &lt;!-- /agent:exchange --&gt;` and `[agent] ## Pending / Not Built` structural markers, so the algorithm counted those as "agent lines after" and included it.

#### Fix needed

Two exclusion rules would eliminate both false positives:

1. **Exclude `(HEAD)` boundary marker modifications** — `[user~]` lines where the only change is appending ` (HEAD)` to a heading should be excluded (these are binary reposition artifacts, not user edits).

2. **Exclude component/structural markers from the "agent lines after" check** — lines like `<!-- /agent:... -->`, `<!-- agent:... -->`, and `## ` section headers outside the exchange shouldn't count as "agent content" for the inline annotation heuristic. Only `[agent]` lines that are response prose should qualify.

Alternatively: gate the check more conservatively — require at least one `[agent]` line after that is NOT a component marker or section boundary.

❯ The fix did not seem to work. See claudescore.md

❯ > do
❯ > > This is wrong. Do not lower expert scores. We will recalibrate later. The scorer seems wrong. Remember that superpowers is a library that is installed into a harness. Please explain the rationele of 5 or 3. I'm inclined to think the score should be 8-9.

❯ > do
❯ > > We may need to broaden the gate to update the definition of the rubric.
### Re: false positive fix + claudescore.md context — sonnet-4-6

#### Why claudescore.md still shows the ignored corrections

The Run 23 cycle ran **before** 0.33.6 was installed. The 18:32:41 commit to claudescore.md was the user reformatting the inline annotations to blockquote form — no new agent response ran. No agent cycle on claudescore.md has run yet with 0.33.6.

Running `/agent-doc src/session-share/tasks/claudescore.md` now would surface those corrections via `inline_annotations`:
```json
{
  "inline_annotations": [
    "This is wrong. Do not lower expert scores...",
    "We may need to broaden the gate..."
  ]
}
```

#### False positive fix (now in 0.33.6)

Two exclusion rules added to `extract_inline_annotations`:

**1. Component/structural marker exclusion** — `is_substantive_agent_line()` now rejects:
- `<!-- ... -->` component tags
- `# ...` section headers
- blank lines

End-of-exchange user input followed only by `&lt;!-- /agent:exchange --&gt;` + `## Pending / Not Built` + `&lt;!-- agent:pending --&gt;` is now correctly classified as regular input, not inline annotations.

**2. `(HEAD)` boundary artifact exclusion** — `is_head_boundary_artifact()` skips `[user~]` lines where the only change is appending ` (HEAD)` to a heading. These are reposition artifacts from `agent-doc commit`.

**13 tests pass** (4 new for false positive cases), 1175 total. Both binaries rebuilt.

❯ When I change files on either the right editor side (claudescore.md->shelltab-clone.md or shelltab-clone.md->claudescore.md), the claiming tmux pane is swapped from the right side to the left side...which is incorrect. Why?
### Re: tmux pane side-swap on file change — opus-4-6

**Root cause:** `route.rs:675` used `list_window_panes()` which returns panes in **tmux creation order**, not screen position order. When provisioning a right-column pane:

1. `split_before=false` → `.last()` picks the last pane in creation order
2. But that's not necessarily the rightmost pane on screen
3. Splitting from the wrong reference pane → new pane ends up on the wrong side

**Fix:** Replaced `list_window_panes()` with `list_panes_ordered()` at `route.rs:675`. The ordered variant queries `#{pane_left} #{pane_top}` and sorts by screen coordinates, so `.next()` = leftmost and `.last()` = rightmost by visual position.

```diff
-        let window_panes = tmux.list_window_panes(
+        let window_panes = tmux.list_panes_ordered(
             &format!("{}:agent-doc", session_name)
         ).unwrap_or_default();
```

**All 1175 tests pass.** Both binaries rebuilt. Ready to test — switch files on the right editor side and confirm the pane stays right.
Add tests
### Re: tests for pane ordering fix — opus-4-6

Two integration tests added to `route.rs`:

| Test | What it covers |
|------|---------------|
| `list_panes_ordered_returns_screen_position_after_rearrange` | Breaks a pane out and re-joins it to the opposite side. Verifies `list_panes_ordered` returns screen position order (by `pane_left`) even when creation order diverges. |
| `provision_pane_right_col_picks_rightmost_after_rearrange` | Same rearrangement setup, then calls `provision_pane` with a second-column file. Verifies the new pane ends up rightmost — confirms the `list_panes_ordered` fix in the actual provision path. |

The rearrangement technique: `break-pane` + `join-pane -bh` moves a right pane to the left of another, creating the creation-order/screen-order divergence that triggers the original bug.

**1177 tests pass.** Both binaries rebuilt.
build and install for testing
### Re: 0.33.6 ready for testing — sonnet-4-6

Both binaries freshly built and installed:
- `~/.cargo/bin/agent-doc` → `agent-doc 0.33.6`
- `.bin/agent-doc` (release symlink) → rebuilt

Ready to test: switch files on the right editor side and confirm the pane stays right.

When switching the right editor between claudescore.md and shelltab-clone.md...the tmux pane layout is still reversing sides. Why?
### Re: pane swap still happening — second fix in sync.rs rescue path — opus-4-6

The route.rs fix was correct but addressed the wrong code path. The actual reversal happens in the **stash rescue** at `sync.rs:793-795`.

**Root cause:** When a pane is rescued from the stash, the rescue code picks a swap target using `list_window_panes().first()` — which returns the first pane by tmux creation order, not screen position. If the left pane happens to be `.first()` and the rescued pane belongs on the right, `swap-pane` puts the rescued pane on the LEFT (where the swap target was), displacing the left pane to the stash.

**Two fixes applied:**

1. **`list_panes_ordered()`** — replaced `list_window_panes()` with position-aware ordering at `sync.rs:794`.

2. **Column-position-aware target selection** — instead of always picking `.first()`, the rescue now uses `is_first_column(file_path, col_args)` to pick `.first()` (leftmost) for left-column files and `.last()` (rightmost) for right-column files. Made `is_first_column` `pub(crate)` to share between route.rs and sync.rs.

```diff
-let target_panes = tmux.list_window_panes(target_win).unwrap_or_default();
-if let Some(target) = target_panes.first() {
+let target_panes = tmux.list_panes_ordered(target_win).unwrap_or_default();
+let target = if crate::route::is_first_column(file_path, col_args) {
+    target_panes.first()
+} else {
+    target_panes.last()
+};
+if let Some(target) = target {
```

**All 1177 tests pass.** Both binaries rebuilt. Try switching right editor between claudescore.md and shelltab-clone.md.

When I write to shelltab-clone.md, the line I am typing is duplicated via CRDT a bit before where I was typing. Why?
### Re: CRDT line duplication in shelltab-clone.md — opus-4-6

Investigated the CRDT merge pipeline end-to-end. The duplication is most likely caused by the **IPC sidecar ack timeout race** in write.rs, amplified by a **single-line dedup gap** in the CRDT post-merge.

**Root cause chain:**

1. `agent-doc write --stream` delivers the patch via socket IPC (write.rs:1808)
2. Plugin receives and starts applying the patch to the Document API
3. Sidecar ack times out after 1000ms (write.rs:1812-1814) — plugin was slow but DID apply
4. Binary marks IPC degraded, falls through to file IPC or disk write
5. Binary re-reads the file (write.rs:1214) — which now has the plugin's changes (agent response already applied)
6. CRDT merge runs with:
   - `base` = preflight baseline (no user line, no agent response)
   - `ours` = baseline + agent response
   - `theirs` = file on disk = baseline + user line + agent response (plugin applied)
7. Both `base → ours` and `base → theirs` diffs include the agent response as an insertion at the end of the exchange
8. CRDT creates two copies: client_id=1 (ours) before client_id=2 (theirs) — **"a bit before"**
9. `dedup_adjacent_blocks` only catches blocks with ≥2 non-empty lines, so short duplicates slip through

**Why "a bit before":** Yrs orders concurrent inserts at the same position by client_id (lower first). Agent=client_id(1), user=client_id(2). The ours-side copy appears above the theirs-side copy.

**Evidence supporting this theory:**
- shelltab-clone.md has `agent_doc_write: crdt` and `.agent-doc/patches/` exists (IPC active)
- The 22.8K CRDT state file suggests many merge cycles
- The `patch=append` exchange mode means responses append at the end — the exact position where races collide

**Diagnostic:** Check the tmux pane running shelltab-clone.md's session for `[write] sidecar ack timed out` or `[write] IPC degraded` messages. If present, that confirms the race.

**Two fixes needed:**

1. **Guard the disk write on sidecar timeout:** Before CRDT merging on the IPC timeout path, check if `content_current` already contains the agent response. If `content_ours` content appears in `content_current`, skip the merge — the plugin already applied it.

2. **Lower dedup threshold or add content-aware dedup:** Either lower `dedup_adjacent_blocks` to ≥1 non-empty line (risk: false positives on intentionally repeated single lines like `---`), or add a targeted dedup that compares the ours-side patches against the merged result.

Add the fixes to agent:pending. We have a skill for this but the skill is not working.
Implement fix #1 with tests.
### Re: pending + fix #1 implementation — opus-4-6

**Fix #1 implemented:** Added `response_already_in_current()` guard to the IPC timeout path in `write.rs`.

**What it does:** Before CRDT merging on the IPC timeout/exit(75) path, the guard extracts lines that `content_ours` added to the exchange component (relative to `base`). If ≥80% of those lines are already in `content_current`, the plugin has already applied the response — skip the CRDT merge and use `content_current` directly.

**Code changes:**
- `write.rs` — new function `response_already_in_current(base, content_ours, content_current) -> bool`
- `write.rs` — new branch in IPC timeout path (line ~1219): returns `content_current` when plugin applied, avoiding duplicate response via CRDT
- 4 new tests: detects plugin-applied, false when not applied, false when no exchange, false when no changes

**Test results:** 1181 pass, clippy clean. Both binaries rebuilt (`cargo install` + `cargo build --release`).

Two items added to pending. Fix #2 (dedup threshold) is lower priority — fix #1 addresses the root cause; fix #2 is defense-in-depth.

Is #15mw implemented? Implement #3yc9 with tests.
### Re: #15mw status + #3yc9 implementation — opus-4-6

**#15mw is implemented.** The `response_already_in_current()` guard was added in the previous cycle — it's live at `write.rs:1219` with 4 tests covering the function at `write.rs:2443`.

**#3yc9 implemented:** Content-aware dedup for single-line CRDT duplicates.

**What changed in `crdt.rs`:**
- Lowered `dedup_adjacent_blocks` threshold from ≥2 to ≥1 non-empty lines
- Added `is_structural_block()` exemption for thematic break markers (`---`, `***`, `___`) — these may legitimately repeat and are not deduped
- All other single-line content duplicates (headings, text, etc.) are now caught

**New tests (6):**
- `dedup_preserves_structural_thematic_breaks` — `---`, `***`, `___` all exempt
- `dedup_removes_single_line_content_duplicates` — heading-style duplicates caught
- `dedup_removes_single_line_text_duplicates` — prose duplicates caught
- `dedup_preserves_different_single_line_blocks` — non-duplicates untouched
- `is_structural_block_detects_thematic_breaks` — unit tests for the helper
- Existing multi-line dedup tests unchanged and passing

**Test results:** 1185 pass, clippy clean. Both binaries rebuilt (`cargo install` + `cargo build --release`).
src/session-share/tasks/shelltab-clone.md duplicated my currently edited prompt. Why? See `Create a project plan document in tasks/shelltab-clone/*.md to the demo. T`
### Re: shelltab-clone.md prompt duplication — opus-4-6

**Root cause: TOCTOU race in boundary reposition during concurrent user edits.**

The shelltab-clone.md working tree shows a classic read-modify-write race:

```
...Start with BYOK — it's the right MVP choice.
Create a project plan document in tasks/shelltab-clone/*.md to the demo. T   ← partial capture
<!-- agent:boundary:5ba2df36 -->
&lt;!-- /agent:exchange --&gt;
## Pending / Not Built
&lt;!-- agent:pending patch=replace --&gt;
&lt;!-- /agent:pending --&gt;
Create a project plan document in tasks/shelltab-clone/*.md to the demo. Then from demo to production.  ← full input
<!-- agent:boundary:5ba2df36 -->
&lt;!-- /agent:exchange --&gt;
...  (structural tail duplicated)
```

#### How the race works

`reposition_boundary_in_snapshot()` in `git.rs:681-698` does a disk-level read-modify-write on the working tree:

```rust
let working = std::fs::read_to_string(file)?;        // T1: reads partial user input
let repositioned = reposition_boundary_to_end(...);    // strips old boundaries, inserts new one
atomic_write_pub(file, &repositioned)?;                // T2: writes back — truncates user input
```

Then `try_ipc_reposition_boundary(file)` sends a signal to the IDE, which does a second reposition via Document API. Between T1 and T2 (or between disk write and IDE refresh), the user typed more characters. The result: two copies of the structural tail with different user content prefixes.

#### Why two boundary markers in the committed state

The committed state had `<!-- agent:boundary:5bb175d8 -->` AND `<!-- agent:boundary:d9d8d6b6 -->` — two markers. This is itself a pre-existing bug: `reposition_boundary_to_end` should produce exactly one, but something left a stale one behind.

#### Fix direction

1. **Skip working-tree reposition when IPC is available** — if `.agent-doc/patches/` exists (IDE plugin installed), let the IDE handler own the working-tree boundary. The CLI should only reposition the snapshot (which it controls exclusively). This eliminates the disk-level TOCTOU entirely.

2. **IDE handler CAS guard** — in the JB plugin's `repositionBoundaryViaDocument`, compare `document.text` before `setText(result)` to verify it hasn't changed since the read. If it has, skip — the user is editing.

3. **Skip reposition for documents with user edits** — `reposition_boundary_in_snapshot()` already skips when a pending file exists (active run). Extend this: also skip the working-tree reposition when the file has user content beyond the last boundary marker that differs from the snapshot.

Fix #1 is the simplest and eliminates the most common path. Added to pending.
❯ I want all race condition cases fixed. Would you recommend Fix #2 and/or Fix #3?
### Re: which fixes to implement — opus-4-6

**Fix #1 + Fix #2. Skip Fix #3.**

#### Fix #1 — skip disk reposition when IPC available (git.rs)

Eliminates the most common path. In `reposition_boundary_in_snapshot()` at git.rs:681-698, guard the working-tree reposition:

```rust
// Skip working-tree reposition when IDE plugin is installed —
// the IPC handler owns this path and coordinates with user edits.
let patches_dir = crate::snapshot::agent_doc_dir(file)
    .map(|d| d.join("patches"));
let ipc_available = patches_dir.map_or(false, |d| d.exists());
if !ipc_available {
    // Non-IDE path: reposition on disk (no concurrent editor to race with)
    if let Ok(working) = std::fs::read_to_string(file) { ... }
}
```

The snapshot reposition (lines 649-665) stays unconditional — the CLI owns the snapshot exclusively.

#### Fix #2 — CAS guard in IDE reposition handler (PatchWatcher.kt + extension.ts)

Even with Fix #1, the IDE handler still has a TOCTOU between `document.text` read and `document.setText(result)`. The `await_idle` guard has a 5s timeout that can expire while the user is still typing. Add a CAS check:

```kotlin
WriteCommandAction.runWriteCommandAction(project, "Agent Doc Reposition", null, {
    val content = document.text
    val result = NativePatching.repositionBoundaryToEnd(content)
        ?: repositionBoundaryToEnd(content, "exchange")
        ?: return@runWriteCommandAction
    if (result != content && document.text == content) {  // CAS: verify unchanged
        document.setText(result)
    }
})
```

Same pattern for VS Code: check `document.getText() === originalContent` before applying the edit.

Since both reads and the write run on EDT (JB) or the extension host (VS Code), and those are single-threaded, the CAS is effectively atomic — no keystrokes can arrive between the final check and the write within the same event loop tick.

#### Why skip Fix #3

Fix #3 (snapshot-diff check in the CLI) is redundant with Fix #1. When IPC is available, the CLI doesn't touch the working tree at all. When IPC is unavailable (no IDE plugin), there's no concurrent document API to race with — the user is editing in a plain text editor, and boundary reposition on disk is safe.

Updated pending to combine Fix #1 + Fix #2.
❯ Add Fix #2 to agent:pending
❯ Do #xbs3. Add tests
### Re: #xbs3 Fix #1 implemented — opus-4-6

**Fix #1 done.** Working-tree boundary reposition in `git.rs:668-698` now checks for IPC availability before touching the disk.

**What changed:**
- `git.rs:reposition_boundary_in_snapshot()` — added IPC guard: when `.agent-doc/patches/` exists (IDE plugin installed), skip the working-tree reposition entirely. The IPC reposition signal (sent post-commit) lets the IDE plugin handle the boundary via Document API, avoiding the disk-level TOCTOU race.
- `write.rs` — added `resolve_ipc_project_root_pub()` public accessor so git.rs can resolve the project root for the IPC check.
- Snapshot reposition remains unconditional (the CLI owns the snapshot exclusively).

**New tests (2):**
- `reposition_skips_working_tree_when_ipc_available` — with `.agent-doc/patches/` present, working tree retains old boundary while snapshot is repositioned
- `reposition_updates_working_tree_when_no_ipc` — without the patches dir, both snapshot and working tree are repositioned

**Test results:** 1187 pass, clippy clean. Both binaries rebuilt.

Fix #2 (CAS guard in IDE reposition handlers) added as separate pending item.
do #qdb4. add tests
### Re: #qdb4 CAS guard implemented + tests — opus-4-6

**Fix #2 done.** Both IDE reposition handlers now verify `document.text` is unchanged between read and `setText` — if the user typed between `await_idle` timeout and the write action, the reposition is silently skipped.

**JB PatchWatcher.kt:**
- `repositionBoundaryViaDocument()` — added CAS check: `if (result != content && document.text == content)`. Within the `WriteCommandAction`, the second `document.text` read verifies no keystrokes landed between `reloadFromDisk` and the write action dispatch.
- Extracted `repositionBoundaryToEndUtil` and `findCodeBlockRangesUtil` as internal top-level functions for testability. Instance methods delegate.

**VS Code extension.ts:**
- `repositionBoundaryWithDebounce()` — added CAS check: `document.getText() === content` before applying the `WorkspaceEdit`.
- Extracted `repositionBoundaryToEnd` into `reposition.ts` (vscode-free module). PatchWatcher's `repositionBoundaryToEndTs` delegates to it.

**New tests:**
- **Kotlin** `RepositionBoundaryTest.kt` (7 tests): reposition from middle to end, no boundary → null, already at end → null, collapse multiple boundaries, skip code-block fakes, code block range detection, nonexistent component → null
- **TypeScript** `reposition.test.ts` (5 tests): reposition from middle, no boundary → null, collapse multiple, nonexistent component, component with attributes

**Test results:** JB gradle test — BUILD SUCCESSFUL. VS Code npm test — 11 pass, 0 fail. Both plugin test suites green.
<!-- agent:boundary:0e8f1b76 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [/] [#y2gr] Fix agent-doc skill install: does not sync runbooks/*.md to ~/.claude/skills/agent-doc/runbooks/ or <project>/.claude/skills/agent-doc/runbooks/ — only SKILL.md propagates. Had to cp manually today.
<!-- /agent:pending -->
## Assistant
