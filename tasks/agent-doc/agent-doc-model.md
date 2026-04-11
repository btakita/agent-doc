---
agent_doc_session: 66ad9274-0ae0-473e-9043-7c67f6d98c78
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
agent-doc model tier spec — design discussion
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Extracted from agent-doc-bugs.md — model tier selection design*

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
### Re: Short diffs hiding complex intent

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

❯ Would a haiku model know to defer to sonnet or opus for an ambiguous diff with complex context?
### Re: Can haiku recognize when to defer?

No — and that's the crux of the problem. If haiku is already running, you can't rely on it to recognize when it's out of its depth. The semantic override approach is a reliability gap.

**Better architecture: front-load the decision into the binary**

Instead of relying on the agent to semantically override, push more signal into the binary's structural analysis so `suggested_tier` is already correct before any model runs:

```rust
if diff_type == AmbiguousCommand {
    // Structural signals the binary CAN check:
    if document.pending_item_count() > 3 { return Tier::High; }
    if document.path.contains("tasks/software/") { return Tier::High; }
    return Tier::Med;
}
```

The binary can count pending items, inspect document path, check for code blocks in recent content — structural, not semantic. This makes `suggested_tier` reliable without depending on the running model's judgment.

**Gate before generation:**

The skill checks the gate before responding:
1. Preflight returns `suggested_tier: "high"`
2. Skill detects current model is haiku (lower than `high`)
3. Skill writes to document: "Switch to sonnet for this task. Run `/model sonnet` and re-run."
4. **Stop** — no generation attempt

This is a simple comparison, not semantic reasoning. Even haiku executes it reliably.

**The semantic override step is fragile and should be minimized.** Push as much intelligence as possible into the binary's structural analysis. The agent's semantic judgment is only for genuinely ambiguous cases structural signals can't resolve — and even then, the safe default is `med`, not `low`.

❯ Can we have a high model evaluate the diff and be instructed to dispatch subagents for tasks?
### Re: Orchestrator pattern — dispatching subagents

Yes — that's the orchestrator pattern. It fits tasks where the work is decomposable into parallel subtasks.

**The orchestrator role:**

A "thin" high-model pass evaluates the diff + document context and produces a dispatch plan — not a full response. It answers: "What subtasks does this spawn? Which can run in parallel?" Then specialist subagents execute. The orchestrator's output is small (a task list), so it's fast and cheap relative to full generation.

```
diff + doc context
      ↓
 [orchestrator] (high model, fast — produces dispatch plan)
      ↓
 ┌────────────┬────────────┬────────────┐
 [subagent A] [subagent B] [subagent C]   (parallel)
      └─────────────┬───────────┘
               [merge results]
                    ↓
              [write back]
```

**When this is worth the overhead:**
- Task has 2+ clearly independent subtasks (research + implementation, analysis + spec-writing)
- Each subtask is >5 minutes of work (parallelism saves real time)
- Task is naturally "fan out, then fan in"

**When it's NOT worth it:**
- Single linear task → just run the high model directly
- Ambiguous command needing clarification first
- Sequential tasks where output A feeds input B

**Implementation sketch:**

Preflight adds `"dispatch_candidate": true` when it detects high complexity + ambiguous command (pending items >5, code changes, architecture tasks). The skill checks this flag:

1. If `dispatch_candidate == true` and current tier is `high`, skill does a quick orchestration pass (not a full response) — generates a subagent plan
2. Subagents run via the `Agent` tool (already available in Claude Code skills)
3. Results merge and write back via `agent-doc write`

**Inside vs. outside orchestration:**
- Inside (Claude Code's Agent tool): simpler, already available in skills, no extra coordination
- Outside (spawning separate `/agent-doc` sessions): better isolation, more overhead

Inside is the right default.

**Gating:** Only activate when all three hold:
- `suggested_tier == High`
- `dispatch_candidate == true`
- frontmatter `agent_doc_dispatch: auto | true`

Default off — most tasks don't need it.

/model opus: Can we have the user instruct agent-doc to run a model such as here?  
### Re: User-instructed model switch via inline `/model` command (HEAD)

**Builtin note:** `/model` is a Claude Code built-in — cannot be executed from within the skill. To switch models interactively, run `/model opus` at the terminal.

**The design question:** Can agent-doc recognize inline `/model <x>` in the document and act on it?

Not directly — the skill can't invoke CC built-ins. But there are three viable paths:

**Option 1: Preflight emits a `model_switch` field**

Preflight recognizes `/model <name>` in the diff, strips it from the diff (not content), and returns:
```json
{ "model_switch": "opus", "model_switch_reason": "user inline command" }
```
Skill writes a gated note to the document:
> "Document requested opus. Run `/model opus` at the terminal and re-invoke `/agent-doc`."

This surfaces the request but can't auto-execute. Still useful — user sees a clear action in the document.

**Option 2: Native `agent_doc_model_tier` frontmatter**

Skip inline commands. User writes in frontmatter:
```yaml
agent_doc_model_tier: high
```
Preflight reads this and emits `recommended_tier: high`. Skill compares against current model, gates if mismatch. This persists across all cycles (not one-shot like an inline command), and works across all harnesses.

**Option 3: `&lt;!-- agent:model --&gt;` component**

A named component that the user edits:
```markdown
&lt;!-- agent:model --&gt;
high
&lt;!-- /agent:model --&gt;
```
Preflight reads it as the tier override. Can be updated by either user or agent. Works without frontmatter editing.

**Recommended:** Option 2 (frontmatter) for persistent tier, Option 1 (preflight flag) for one-shot inline overrides. Option 3 is ergonomic but adds component complexity.

**For this specific use case (user wants opus for a session):** frontmatter `agent_doc_model_tier: high` is the cleanest — set once, respected every cycle, persists in git history.
<!-- agent:boundary:40187d66 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
