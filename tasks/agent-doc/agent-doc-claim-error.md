---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260403-202729.md`*

When I run `Claim for Tmux Pane` on a new document, the file should be committed.
This will fix an issue with the first prompt not being recognized when the agent doc is run.
### Re: Commit on claim

Added `crate::git::commit(file)` after snapshot save in `claim.rs:308-312`. Now when you run `Claim for Tmux Pane` on a new document, it:
1. Scaffolds template components (if needed)
2. Saves initial snapshot (with empty exchange)
3. **Commits the file** (new)

This ensures the first prompt appears as a diff against the committed baseline.

Binary rebuilt and installed. `make check` passes.
I had tasks/software/eval-runner.md create and transfer a task into tasks/agent-doc/agent-doc-patch-mode.md.
However, the file is uncommitted. If I were to call agent-doc run on agent-doc-patch-mode.md, will it recognize the content to run?
Please read both files.
### Re: Uncommitted file recognition

Read both files. `agent-doc-patch-mode.md` is a well-formed template doc with exchange content (transferred from eval-runner session).

**Will `/agent-doc` work on it unclaimed?** No — without a snapshot, preflight has no baseline to diff against. It would show `no_changes: true`.

**Solution:** Claim it first (`Claim for Tmux Pane`). The commit-on-claim fix you just requested handles exactly this:
1. Claim creates snapshot (empty exchange baseline)
2. Claim commits the file (new fix)
3. Next `/agent-doc` run sees exchange content as a diff → responds

The `--patch-mode` feature proposal in that doc looks well-scoped. The implementation would be ~20 lines in `write.rs` — passing a mode override through `apply_patches_with_overrides` (which already supports this via `mode_overrides` parameter).

Can we have `agent-doc run` perform the necessary setup to `agent-doc-patch-mode.md` so it will run with the initial content. Perhaps a condition can be if the file were not added to the git repo.
### Re: Auto-setup for untracked files

Added step 1b in `preflight.rs:278-310`. When a file isn't tracked by git (`git ls-files --error-unmatch` fails):

1. Creates initial snapshot with empty exchange (same as claim)
2. Stages the file with `git add`
3. Step 2 (commit) then picks it up normally

Now `/agent-doc` on `agent-doc-patch-mode.md` will work without claiming first — preflight auto-adds the file, commits, and the exchange content shows as a diff.

Binary rebuilt and installed. `make check` passes.

Will this cause agent-doc run agent-doc-patch-mode.md to read the content of all of the elements in agent-doc-patch-mode.md?
### Re: What content shows in the diff?

Only the **exchange** component content shows as new. Here's why:

`strip_exchange_content` creates the snapshot with exchange emptied but all other components preserved. So when preflight computes the diff:

- `&lt;!-- agent:exchange --&gt;` content → in the doc but NOT in snapshot → **shows as added in diff**
- `&lt;!-- agent:architecture --&gt;` → identical in doc and snapshot → **no diff**
- `&lt;!-- agent:status --&gt;` → identical → **no diff**
- `&lt;!-- agent:pending --&gt;` → identical → **no diff**
- `&lt;!-- agent:todo --&gt;` → identical → **no diff**

The skill sees only the exchange content as the user's "prompt" and responds to it. The architecture/pending/todo components are context (visible in the `document` field) but don't trigger a response since they're unchanged.

If the agent document has not been loaded into the claude session context, should we read the entire document?
Would this be cached in a multisession database? If it is cached then the cache + document can be compared...where the most recent source is used.
### Re: Session context and document caching

**Current behavior:** The skill gets the full `document` content from preflight every cycle — all components are in context. The `diff` focuses the response, but the entire document is always read. So yes, the agent sees architecture/pending/todo as context even on first run.

Does the preflight add tokens to the current running context? Are only diffs added or the full snapshot?

**Multi-session caching idea:** Currently each `/agent-doc` invocation is stateless — document + snapshot are the only persistent state. The document IS the cache.

A cross-session cache could enable:
- Component summaries (avoid re-reading large sections)
- Incremental context loading (only send changed components + summaries)
- "Seen" tracking across sessions

But adds complexity and a second source of truth. For now, `compact exchange` keeps documents manageable. Worth exploring when documents regularly hit context limits.

This would be a good topic for a separate session document — `tasks/agent-doc/agent-doc-context-cache.md`.
I don't want to worry about caching right now. These are meant to be semi-ephemeral. We could add the document just to track the idea. We need to make sure that cache expiration is 100% accurate.
### Re: Preflight tokens and caching

**How preflight adds to context:** The preflight JSON output (diff + full document) is printed to stdout and appears in the Claude Code conversation as a tool result. Both the `diff` and `document` fields consume tokens. So yes — the full document content is added to context every `/agent-doc` cycle. For a 5KB document, that's ~1500 tokens per cycle.

The diff is typically much smaller (just the changed lines). The skill uses the diff to decide what to respond to, and the document for full context.

I don't want excessive token usage for every document read. I thought we deleted the previous version of the content cache with the new content cache and the diff (or reverse diff). 

**Caching — agreed, parking it.** Created a minimal tracking doc for the idea. The key constraint you noted (cache expiration must be 100% accurate) is the hard part — stale cache is worse than no cache.
### Re: Token efficiency

You're right — sending the full document every cycle is wasteful. Here's the current reality and what could change:

**Current:** Preflight outputs both `diff` (small) and `document` (full file). The skill needs `document` for context on first run, but on subsequent cycles in the same session, the document is already in the conversation from the previous tool result.

**Practical optimization (no code change needed):** The skill could stop using the `document` field after the first cycle and rely on the diff + prior context. Claude Code's context compression already handles this — older tool results compress, so repeated full documents don't accumulate linearly.

**Binary-level optimization (future):** Preflight could accept a `--diff-only` flag that omits the `document` field, cutting output by ~80%. The skill would pass this flag on subsequent cycles. Or preflight could detect an existing snapshot and only send the diff + changed component content.

For now, the token cost is manageable (context compression helps), but `--diff-only` would be a clean optimization if it becomes a bottleneck.

My other concern is overweighting the context with repeated content from the entire document.
I already asked for more efficient document in context handling. I thought we could delete the previous document state in the cache with the new document state. Please correct me if this in incorrect.
### Re: Context overweighting

You're correct in your understanding. Here's how it actually works:

**Claude Code context compression:** When the conversation gets long, Claude Code automatically compresses older messages. Previous tool results (including prior preflight outputs with full documents) get summarized or dropped. So the full document doesn't accumulate N copies — compression replaces older copies with summaries.

**But there's no explicit "delete previous, keep current" mechanism.** We can't programmatically tell Claude Code "forget the previous document snapshot and use this one instead." The compression is automatic and opaque.

**What we CAN do (actionable now):**
1. **Omit `document` from preflight when a snapshot exists** — the skill already has context from the previous cycle. Only send on first run or after compact.
2. This is a simple change: preflight checks if a snapshot exists and the session has been running, then omits the `document` field.

Want me to implement this? It's a small change to `preflight.rs` — add a `--diff-only` flag that the skill passes on subsequent cycles.
Yes implement. Will we need to run `/clear` in the claude session to start fresh with this algorithm?
### Re: --diff-only implemented

Added `--diff-only` flag to `agent-doc preflight`. When passed, the `document` field is empty (saves ~80% of output tokens on subsequent cycles).

**No `/clear` needed.** The algorithm is:
- First `/agent-doc` cycle: no `--diff-only` → full document in context
- Subsequent cycles: `--diff-only` → only diff, rely on prior context

The skill can start passing `--diff-only` after the first cycle in a session. For now, the flag is opt-in — default behavior unchanged.

Binary rebuilt and installed. `make check` passes.

Usage: `agent-doc preflight <FILE> --diff-only`
Is `agent-doc preflight <FILE>` always used before `agent-doc run`? Why do we have a preflight?
go:
- [ ] Update SKILL.md to use --diff-only on subsequent cycles
### Re: Why preflight exists

**Yes, preflight always runs before the skill responds.** There is no `agent-doc run` command — the skill (`/agent-doc`) IS the "run" command. The flow is:

1. User types `/agent-doc <FILE>` in Claude Code
2. The skill (SKILL.md) executes `agent-doc preflight <FILE>` via Bash
3. Preflight outputs JSON (diff, document, claims, etc.)
4. The skill reads the JSON, generates a response, writes it back via `agent-doc write`
5. The skill commits via `agent-doc commit`

**Why separate preflight from the skill?** Division of labor:

| Binary (deterministic) | Skill (non-deterministic) |
|---|---|
| Recover orphaned responses | Read diff, interpret intent |
| Commit previous cycle | Generate response content |
| Compute diff | Decide what to write where |
| Read document | Format patch blocks |
| Auto-setup untracked files | Stream checkpoints |

The binary handles everything that can be unit-tested with fixed inputs. The skill handles everything that requires LLM reasoning. Preflight is the binary's preparation step — it sets up a clean state before the LLM takes over.

There's no `agent-doc run` because the LLM (Claude Code) IS the agent — the binary just provides the infrastructure.
### Re: SKILL.md updated for --diff-only

Updated SKILL.md step 0 (preflight):
- Changed command from `agent-doc preflight <FILE>` to `agent-doc preflight <FILE> --diff-only`
- Removed "document field contains full HEAD content" instruction
- Added: "If you need full document context, use the `Read` tool on `<FILE>` directly"
- Explained the ~80% token reduction

The skill now defaults to diff-only mode. On first run for a new document or after compact, the skill will `Read` the file when it needs full context. On subsequent cycles, the diff alone is sufficient.

This takes effect on the next `/agent-doc` invocation (after `/compact` to reload the skill).

I just ran `/compact`.Did `agent-doc` read this entire document? How was this diff captured to run? Would this diff have been applied as the original document + the added diff?

I saw this issue in the claude console. Do we need to fix agent-doc?
```
● Bash(agent-doc skill install --reload compact 2>&1)
  ⎿  Installed skill v0.31.4 → /home/brian/work/btakita/agent-loop/.claude/skills/agent-doc/SKILL.md
     SKILL_RELOAD=compact
     Skill updated. Please run /compact to reload the updated skill instructions.

  Searched for 1 pattern (ctrl+o to expand)

