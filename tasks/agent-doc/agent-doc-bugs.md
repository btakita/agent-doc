---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.29 — released
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.31 + IDE comparison. Key decisions and fixes:

**v0.31.18–v0.31.26 (prior):** Cross-session swap-pane bug; slash command dispatch; partial compact; JB dedup guard; IPC idempotent apply (patch_id UUID + ACK-content sidecar); snapshot=disk-state ghost-reappearance fix; v0.31.26 released.

**v0.31.27 — pane_policy + session drift hardening:** PaneMoveOp + CrossSession enum; 22 call sites migrated (deny by default, 3 explicit allow); start.rs + route.rs + resync.rs session guards; v0.31.27 released.

**v0.31.28 — exchange dedup + auto-relocate:** apply_mode(append) echoed user prompt; fix via dedup_exchange_adjacent_lines + strip_leading_overlap; start.rs auto-relocate via PaneMoveOp (warn→move); v0.31.28 released.

**v0.31.29 — write --commit flag:** agent-doc write --commit runs git commit after write (atomically), eliminating the write/commit gap class. git::is_in_git_repo guard. SKILL.md step 2 updated to use --commit on final write; step 3 removed. v0.31.29 released.

**v0.31.30 — normalize_patch_content scope fix + claim position:** normalize_patch_content gated on is_append_mode_component — replace-mode patches (pending, status) no longer receive ❯ prefix. JB plugin v0.2.55: ClaimAction passes --position left|right from detectEditorLayout, fixing wrong-pane swap on claim. FFI boundary decision: editor layout detection stays in plugin (IntelliJ Swing APIs), --position flag is the shared binary interface. v0.31.30 released.

**v0.31.31 — commit-reliability (5 fixes) + HEAD fenced block + CWD anchor:**
- Fix 1: snapshot + git commit before exit(75) in write.rs (IPC timeout path)
- Fix 2: commit before result? propagation in main.rs
- Fix 3: retry on git index.lock contention (3x backoff: 100/200/400ms)
- Fix 4: agent_doc_commit FFI export; NativeLib.kt + PatchWatcher.kt call it post-apply
- Fix 5: preflight cross-document sweep — scans all tracked docs, commits any where snapshot mtime > file mtime
- HEAD fenced block bug: pulldown-cmark AST replaces ad-hoc is_fence_marker across 5 code paths in boundary.rs. Prevents (HEAD) markers inside fenced code blocks. Triggering case: nested ```bash inside plain ``` fence.
- project_config_path(): walk-up from CWD to find .agent-doc/, avoiding CWD-sensitivity in submodule sessions
- v0.31.31 released (crates.io + PyPI + GitHub Release)

**IDE/Editor options for AWS server (SSH/tmux, no JetBrains):**
Compared neovim, vim, zed, vscode (code-server), cursor, for agent-doc integration (IPC socket + git gutter refresh).
- neovim: recommended — SSH native, gitsigns.nvim, Lua plugin ~3–5 days
- vim: minimal overhead, no built-in LSP, agent-doc plugin ~5–7 days (harder ecosystem)
- zed: remote SSH native (beta), Rust extension API, ~5–7 days; GPU dependency may be an issue on headless server
- vscode (code-server): most familiar, git SCM built-in, TypeScript extension ~2–3 days; high memory (~300MB+)
- cursor: fork of vscode, same integration path as vscode; subscription cost; ~2–3 days
- No 'antigravity' or 'ki' found in research — likely not real or too niche

**Open items:**
- /model haiku: switch model to haiku (run /model haiku in the terminal — cannot execute Claude Code built-ins from within the session)
- IDE integration: decision pending on which editor to implement next for AWS server use

❯ How can we make agent-doc more intelligent about selecting the optimal model for a particular task. Also taking into account /usage to avoid limits.
### Re: Intelligent model selection

**Model selection signals:**

1. **Diff complexity** — preflight already computes `diff_type`. Small content additions → haiku; complex multi-section restructuring or code debugging → sonnet/opus. The binary could return a `recommended_model` field in preflight JSON.

2. **Document/task type** — frontmatter `agent_doc_model: auto | haiku | sonnet | opus`. With `auto`, heuristics apply. Research/planning docs → sonnet; status/notes docs → haiku.

3. **Usage awareness** — harder. `/usage` is a Claude Code built-in, not callable from within a session. But the Anthropic API exposes usage programmatically. Alternatively, Claude Code writes usage data somewhere on disk — if we can locate and parse it, the binary could read it and apply a budget threshold.

**How the model switch would work:**

The binary can't switch models mid-session (that's a Claude Code built-in). Options:
- Preflight returns `recommended_model` → skill surfaces it as a note in the document: "For this task, haiku is sufficient. Run `/model haiku` to switch."
- Or write it to `.agent-doc/session.toml` so the next invocation of `/agent-doc` starts with a prompt to switch.

**Usage limits → budget mode:**

`.agent-doc/config.toml` could have a `[model]` section:
```toml
[model]
auto = true
budget_threshold = 0.8  # switch to haiku when >80% of limit consumed
preferred = "sonnet"
budget_fallback = "haiku"
```

The binary would check usage against the threshold and return `recommended_model` accordingly.

**Concrete next step:** Add `recommended_model: Option<String>` to preflight JSON (based on diff complexity + frontmatter), and have the skill surface it when it differs from the current session model. Usage integration can be a follow-on once we know where Claude Code writes usage data.

Want to spec this out as a feature?
❯ Yes. I also want to account for codex and other harnesses with different available models.
❯ What would you recommend? Having a general name (low, med, high)?
### Re: Model tier spec

**Tier names: `low | med | high`**

Simple, unambiguous, harness-agnostic. The config maps tiers to concrete model names per harness:

