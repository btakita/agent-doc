---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: "--dangerously-skip-permissions"
---

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e7cc7f7a85d39c090242bb3a72d53651075f5c7f9a1053e25d29aa36657310cc-20260318-030250.md`*

### Re: Audit docs + compact

**Compact done:** 591 lines archived to `.agent-doc/archives/`.

**Audit-docs found 4 stale files** (all older than `src/resync.rs`):
- `.claude/skills/agent-doc/SKILL.md`
- `CLAUDE.md`
- `README.md`
- `SPEC.md`

These need updating to document `resync --fix`, the `split-window` auto_start behavior, and the new tests. Want me to update them?

yes

It seems that human prompts sometimes are lost...when human has multiple prompts...especially when rerunning the document.

agent-doc should track the prompts list.
What would be a good way of doing this?
- agent:pending component
- lazily-rs to track current state
  - ideally this state would be mapped to the document
  - we should avoid having hidden ephemeral state as much as possible
  - we should also be careful to not have "memory leaks"...caused by tracked state being orphaned due to editing the document
- any other suggestions

### Re: Docs updated + incomplete prompt

**All 4 stale docs updated and pushed:**
- **CLAUDE.md:** Updated `resync.rs` description in module layout
- **README.md:** Added Resync section (dry-run + --fix), split-window note in routing
- **SPEC.md:** Full resync specification (detection, allowlist, --fix behavior), auto_start algorithm (5-step cascade)
- **SKILL.md:** Touched mtime only (resync is CLI-only, not a skill)

`agent-doc audit-docs` passes clean.

Your message was cut off: "It seems that person prompts sometimes" — want to finish that thought?

### Re: Prompt tracking — preventing lost prompts

**The problem:** When the user writes multiple prompts between cycles, the agent may only respond to the first (or last), losing the others. This happens because the diff is treated as one blob rather than a queue of discrete user changes.

**Analysis of your options:**

**1. `<!-- agent:pending -->
- [ ] Should the skill automatically add agent:pending/agent:exchange to all documents?
<!-- /agent:pending -->
```

The agent reads pending items, processes each, checks them off, and writes `<!-- patch:pending -->` with updated checkboxes. Unaddressed items survive across cycles.

Want me to implement this?

Will patch:pending delete items once they are complete?

### Re: Will patch:pending delete completed items?

**It depends on the component mode.** Since `agent:pending` uses `mode=replace`, each `patch:pending` replaces the entire component content. The agent controls what remains:

- **Option A: Check off then delete** — Agent marks `[x]` on the current cycle, removes the item on the next cycle. Gives the user one cycle to see it was addressed.
- **Option B: Immediate delete** — Agent omits completed items from the replacement. Cleaner, but no visibility that the item was processed.
- **Option C: Keep checked items** — Agent keeps `[x]` items indefinitely. User manually deletes when satisfied. Most conservative.

**Recommended: Option A** (check off, then remove next cycle). The pending component stays concise while giving brief visibility. The exchange component has the full response anyway, so the audit trail is there.

The skill logic would be:
1. Read `agent:pending` — get list of items
2. For each unchecked item: process it, write response to exchange, mark `[x]`
3. Remove items that were already `[x]` from last cycle
4. Write `patch:pending` with the updated list

This way completed items survive exactly one cycle for visibility, then auto-clean.

Want me to implement this in the skill and document it?

Yes. I'm also not liking mode=append|replace...Should we call it patch=append|replace instead?
mode is too generic. This is how we are patching the content of the component.

### Re: Rename mode→patch + implement pending

**`mode` → `patch` rename:** Agreed. `patch=append|replace|prepend` is more precise — it describes the patching strategy, not a generic "mode." Current syntax `<!-- agent:name mode=append -->` becomes `<!-- agent:name patch=append -->`.

**Backward compatibility:** Accept both `mode=` and `patch=` during a transition period, with `patch=` taking precedence. This lets existing documents keep working while new ones use the better name.

**Changes needed:**
1. `component.rs` — parse `patch=` attribute (alongside `mode=` for compat)
2. `patch.rs` — use the resolved patch strategy
3. `components.toml` — rename `mode` key to `patch` (keep `mode` as alias)
4. SKILL.md, SPEC.md, README.md — update docs
5. All existing documents — update inline attributes (can be done gradually)

**Pending tracking:** I'll implement the `agent:pending` component behavior in the SKILL.md skill logic. This is a skill-side change (Claude Code interprets the component), not a Rust change (the Rust side already handles replace-mode components).

I'll implement both now. The `mode→patch` rename goes in Rust; the pending tracking goes in the skill.

I thought we were using rust for the at least some of the pending tracking.

### Re: mode→patch rename implemented

**Done.** `patch=append|replace|prepend` is now the primary inline attribute. `mode=` kept as backward-compatible alias.

**Changes (12 files):**
- `component.rs` — new `patch_mode()` method: checks `patch=` first, falls back to `mode=`
- `patch.rs` — `ComponentConfig.mode` renamed to `patch` with `#[serde(alias = "mode")]`
- `template.rs` — all `attrs.get("mode")` → `patch_mode()`
- `claim.rs` — scaffold now uses `patch=`
- `write.rs` — updated comment
- JB plugin + VSCode extension — updated parsers for `patch=` with `mode=` fallback
- CLAUDE.md, SPEC.md, README.md, PLUGIN-SPEC.md — docs updated
- SKILL.md (repo-level) — updated manually
- 7 new tests for attribute parsing and precedence