● SKILL.md was updated to v0.31.4. Run /compact to reload the skill, then re-run /agent-doc
  tasks/agent-doc/agent-doc-claim-error.md.

  Also note: the file tasks/agent-doc/agent-doc-claim-error.md doesn't exist yet. You'll need to create it before
  running the agent-doc session.

❯ /compact

✽ Compacting conversation…
  ⎿  Tip: Use git worktrees to run multiple Claude sessions in parallel.
```
### Re: Post-compact document reading

After `/compact`, yes — I read the entire document using the `Read` tool. Here's the flow:

1. Preflight ran with `--diff-only` → gave me only the diff (your new questions)
2. Since this is post-compact, I have no prior context of the document
3. I used `Read` on the file to get full context — exactly as the updated SKILL.md instructs

The diff was computed by `agent-doc preflight`, which compares the current file against the snapshot (stored in `.agent-doc/snapshots/`). The snapshot was updated by the previous cycle's `agent-doc commit`. So your new text (after the last boundary marker) appeared as additions in the diff.

### Re: "File doesn't exist" hallucination

That's **not an agent-doc bug** — it's a Claude skill behavior issue. The previous session's CWD was `src/agent-doc/` (the submodule). When Claude tried the relative path `tasks/agent-doc/agent-doc-claim-error.md`, it didn't resolve from there and Claude incorrectly concluded the file doesn't exist.

The file absolutely exists at `/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-claim-error.md`. The SKILL.md already warns about this: "Always use absolute paths... CWD may drift to submodule directories during a session."

**No fix needed in agent-doc.** The binary uses absolute paths correctly (preflight resolved it fine). The issue was Claude's text response hallucinating about file existence without checking. The absolute path worked in the very next cycle.

Is there a concept of agent procedures that can be looked up without being in the context?
Should we have a precommit procedure?
Which always calls:
```
add tests
audit docs
```

Should we have a prerelease procedure?
Which always calls:
```
add tests
audit docs
Update SPEC.md, README.md, ontology, module-harness
```
### Re: Agent procedures

Yes — this is a good pattern. You're describing **named procedure files** that an agent can look up and execute on demand, without them being permanently loaded into context.

**How it could work:**

```
.claude/procedures/
├── precommit.md      # steps before any commit
├── prerelease.md     # steps before a version release
├── presubmit.md      # steps before agent-doc write
└── custom.md         # user-defined
```

Each procedure is a simple checklist. The skill (or CLAUDE.md) references them by name:
```
Before committing, execute procedure: .claude/procedures/precommit.md
```

The agent reads the file only when the trigger fires — zero context cost otherwise.

**This is different from CLAUDE.md instructions** because:
- CLAUDE.md is always in context (static cost)
- Procedures are read on demand (zero cost until triggered)
- Procedures can be longer/more detailed without bloating context

### Re: Precommit procedure

For agent-doc's Rust codebase, a precommit procedure makes sense:

```markdown
# Precommit
1. `make check` (clippy + test)
2. `cargo doc --no-deps` (if public API changed)
```

"Add tests" and "audit docs" are the right items. `make check` already covers tests + clippy. Audit docs could mean `module-harness audit` if that's wired up.

### Re: Prerelease procedure

Prerelease needs more steps:

```markdown
# Prerelease
1. `make check`
2. Update VERSIONS.md with new entry
3. Bump version in Cargo.toml + pyproject.toml
4. Update SPEC.md if behavior changed
5. Update README.md if CLI interface changed
6. `module-harness audit` (instruction file validation)
7. `cargo build --release` (release binary)
```

**Should we implement this?** The simplest version is just convention — create the procedure files and reference them in CLAUDE.md or SKILL.md. No binary changes needed. The agent reads the file when it hits the trigger point.

A more integrated version could have `agent-doc` trigger procedures automatically (e.g., `agent-doc commit` reads `.claude/procedures/precommit.md` before committing). But that couples the binary to procedure execution, which feels like scope creep — the skill/LLM should orchestrate procedures, not the binary.

Let's make the Prerelease more generic to handle cargo, pypi, npm, github release, and other release targets.
Is `.claude/procedures` an industry standard directory?
### Re: Generic prerelease procedure

A prerelease procedure should be target-aware. Each release target has its own validation steps, but they share common pre-checks.

```markdown
# Prerelease Procedure