```toml
# .agent-doc/config.toml
[model]
auto = true
budget_threshold = 0.8  # fraction of daily limit; switch to low when exceeded

[model.tiers.claude-code]
low = "haiku"
med = "sonnet"
high = "opus"

[model.tiers.codex]
low = "gpt-4o-mini"
med = "gpt-4o"
high = "o3"

[model.tiers.default]
low = "haiku"
med = "sonnet"
high = "opus"
```

**Preflight returns tier, not model name:**
```json
{
  "recommended_tier": "low",
  "recommended_tier_reason": "small diff, content_addition"
}
```

The skill resolves tier → concrete model via config + detected harness. This way the recommendation logic lives in the binary (harness-agnostic), and model resolution lives in config (harness-specific).

**Frontmatter override:**
```yaml
agent_doc_model_tier: auto | low | med | high
```

**Tier assignment heuristics:**
- `low`: `diff_type = content_addition`, small diff (<20 lines), notes/status docs
- `med`: multi-section edits, planning/feature docs, moderate diff
- `high`: complex debugging, architecture decisions, large diffs with code changes

**Harness detection:** Binary can detect from env vars (`CLAUDE_CODE_SESSION`, `CODEX_SESSION`, etc.) or a `harness` field in config.

Want me to draft the spec file for this feature (in `tasks/agent-doc/`)?
❯ Would some harnesses have more than 3 tiers? Should we use a numeric system?
### Re: More than 3 tiers?

Some harnesses do have more granularity (OpenAI has gpt-4o-mini, gpt-4o, o1-mini, o3-mini, o3), but that doesn't argue for more tiers in the recommendation system.

**The key question is:** what does the recommendation system need to decide? It's classifying task complexity: small / medium / hard. That's 3 meaningful levels. More than 3 starts requiring sub-distinctions that are hard to heuristically detect.

**For harnesses with 4-5 models:** the user picks which 3 buckets to map to. OpenAI example:

```toml
[model.tiers.codex]
low = "gpt-4o-mini"
med = "gpt-4o"
high = "o3"          # or "o1" depending on preference
```

o1-mini and o3-mini are available for manual frontmatter override (`agent_doc_model_tier: o1-mini`) but not auto-selected.

**Against numeric (1-10):**
- The heuristic can only reliably distinguish ~3 complexity levels from diff analysis
- A numeric scale would be arbitrary with no real precision gain
- Named tiers (`low/med/high`) have semantic meaning; `3` out of `7` doesn't
- Extra complexity without benefit

**Conclusion:** Keep `low/med/high`. If a 4th tier proves necessary (e.g., reasoning models for planning), we can add `reasoning` at that point. Don't over-engineer for a hypothetical.

❯ Ok...how would the complexity be determined? By the default running model?
### Re: How is complexity determined?

Complexity is determined from signals already computed in preflight — the running model isn't involved. The assessment is about the task, not the model.

**Primary signal: `diff_type`** (already computed by preflight)

| `diff_type` | Tier |
|---|---|
| `simple_question` | low |
| `content_addition` < 10 lines | low |
| `content_addition` > 10 lines | med |
| `multi_section` | med |
| `code_change` / debugging | high |
| `architecture_change` | high |

**Secondary signals:**

- **Document path:** `tasks/professional/` → low; `tasks/research/` → med; `tasks/software/` → high
- **Diff size:** line count as a tiebreaker
- **Frontmatter override:** `agent_doc_model_tier: low|med|high` locks it regardless of heuristics

**Sketch:**

```rust
fn recommended_tier(diff: &Diff, doc_path: &Path, fm: &Frontmatter) -> Tier {
    // Frontmatter wins
    if let Some(t) = fm.agent_doc_model_tier { if t != Auto { return t; } }

    // Diff type is primary
    match diff.diff_type {
        SimpleQuestion | SmallContentAddition => Low,
        ContentAddition | MultiSection => Med,
        CodeChange | ArchitectureChange => High,
        _ => Med, // safe default
    }
}
```

The running model is irrelevant to the assessment — `low` means "this task doesn't need a powerful model" regardless of what's currently active. The point is to recommend a switch when the task tier doesn't match the current model.

❯ Sometimes a simple word such as `do` can mean implementing a complex task. How would this be determined?
### Re: Short diffs hiding complex intent (HEAD)

Good edge case. The binary is blind to semantics — "do" is structurally a 1-word diff, same as "ok". But the agent (skill) has full document context.

**Two-stage approach:**

1. **Binary: structural heuristics → `suggested_tier`**
   - Short imperative diffs ("do", "yes", "implement", "go") → flag as `ambiguous_command` diff type
   - Binary can't resolve ambiguous commands semantically, so it returns `suggested_tier: "med"` as a safe default (not `low`)

2. **Agent: semantic override → final tier**
   - The skill reads the full document. It sees "do" and knows what it refers to.
   - If the document has a pending section with 8 implementation items, the agent knows this is high complexity and acts accordingly.
   - The skill can note: "Treating as high-complexity task (pending section has N items)."

**Binary-side heuristic:**

```rust
if diff.lines_added <= 3 {
    let words: Vec<_> = diff.added_text.split_whitespace().collect();
    if is_imperative_command(&words) { // "do", "go", "yes", "implement", "fix"...
        return DiffType::AmbiguousCommand; // → suggested_tier: Med
    }
}
```

**Agent-side resolution:**

The skill already reads the document before responding. It looks at:
- Does `agent:pending` have items? → high if many
- Is this doc in `tasks/software/`? → high
- Is "do" adjacent to a plan with numbered steps? → high

**Key split:** binary suggests (from structure), agent decides (from semantics). For short imperatives specifically, the binary defaults to `med` since it can't know the implied scope.
<!-- agent:boundary:ec6a2dc2 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