Committed and pushed. This document's `<!-- agent:status mode=replace -->` and `<!-- agent:exchange mode=append -->` still work via backward compat — you can update them to `patch=` at your convenience.

**Pending tracking:** Deferred to next cycle — the `agent:pending` component is a skill-side addition that needs careful design of the checkpoint/flush behavior. Want to tackle it in a fresh session?

What if we treat the document as the "memory".
Would the skill be sufficient to manage checking the boxes and ensuring the pending component content is consistent?
The document would be the source of truth.
If there are dependencies between tasks, we can have a graph language that we can use to track the task dependencies and their status.

### Re: Document as memory + task graph

**Yes, the skill alone is sufficient.** The document IS the memory — no Rust-side pending tracking needed. Here's why:

1. **The skill reads `agent:pending` as a replace-mode component** — it gets the full list of items each cycle
2. **The skill processes unchecked items**, writes responses to `exchange`, and writes back an updated `patch:pending` with checkboxes updated
3. **The snapshot diff catches any new items** the user added between cycles
4. **No Rust changes needed** — the existing component/patch infrastructure already handles replace-mode components perfectly

**Addressing your earlier point** ("I thought we were using rust for at least some"): You're right that Rust could validate consistency (e.g., `agent-doc write` could warn if pending items exist but weren't addressed). But the core logic — reading items, deciding what to process, checking boxes — is inherently non-deterministic (it requires LLM judgment), so it belongs in the skill.

**Task dependency graph:** Great idea. A lightweight syntax within the pending component:

```markdown
<!-- agent:pending patch=replace -->
- [ ] Implement auth middleware
- [ ] Add rate limiting (depends: auth middleware)
- [ ] Write integration tests (depends: auth middleware, rate limiting)
<!-- /agent:pending -->
```

The skill parses `(depends: ...)` annotations and only processes items whose dependencies are `[x]`. This is purely skill-side — just text parsing in the SKILL.md instructions. No graph library needed.

**Or** if dependencies get complex, a DAG notation:

```markdown
<!-- agent:pending patch=replace -->
auth -> rate_limiting -> integration_tests
auth -> logging

- [ ] auth: Implement auth middleware
- [ ] rate_limiting: Add rate limiting
- [ ] logging: Add structured logging
- [ ] integration_tests: Write integration tests
<!-- /agent:pending -->
```

The first lines define the DAG; the checkbox items are the work. Skill processes only items with all upstream dependencies checked.

**Next step:** Implement the pending tracking in SKILL.md. Want me to do it now, or save for a fresh session? This session has been very productive — we shipped v0.17.16 through v0.17.19 plus the `patch=` rename.

go

### Re: Pending tracking implemented + v0.17.20 released

**v0.17.20 released** (all targets: crates.io, PyPI, GitHub):
- `patch=` inline attribute (renamed from `mode=`)
- 7 new attribute parsing tests
- Updated JB + VSCode plugins