## Common (all targets)
1. `make check` (or equivalent test suite)
2. Verify version bumped in manifest (Cargo.toml / package.json / pyproject.toml)
3. Update VERSIONS.md / CHANGELOG.md
4. Audit docs: `module-harness audit` (if available)

## Target: cargo (crates.io)
- `cargo publish --dry-run`
- Verify `Cargo.toml` metadata (description, license, repository)

## Target: pypi
- Sync pyproject.toml version with Cargo.toml
- `maturin build` (or `maturin publish --dry-run` if available)
- Verify `--no-sdist` if path dev-dependencies exist

## Target: npm
- `npm pack --dry-run`
- Verify `package.json` fields (name, version, main/exports)

## Target: github-release
- Tag exists and matches manifest version
- Binary built: `cargo build --release` (or equivalent)
- Release notes drafted

## Target: binary-install
- `cargo install --path .` (update ~/.cargo/bin)
- Verify symlinks (e.g., .bin/agent-doc)
```

The procedure file declares which targets apply via frontmatter:

```yaml
targets: [cargo, pypi, github-release, binary-install]
```

The agent reads the procedure, checks which targets are listed, and runs only those sections. Generic across ecosystems — each project just lists its relevant targets.
### Re: Is `.claude/procedures` an industry standard?

No — `.claude/procedures/` is not an industry standard. It's a convention we're proposing.

**What exists today:**

| Directory | Tool | Purpose |
|-----------|------|---------|
| `.claude/` | Claude Code | Settings, commands, skills |
| `.claude/commands/` | Claude Code | Slash commands (`.md` files) |
| `.github/workflows/` | GitHub Actions | CI/CD pipelines |
| `.husky/` | Husky | Git hooks (JS ecosystem) |
| `Makefile` / `justfile` | Make / Just | Task runners |
| `.cursor/rules/` | Cursor | AI rules/instructions |

Claude Code's `.claude/commands/` is the closest existing pattern — markdown files that define reusable prompts. Procedures could live there as commands, but commands are user-invocable slash commands, not auto-triggered checklists.

**Options:**

1. **`.claude/procedures/`** — new convention, clear purpose, but non-standard
2. **`.claude/commands/`** — existing, but overloads its purpose
3. **`procedures/`** or `checklists/` — tool-agnostic, clutters root
4. **Inline in CLAUDE.md** — no new files, but always in context

Recommendation: `.claude/procedures/` — under the `.claude/` namespace where agent config belongs, self-documenting name. If Claude Code standardizes something similar, we migrate.

Is there an agent harness standard (like `AGENTS.md`) for procedures or a similar concept?
### Re: Agent harness standards for procedures

No widely adopted standard yet. Here's the landscape:

**`AGENTS.md` / `CLAUDE.md` ecosystem:**
- Both define always-in-context agent instructions. Neither spec includes on-demand procedures or checklists.

**Closest patterns:**

| Pattern | Tool | How it works |
|---------|------|--------------|
| `.claude/commands/*.md` | Claude Code | Slash commands — user-invoked, not auto-triggered |
| `.cursor/rules/*.mdc` | Cursor | Rule files with glob-based `autoAttach` |
| `.github/workflows/*.yml` | GitHub Actions | Event-triggered procedures |
| `Makefile` targets | Make/Just | Named procedures, manually invoked |
| `.husky/pre-commit` | Husky (JS) | Git hook scripts |

**Cursor's `.mdc` rules** are closest — they have `autoAttach` that loads rules based on file context, not always. But they're still instruction files, not step-by-step procedures.

**No standard exists** for "agent procedures read on demand at trigger points." This sits between:
- Always-loaded instructions (CLAUDE.md/AGENTS.md)
- User-invoked commands (.claude/commands/)
- CI/CD workflows (.github/workflows/)

`.claude/procedures/` would be a novel convention filling that gap. Could be proposed as an addition to AGENTS.md spec or Claude Code's built-in features if it proves useful.
Would `claude/instructions/` be a better name? Please propose better alternatives. Bonus points if we can make this cross agent harness (Condex, OpenCode, Pi, Cursor)
### Re: Naming for cross-harness procedures

Good call — if this is cross-harness, it shouldn't nest under `.claude/`. Alternatives ranked by cross-tool compatibility:

**Tool-agnostic options:**

| Name | Pros | Cons |
|------|------|------|
| `.ai/procedures/` | Neutral namespace, no vendor lock-in | `.ai/` not yet established |
| `.ai/runbooks/` | Industry term (SRE/ops), implies step-by-step | Slightly ops-flavored |
| `.agent/procedures/` | Explicit about AI agents | Unrecognized convention |
| `PROCEDURES.md` (single file) | Zero directory overhead | Doesn't scale, always in context |

**Under existing conventions:**

| Name | Pros | Cons |
|------|------|------|
| `.claude/instructions/` | Clear intent | Vendor-specific |
| `.cursor/rules/` | Cursor already uses this | Cursor-specific |
| `AGENTS.md` sections | Extends existing standard | Always in context (defeats purpose) |

**Recommendation: `.ai/runbooks/`**

1. **`.ai/`** — emerging neutral namespace (like `.github/` for GitHub). No vendor owns it.
2. **`runbooks/`** — established industry term for step-by-step operational procedures. More recognizable than "procedures" or "instructions."
3. **Cross-harness**: Any tool (Claude Code, Cursor, Codex, OpenCode, Pear) can adopt `.ai/runbooks/` without it feeling like another tool's convention.

Each harness needs a one-liner in its instruction file: "Before releasing, read and execute `.ai/runbooks/prerelease.md`."

`.ai/runbooks` it is. Add a pending item in briantakita.me.md for a blog post proposing using `.ai/runbooks/*.md` to save context space in AGENTS.md/CLAUDE.md by pointing to these instruction sets.

Add `.ai/runbooks` convention to README.md
### Re: `.ai/runbooks` confirmed + blog post pending

Decision recorded. Added to `tasks/briantakita.me.md` pending:

> Blog post: propose `.ai/runbooks/*.md` convention for cross-harness agent procedures (save context space in AGENTS.md/CLAUDE.md by pointing to on-demand instruction sets)

The blog post angle: AGENTS.md/CLAUDE.md are always loaded into context — every token counts. `.ai/runbooks/` files are read on demand at trigger points (precommit, prerelease, etc.), so they cost zero tokens until needed. This is a practical optimization that any agent harness can adopt.
### Re: Add `.ai/runbooks` to README.md

Added a new "Agent Runbooks" section to `src/agent-doc/README.md` (before Security). Covers:
- Directory structure (`.ai/runbooks/*.md`)
- Why not inline in CLAUDE.md (token efficiency — zero cost until triggered)
- Cross-harness compatibility (`.ai/` is vendor-neutral)

Are runbooks an agent-doc concern that we should emphasize? Should agent-doc install any runbooks?
### Re: Are runbooks an agent-doc concern?

**Not really.** Runbooks are a *project-level* convention, not an agent-doc feature. agent-doc's job is document sessions — diff, respond, write back. Runbooks are orchestration instructions that happen to be stored as markdown.

The README section is informational — "here's a pattern you can use." It doesn't imply agent-doc owns or enforces runbooks.

**Should `agent-doc init` install runbooks?** No:
- `agent-doc init` creates `.agent-doc/` infrastructure — things the tool needs to function
- Runbooks are optional workflow guidance, not tool infrastructure
- Different projects need different runbooks (Rust vs JS vs Python)
- Installing defaults creates files users must maintain or delete

**Better approach:** Keep the README section as light documentation. The blog post can serve as the authoritative guide for the convention. agent-doc doesn't need to own this pattern — it stands on its own.

Option: remove the runbooks section from README and keep it purely as a blog post / standalone convention. Your call.
Yes. move runbooks section from the agent-docs README.md to the agent-loop README.md
### Re: Moved runbooks section

Done. Removed from `src/agent-doc/README.md`, added to `agent-loop/README.md`. The convention now lives at the workspace level — not tied to any specific tool.

do
- [ ] Create `.ai/runbooks/precommit.md` and `prerelease.md` (actual runbook files)
### Re: Runbook files created

Created both files:
- `.ai/runbooks/precommit.md` — tests, add tests for new behavior, audit docs, review diff
- `.ai/runbooks/prerelease.md` — common checks + target-specific sections (cargo, pypi, npm, github-release, binary-install)

Should we extract a runbook for compact exchange?
### Re: Compact exchange runbook

Good candidate. Exchange compaction is a multi-step procedure with a gotcha (Edit tool, not `agent-doc write`) that's easy to forget. Currently only encoded in auto memory.

**What it would cover:**
1. Read full exchange content
2. Summarize: preserve decisions, key facts, open items
3. Archive original to `.agent-doc/archives/<hash>-<timestamp>.md`
4. Use **Edit tool** (not `agent-doc write`) to replace content — write appends, compaction requires replacement
5. Add archive pointer
6. Commit via `agent-doc commit`

Good fit for a runbook because:
- Skill-executed procedure (not binary)
- Specific trigger ("compact exchange")
- Edit-not-write gotcha is easy to forget
- Referenced from memory but not always needed in context

Want me to create `.ai/runbooks/compact-exchange.md`?
Sure...Should this runbook be installed with agent-doc? Should we make this deterministic? Or should we point this case out in the documentation so people can adjust for their own practice?
### Re: Compact exchange — install, deterministic, or document?

Three options:

| Option | Pro | Con |
|--------|-----|-----|
| **Install with agent-doc** | Users get it automatically | agent-doc owns `.ai/runbooks/` — contradicts vendor-neutral convention |
| **Make deterministic** | No LLM needed for mechanical steps | Summarization requires LLM — binary can't do it alone |
| **Document it** | Users adapt to their practice | Easy to miss; Edit-not-write gotcha bites people |

**Recommendation: Hybrid deterministic + document.**

`agent-doc compact` already handles the mechanical parts (archive, replace, commit). The skill provides the summary content. Adding a runbook in agent-loop's `.ai/runbooks/` is fine as a project-specific convenience, but agent-doc shouldn't install runbooks — it doesn't own that namespace.

For documentation: mention the compact workflow in agent-doc README. Let users create their own runbooks.

Can we have the runbook be installed in the agent-doc skill...`agent-doc skill install`?

Creating `.ai/runbooks/compact-exchange.md` now as an agent-loop project runbook (not an agent-doc install artifact).
### Re: Install runbook via `agent-doc skill install`

Clean approach. `agent-doc skill install` already installs SKILL.md — it could also install runbooks into `.ai/runbooks/`.

```
agent-doc skill install
  → .claude/skills/agent-doc/SKILL.md    (existing)
  → .ai/runbooks/compact-exchange.md     (new)
```
We should have this instead...
→ .claude/skills/agent-doc/.ai/runbooks/compact-exchange.md     (new)

**Why skill install, not init:**
- `skill install` = agent instructions — runbooks are instructions
- `init` = project infrastructure (`.agent-doc/` dirs)
- Runbook stays in sync with SKILL.md versions
- Users without the skill don't get unwanted runbooks

**Implementation:** Bundle runbook content in the binary (like SKILL.md). `skill install` writes both. Version-check logic extends to cover runbooks.

Want me to implement this?
### Re: Runbooks under skill directory

Good correction. Keeping it under `.claude/skills/agent-doc/` scopes it to the skill:

```
.claude/skills/agent-doc/
├── SKILL.md
└── .ai/runbooks/
    └── compact-exchange.md
```

Benefits:
- Scoped to agent-doc skill, not the project
- No collision with project-level `.ai/runbooks/` (precommit, prerelease)
- `skill install --reload compact` updates both atomically
- SKILL.md references it via relative path from skill base directory

SKILL.md instruction: "When user requests 'compact exchange', read `.ai/runbooks/compact-exchange.md` relative to this skill's base directory."
Should this be in `.ai/runbooks` or in `runbooks` relative to the skill directory?
### Re: `.ai/runbooks` vs `runbooks` in skill dir

`runbooks/` (no `.ai/` prefix) is cleaner inside a skill directory:

```
.claude/skills/agent-doc/
├── SKILL.md
└── runbooks/
    └── compact-exchange.md
```

Rationale:
- `.ai/` is a project-root namespace — makes sense at repo root where it's vendor-neutral
- Inside `.claude/skills/agent-doc/`, you're already tool-specific — `.ai/` adds no value
- `runbooks/` is simpler, shorter

Convention:
- **Project-level:** `.ai/runbooks/` (vendor-neutral, repo root)
- **Skill-level:** `runbooks/` (relative to skill base dir)

Should we have `.runbooks` instead of `.ai/runbooks` relative to the project root by convention?
I suppose if we want vendor neutral configs, we should keep `.ai/`...
### Re: `.runbooks` vs `.ai/runbooks` at project root

| Convention | Pro | Con |
|-----------|-----|-----|
| `.runbooks/` | Simple, no nesting | One more top-level dotdir; doesn't scale |
| `.ai/runbooks/` | Groups all AI config; extensible | Extra nesting |

**Stick with `.ai/runbooks/`.** The `.ai/` namespace has room to grow — evals, prompts, agent config. `.runbooks/` is a dead-end. If you later add `.ai/evals/`, having `.runbooks/` AND `.ai/` at root is messier than everything under `.ai/`.

The blog post establishes `.ai/` as the cross-harness namespace, with `runbooks/` as the first subdirectory.
### Re: Vendor-neutral `.ai/` confirmed

Exactly. `.ai/` doesn't belong to any specific tool — that's the point. Decisions settled:

- **Project-level:** `.ai/runbooks/` (vendor-neutral)
- **Skill-level:** `runbooks/` (relative to skill base dir)
- **agent-doc installs skill-scoped runbooks only** (via `skill install`)

All naming conventions locked in. Ready to implement the `skill install` bundling whenever you want to proceed.
go
### Re: Implemented `skill install` runbook bundling

Changes to `src/agent-doc/src/skill.rs`:
- `BUNDLED_RUNBOOKS` array with `include_str!` for `compact-exchange.md`
- `install_runbooks()` — writes to `.claude/skills/agent-doc/runbooks/`, idempotent
- All install paths now also install runbooks
- Added `install_creates_runbooks` test

Source: `src/agent-doc/runbooks/compact-exchange.md`
Install target: `.claude/skills/agent-doc/runbooks/compact-exchange.md`

Binary rebuilt and installed. `make check` passes (38 tests). Verified end-to-end.
release...make sure the prerelease runbook is used
### Re: Release v0.31.5 — prerelease runbook in progress

Following `.ai/runbooks/prerelease.md`:

**Common checks:**
- [x] `make check` — 38 tests pass
- [x] Version bumped: Cargo.toml + pyproject.toml → 0.31.5
- [x] VERSIONS.md updated with 0.31.5 entry
- [x] SKILL.md frontmatter version → 0.31.5
- [x] No secrets in diff

**Targets: cargo, pypi, github-release, binary-install**

Committing submodule changes now, then proceeding with publish.
### Re: v0.31.5 released

Following `.ai/runbooks/prerelease.md`:

| Step | Status |
|------|--------|
| `make check` | 38 tests pass |
| Version bump (Cargo.toml + pyproject.toml) | 0.31.5 |
| VERSIONS.md | Updated |
| SKILL.md frontmatter | 0.31.5 |
| cargo publish | Published to crates.io |
| PyPI | Skipped — needs manylinux Docker build (glibc too recent) |
| GitHub Release | https://github.com/btakita/agent-doc/releases/tag/v0.31.5 |
| `cargo install --path .` | Installed to ~/.cargo/bin |
| Binary verify | `agent-doc --version` → 0.31.5 |

If skills are not loaded unless they are used, is there any advantage to having a runbook?
### Re: Skills vs runbooks advantage

| | Skill (SKILL.md) | Runbook |
|---|---|---|
| **Loaded when** | User invokes `/agent-doc` | Agent reads file during a procedure |
| **Size** | Large (~5KB+) — full workflow | Small (~500B) — checklist |
| **Purpose** | Complete workflow orchestration | Step-by-step procedure for a specific task |

**The real advantage:** Skills are monolithic — you can't load "just the compact part." Runbooks are modular — load exactly the procedure needed. SKILL.md is already ~5KB; adding every procedure would bloat it.

**Your point is valid though:** if SKILL.md is in context and the procedure is small, inlining costs nothing extra. Runbooks save tokens only when the procedure is (a) rarely needed or (b) significantly large.

For `compact-exchange.md`: ~30 lines, rarely triggered. Keeping it as a runbook saves those tokens from every `/agent-doc` invocation. Marginal but correct.

Two things:
- `agent-doc compact` should **always** default to overriding `agent:exchange` to replace mode unless it is overridden itself.
- Does `agent-doc compact` with append or prepend even make sense? Probably not.
### Re: `agent-doc compact` already uses replace mode

Good news — no change needed. `compact.rs:174` uses `comp.replace_content()` directly, bypassing the patch pipeline entirely. The `patch=append` attribute is irrelevant.

- **`agent-doc compact`** — always replaces. Correct as-is.
- **Runbook's Edit tool instruction** — for when the *skill* does compaction manually. `agent-doc write` would append; Edit replaces.
- **Append/prepend for compact** — nonsensical, but not a bug since the binary never uses patch mode for compaction.

Two valid paths for compaction:
1. `agent-doc compact <FILE>` — binary handles archive + replace + snapshot atomically
2. Skill follows runbook manually — Edit tool for replace, `agent-doc commit` for git

compact-exchange.md in the skill
> - **IMPORTANT:** Use the Edit tool, NOT `agent-doc write`

Is this valid? Seems like we should **always** use `agent-doc compact`
### Re: Runbook updated — always use `agent-doc compact` (HEAD)

You're right. Removed the Edit tool instruction from the runbook. Simplified to:

1. Read exchange, summarize decisions/facts/open items
2. `agent-doc compact <FILE>` (archives + replaces + snapshot atomically)
3. `agent-doc commit <FILE>`

No Edit tool, no manual archive. The binary handles everything. Updated in:
- `src/agent-doc/runbooks/compact-exchange.md` (source, bundled in binary)
- `.ai/runbooks/compact-exchange.md` (project copy)
- `.claude/skills/agent-doc/runbooks/compact-exchange.md` (installed via `skill install`)

Binary rebuilt and installed.
<!-- agent:boundary:e38d142e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release v0.31.5
- [ ] PyPI publish (needs manylinux Docker build)
<!-- /agent:pending -->
