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
<!-- agent:boundary:5fd48f69 -->
<!-- agent:boundary:36574d98 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [/] [#y2gr] Fix agent-doc skill install: does not sync runbooks/*.md to ~/.claude/skills/agent-doc/runbooks/ or <project>/.claude/skills/agent-doc/runbooks/ — only SKILL.md propagates. Had to cp manually today.
<!-- /agent:pending -->
## Assistant