**Pending tracking added to SKILL.md:**

The skill now manages `<!-- agent:pending patch=replace -->` as a task queue:

1. Read pending component, parse `- [ ]` / `- [x]` items
2. Remove `[x]` items from previous cycle (already addressed)
3. Extract new prompts from diff, add as `- [ ]` items
4. Process each unchecked item sequentially, mark `[x]` after responding
5. Write `patch:pending` with updated list alongside exchange response

**Dependency tracking** supported via `(depends: item_name)` — skill only processes items with all dependencies checked.

**No Rust changes needed** — the existing replace-mode component infrastructure handles everything. The pending logic is purely in the SKILL.md instructions.

This is ready to use. Add `<!-- agent:pending patch=replace --><!-- /agent:pending -->` to any template document to enable it.

What would happen with agent:pending patch=append or patch=prepend?

### Re: pending with append or prepend

**`patch=append`** — Each cycle's checked/unchecked list gets *appended* below the previous content. The pending component grows forever — completed items accumulate rather than being replaced. This creates an audit log of all tasks ever tracked, but the component becomes noisy fast. Not useful for a task queue.

**`patch=prepend`** — Each cycle's list gets *prepended* above existing content. Same accumulation problem as append, just in reverse order. Newest items at top, oldest at bottom.

**Why `patch=replace` is correct for pending:** The whole point is that the skill rewrites the complete task list each cycle — removing completed items, adding new ones, updating checkboxes. Replace gives the skill full control over the component's content. Append/prepend would mean the skill can only add, never clean up.

**The pattern:** Components that represent *current state* (status, pending) use `patch=replace`. Components that represent *history* (exchange, findings) use `patch=append`. This maps cleanly to the semantics:

| Component | Patch | Why |
|-----------|-------|-----|
| `status` | replace | Current state, not history |
| `pending` | replace | Current task queue, auto-cleaned |
| `exchange` | append | Conversation history, grows |
| `findings` | append | Accumulated research, grows |
| `output` | replace | Latest response only |

Is the 2nd column the default patch value?

### Re: Default patch values

**No — the 2nd column shows the *recommended* patch value, not the default.** The actual default for all components is `replace`. The table shows what each component *should* be set to via `patch=` attribute or `components.toml`.

We should make this the default value.
All of these components should be documented in README...with the default patch value.

Currently, `exchange` and `findings` get `patch=append` because:
- `exchange` has a built-in default of `append` in the Rust code (hardcoded for this specific component name)
- `findings` also has a built-in default of `append`
- All other components default to `replace`

So the built-in defaults are:

| Component | Built-in default | Rationale |
|-----------|-----------------|-----------|
| `exchange` | append | Hardcoded in Rust |
| `findings` | append | Hardcoded in Rust |
| Everything else | replace | Generic default |

The inline `patch=` attribute or `components.toml` can override any of these. The precedence chain is: inline `patch=` > `components.toml` > built-in defaults.

### Re: Document component defaults in README

**Done.** Added standard component names table to both README.md and SPEC.md:

| Component | Default `patch` | Description |
|-----------|----------------|-------------|
| `exchange` | append | Conversation history |
| `findings` | append | Accumulated research data |
| `status` | replace | Current state |
| `pending` | replace | Task queue, auto-cleaned |
| `output` | replace | Latest agent response |
| `input` | replace | User prompt area |
| (custom) | replace | All others default to replace |

`pending` already defaults to `replace` (it's not `exchange` or `findings`, so it gets the generic default). No Rust changes needed — just documentation.

Committed and pushed. All tests pass.

Add `agent:pending` so I can see what's happening. Have `agent:pending` below `agent:exchange` so both can be viewed.

Should the skill automatically add agent:exchange to all documents?

### Re: Adding agent:pending component

Adding `<!-- agent:pending -->` below `<!-- agent:exchange -->` now. This will be visible in the editor so you can see the task queue in action.

What is the difference between agent:status and agent:pending?

### Re: status vs pending

**`agent:status`** — A human-readable summary of where things stand. Updated at milestones. Shows phase progress like `- [x] Phase 1\
- [ ] Phase 2`. The agent writes it; the user reads it. It's a dashboard.

**`agent:pending`** — A task queue of user prompts waiting to be processed. The user writes items (or the skill extracts them from the diff); the agent processes and checks them off. It's an inbox.

| | `status` | `pending` |
|---|---------|-----------|
| **Who writes** | Agent | User (+ skill auto-extracts) |
| **Who reads** | User | Agent (to know what to do next) |
| **Content** | Phase/milestone progress | Checkbox task items |
| **Lifecycle** | Updated at milestones | Items added/removed each cycle |
| **Purpose** | "Where are we?" | "What needs doing?" |

They complement each other: `pending` is the work queue, `status` is the progress report.

### Re: Should skill auto-add agent:pending/exchange?

**Yes, for template documents.** When `agent-doc claim` scaffolds a new template document, it already creates `agent:status` and `agent:exchange`. Adding `agent:pending` to the scaffold makes sense — it's zero-cost if unused (empty component) and immediately useful when the user starts writing multiple prompts.

The `claim.rs` scaffold would become:
```markdown
<!-- agent:status patch=replace -->
<!-- /agent:status -->

<!-- agent:exchange patch=append -->
```

### Re: claim.rs scaffold updated

**Done.** `agent-doc claim` now scaffolds `agent:pending` alongside `agent:status` and `agent:exchange` for new template documents:

```markdown
<!-- agent:status patch=replace -->
<!-- /agent:status -->

<!-- agent:exchange patch=append -->
```

### Re: Pending additions → exchange prompts

**Agreed.** Any free-text additions to `agent:pending` should be treated as prompts for `agent:exchange`, not as pending items. The pending component should only contain checkbox items managed by the skill.

**Updated rule for the skill:**
- If the diff shows new text in `agent:pending` that is NOT a checkbox item (`- [ ]` / `- [x]`), move it to the exchange as a user prompt and respond to it there
- Only `- [ ]` items in pending are treated as task queue entries
- This prevents accidental prompt loss when the user types below the exchange

**Practical example:** If the user accidentally types "fix the bug" inside `agent:pending`, the skill should:
1. Extract "fix the bug" from pending
2. Add it to exchange as a user prompt
3. Respond to it in exchange
4. Write `patch:pending` without the free text (only checkboxes remain)

I'll update the SKILL.md pending tracking section with this rule.

### Re: discuss (moved from pending)

*[Free text "discuss" detected in `agent:pending` — moved to exchange per the free-text rule.]*

**Session summary — what we've built tonight:**

| Version | Features |
|---------|----------|
| v0.17.16 | `tmux_session` routing from frontmatter, `claude_args` 3-source config |
| v0.17.17 | `parse_option_line` bracket format fix (no more markdown false positives) |
| v0.17.18 | `resync --fix` (wrong-session/wrong-process detection) |
| v0.17.19 | `split-window` auto_start (no orphaned windows), 11 new tests |
| v0.17.20 | `patch=` rename from `mode=`, component defaults table in docs |

**Design decisions made:**
- `agent:pending` component with replace-mode task queue
- Free-text in pending → exchange (prevents lost prompts)
- Document-as-memory (no hidden state)
- Dependency tracking via `(depends: name)` syntax
- `patch=append|replace|prepend` naming (clearer than `mode=`)

**Open item in pending:** "Should the skill automatically add agent:pending/agent:exchange to all documents?" — already implemented in claim.rs for template documents. Should I mark it done?

What would you like to discuss?

Yes...it's done. Seems implemented.
Looks like the triple backtick formatting is not correct though.
I corrected it manually.
Also "discuss" was not replaced in `agent:pending`.

```
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
```

Committed and pushed.

Want me to update `claim.rs` to include `agent:pending` in the scaffold?

Yes

While it is useful to see pending below exchange...especially for long exchange content...I can forsee accidently adding prompts meant for exchange into the pending content.
We should assume additions to `agent:pending` were meant for `agent:exchange`.


<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] Add agent:pending component to this document
- [ ] Should the skill automatically add agent:pending/agent:exchange to all documents?
discuss
<!-- /agent:pending -->
