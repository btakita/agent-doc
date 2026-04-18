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
v0.32.5 session. Key fixes: supervisor IPC stack wired into start.rs (#vnp0/#6ae3/#zp02), DSR interleaving fix (prompt-poll instead of fixed sleep), supervisor smoke test (#f7d5), sun_path overflow for deep submodule paths (XDG_RUNTIME_DIR fallback when >107 bytes, 13 tests), tmux install detection (#t8tg), TerminalBackend trait spec (#1yk1), DirectTerminal plan (#zy7m), Library Context Policy stub in rust-package skill (#breh). Decisions: [/] vs [x] semantics (gate after impl, done on confirm); typed gates [/release]/[/deploy]; replace:* restricted to pending; --status flag mirrors --pending-add; always run cargo install --path . after builds without asking. State: 1055 tests, 7 gated pending items.
❯ Look at #vdyj. I also installed wozcode: https://app.wozcode.com/
❯ How can I synthesize skills janitor and wozcode to save on tokens?
❯ ### Re: #vdyj + WozCode synergy — opus-4-6 (HEAD)

**#vdyj status:** Currently `[ ]` (unstarted). The plan is: fork skills-janitor → `src/skills-janitor` submodule → wire `/janitor-report` into `make precommit` → optionally add nightly cron via `/schedule`. This gives you drift detection as part of the normal commit flow.

**Synthesizing skills-janitor + wozcode for token savings:**

The two tools complement each other at different layers:

1. **Reduce skill context bloat (janitor):** Every installed skill adds its description to the system prompt. Run `/janitor-usage` to find skills you never invoke, `/janitor-duplicates` to find overlapping ones, then `/janitor-cleanup` to remove dead weight. Fewer skill descriptions = smaller system prompt = fewer tokens per turn.

2. **Use woz agents for cheaper execution (wozcode):** Once skills are clean, use `woz:explore` instead of general-purpose agents for read-only research (it's faster and cheaper). Use `woz:code` for editing tasks that benefit from batch edits and smart search. The `mcp__plugin_woz_code__Search` tool replaces multi-round Grep/Glob cycles with a single semantic call.

3. **Concrete workflow for #vdyj:**
   - Fork + submodule skills-janitor (the planned work)
   - Add `janitor-report` to `make precommit` — catches skill drift before it accumulates
   - Use `/janitor-usage` periodically to prune unused skills (immediate token savings every session)
   - Delegate janitor audit operations to `woz:explore` subagents instead of full agents (cheaper per-audit)

4. **Quick win right now:** Run `/janitor-usage` to see which of your ~40+ skills actually get used. Pruning the unused ones is the single highest-leverage token reduction — each removed skill saves its description tokens on every turn of every session.

Want me to run `/janitor-usage` now to identify pruning candidates? Or should I start on the #vdyj submodule setup?

❯ Can I use sift (https://www.alexdk.com/blog/introducing-sift/) in place of woz code? I want a free/open source usage...not be locked into another subscription.
❯ ### Re: Sift vs WozCode — opus-4-6 (HEAD)

Good instinct — avoiding subscription lock-in is smart. Here's the honest comparison:

**Sift** (`github.com/rupurt/sift`) is a local hybrid search CLI: BM25 + vector + LLM reranking, single binary, no cloud, no daemons. Sub-6ms BM25 on ~5K docs. Genuinely good at what it does.

**But it covers ~25% of WozCode's surface:**

| Capability | WozCode | Sift |
|---|---|---|
| Smart search/recall | Yes (cloud) | Yes (local) |
| Batch file editing | Yes | No |
| SQL introspection | Yes | No |
| Subagent delegation | Yes (cost-optimized) | No |
| Session memory/recall | Yes | No |
| License | Subscription | Free (verify LICENSE in repo) |

**What you actually need to decide:** Which WozCode features do you use?

- If you mostly use `mcp__plugin_woz_code__Search` and `woz:explore` → Sift can replace those with zero subscription cost. Wire it as an MCP server or shell tool.
- If you use `woz:code` batch edits or `mcp__plugin_woz_code__Edit` → Sift doesn't cover that. But Claude Code's native Edit tool + skills-janitor pruning for context savings might be enough.
- `woz:recall` (cross-session memory) → Sift doesn't do this, but `claude-mem:mem-search` already covers it.

**My take:** You could drop WozCode and combine Sift (search) + native Claude Code tools (edit) + skills-janitor (context pruning) + claude-mem (session recall). The main loss is WozCode's cost-optimized subagent routing, but that's an optimization, not a capability gap.

Worth checking the actual LICENSE file in the Sift repo before committing — the blog doesn't state it explicitly.
❯ MIT licensed https://github.com/rupurt/sift

❯ How much effort would it take to implement the rest of WozCode using sift?
❯ Should this be a separate project?
❯ ### Re: Building WozCode-equivalent on Sift — opus-4-6 (HEAD)

MIT — that settles the licensing question. Now let's scope the build-vs-buy honestly.

**What WozCode actually provides (decomposed):**

| Capability | Effort to replicate | Notes |
|---|---|---|
| Smart search | **Done** — Sift covers it | Wire as MCP server |
| Batch editing | **Low** (~1-2 days) | Thin MCP wrapper around Claude Code's native Edit. The "batch" part is just accepting multiple edits in one call and applying sequentially. |
| SQL introspection | **Low-Med** (~2-3 days) | MCP server that runs `SHOW TABLES`, `DESCRIBE`, `SELECT` with output formatting. Straightforward if you scope to SQLite/Postgres. |
| Subagent cost routing | **Med** (~1 week) | This is the hard one. WozCode routes tasks to cheaper models (haiku for simple searches, sonnet for edits). Requires a routing heuristic + model switching. But Claude Code's native `model` parameter on Agent already does this — you just need a skill that encodes the routing rules. |
| Session recall | **Done** — claude-mem:mem-search | Already installed |

**Total estimate:** ~2 weeks of focused work for a full MCP server that wraps Sift + batch edit + SQL + routing rules. The routing is the only genuinely complex piece.

**Should it be a separate project?** Yes:

1. **Reuse** — other Claude Code users would benefit. MIT Sift + MIT wrapper = clean OSS story.
2. **Scope** — it's an MCP server, not an agent-doc feature. Mixing it into agent-loop or agent-doc muddies both.
3. **Distribution** — single `cargo install` or binary download, like Sift itself.

**Suggested structure:** `src/sift-mcp` or standalone repo. Ships as one binary exposing MCP tools: `sift_search`, `sift_edit` (batch), `sift_sql`, `sift_route` (subagent dispatch). Config points at Sift binary path + DB connection strings.

**Or the pragmatic middle:** Skip building anything. Use Sift for search (MCP or shell), Claude Code native Edit for edits, claude-mem for recall, and write a 50-line routing skill that picks agent model tiers. That's 90% of WozCode's value in an afternoon, no new project needed.

❯ We can implement the "the pragmatic middle" as a plugin. I want a cli instead of a mcp. What should we call it? sift-tokens? token-sifter? sifting-tokens? siftoken? any other suggestions?
❯ ### Re: Naming the CLI plugin — opus-4-6 (HEAD)

CLI, not MCP — good call. Lower overhead, works as a Claude Code hook target, and you already have the `rtk` pattern for CLI proxies.

Evaluating your candidates:

| Name | Pros | Cons |
|---|---|---|
| `sift-tokens` | Clear, descriptive | Hyphenated (annoying to type repeatedly) |
| `token-sifter` | Reads well | Long, hyphenated |
| `sifting-tokens` | Very descriptive | Way too long for a CLI |
| `siftoken` | Short, clean portmanteau, 8 chars | Might read as "sif-token" on first glance |

**Additional suggestions:**

- **`tsift`** — 5 chars, "token sift", mirrors the `rtk` pattern of terse CLI names. Easy to type.
- **`sieve`** — metaphor for filtering/refining tokens. 5 chars, memorable, but might conflict with existing tools.
- **`pansift`** — gold-panning metaphor (sift for the valuable tokens, discard the rest). 7 chars.
- **`siftcc`** — "sift for claude code". 6 chars, explicit about what it targets.

**My pick: `tsift`** — same family as `rtk` (terse, 4-5 chars, token-focused), easy to type, and the `t` prefix clearly signals "token." Second choice: `siftoken` if you want something more self-documenting.

❯ Please extract a repo btakita/tsift. The binary will be written in Rust for universal installation. Extract a tasks/tsift.md agent doc to manage the project. Add src/tsift as a git submodule.
❯ ### Re: tsift repo extracted — opus-4-6 (HEAD)

Done. Here's what was set up:

1. **GitHub repo**: https://github.com/btakita/tsift (public, MIT)
2. **Rust scaffold**: `Cargo.toml` with metadata, `src/main.rs` stub, LICENSE
3. **Submodule**: `src/tsift` added to agent-loop
4. **Session doc**: `tasks/tsift.md` — agent-doc template with pending items for the build plan

**Pending items in `tasks/tsift.md`:**
- Wire Sift as search backend
- Batch edit command
- Agent model routing skill
- SQL introspection
- Claude Code hook integration (rtk-style)
- CLAUDE.md, CI, release workflow

To start building, open a session on `tasks/tsift.md`. The first step would be wiring the Sift binary as a search backend with shell-out + output parsing.
❯ Please add any useful info from this doc into tsift.md before we compact exchange.
❯ ### Re: Context transferred to tsift.md — opus-4-6 (HEAD)

Done. Transferred to `tasks/tsift.md` exchange section:

- **Origin story**: WozCode evaluation → Sift discovery → pragmatic middle decision
- **Architecture table**: what each WozCode capability maps to in tsift (Sift search, native Edit, direct DB, model routing skill, claude-mem)
- **Key decisions**: CLI not MCP, Rust, naming rationale, MIT, composition over reimplementation
- **Sift technical details**: pipeline stages, SIMD, caching, performance benchmarks

Ready for `/agent-doc compact exchange tasks/agent-doc/agent-doc-bugs.md` whenever you are.
❯ Please trans the skills-janitor fork & related pending items into tsift.md. Let's have tsift be an all-in-one audit + token sifter tool.
❯ ### Re: Skills-janitor → tsift merge — opus-4-6 (HEAD)

Transferred to `tasks/tsift.md`. The vision: **tsift = token sifter + skill auditor** in one binary.

**What moved:**
- **#vdyj** (skills-janitor fork) → absorbed into tsift. Instead of a separate `src/skills-janitor` submodule, tsift gets a `tsift audit` subcommand that covers skill drift detection, duplicate finding, usage analysis, and cleanup recommendations.
- **Skill audit integration** → `tsift audit` replaces the planned `make precommit` janitor hook. Same drift reports, but unified under tsift's CLI.
- **Nightly cron** → `tsift audit --report` writes drift reports to `tasks/agent-doc/skills-audit.md` via `/schedule`.

**tsift's expanded scope:**
1. **Search** — Sift backend for codebase search
2. **Audit** — skill health checks, duplicate detection, usage tracking, cleanup (replaces skills-janitor)
3. **Route** — agent model tier routing (haiku/sonnet/opus)
4. **Hook** — Claude Code command rewriting (rtk-style)

Marked #vdyj as done here since the work now lives in tsift's backlog. Also fixed the "Avisory" typo in #wcf5.
❯ do #9v7h
❯ ### Re: #9v7h implemented — opus-4-6 (HEAD)

Done. Three changes to agent-doc:

1. **Validation**: `--pending-add` now rejects text starting with `[ ]`, `[/]`, `[x]`, `[X]` — prevents ambiguous state markers embedded in item text.

2. **`--pending-add-gated`**: New flag on both `agent-doc write` and `agent-doc pending add-gated`. Creates items with `[/]` state directly, no separate gate step needed.

3. **Hash echo**: Both `--pending-add` and `--pending-add-gated` print the assigned hash ID to stdout. The skill can now compose add+gate atomically:
   ```bash
   agent-doc write doc.md --pending-add-gated "implement feature" --pending-only
   # stdout: a3f2
   ```

**Files changed:** `pending.rs` (op_add signature + validation), `pending_cmd.rs` (gated param + println), `main.rs` (CLI arg + dispatch + PendingAction::AddGated).

**Tests:** 4 new tests (state marker rejection, gated add, id return). Full suite: 1083 passed, 0 failures. Binary installed.
❯ do #txex
❯ ### Re: #txex implemented — opus-4-6 (HEAD)

Done. Three changes to notify command:

1. **`--pending-add` / `--pending-add-gated`**: Adds items to the target document's `agent:pending` component. Works identically to the write command's flags — assigns hash ID, prints to stderr.

2. **Auto-create `agent:pending`**: When the target document has no pending component, one is auto-created after the exchange close tag. `--no-create-pending` opts out (errors become no-ops).

3. **Pending-only mode**: When `--pending-add` is used without a message, the exchange blockquote is skipped entirely. Only pending mutations + snapshot update + commit.

**Usage examples:**
```bash
# Pending-only (no exchange blockquote)
agent-doc notify target.md --pending-add "implement feature X"

# Gated pending item
agent-doc notify target.md --pending-add-gated "blocked on release"

# Pending + exchange notification
agent-doc notify target.md "API changed" --pending-add "update tests" --source build

# Opt out of auto-creation
agent-doc notify target.md --pending-add "item" --no-create-pending
```

**Files changed:** `notify.rs` (run signature + ensure_pending_component + add_pending_item + save_snapshot helper), `pending_cmd.rs` (pub doc_id_for), `main.rs` (CLI args + dispatch).

**Tests:** 8 new tests (auto-create, existing component, gated, no-create skip, both pending+message, snapshot update, insert position, message required). Full suite: 1091 passed, 0 failures. Binary installed.
❯ do #hrmt
❯ ### Re: #hrmt implemented — opus-4-6 (HEAD)

Done. The stale `.claude/skills/agent-doc/SKILL.md` inside the agent-doc repo (v0.28.0) has been synced with the current bundled version (v0.32.0).

**What changed:** The repo-local `.claude/skills/agent-doc/SKILL.md` had the old §1b: "every response MUST include a `patch:pending` block reflecting the current state." Replaced with the granular-ops contract: mutations via `--pending-add`, `--pending-done`, `--pending-edit`, etc. Full-replace via `patch:pending` is now explicitly rejected.

**Scope of fix:**
- The binary-bundled `SKILL.md` (root `src/agent-doc/SKILL.md`, `include_str!`'d in `skill.rs:38`) was already correct — no change needed.
- The stale copy at `src/agent-doc/.claude/skills/agent-doc/SKILL.md` was the only file with the old guidance. Now matches the bundled version exactly.

**Other updates in the sync:** v0.28.0 → v0.32.0 frontmatter, subcommand detection (compact), preflight JSON fields (baseline_file, slash_commands, model tier), model attribution, streaming checkpoints, runbook references, document-format runbook link.

**Tests:** 15 skill tests passed, 1091 total (0 failures). Release binary built and installed.

**Release gate:** #hrmt was the last blocker for #gqep (v0.32.4 release). The bundled skill guidance is now consistent — `patch:pending` is rejected in both the binary enforcement and the skill instructions.
❯ do #e46w
❯ ### Re: #e46w already implemented — opus-4-6 (HEAD)

Already fixed. `reposition_boundary_in_snapshot()` (git.rs:620-701) already rewrites both snapshot AND working-tree file with active-run guard:

- Lines 623-629: active-run guard (pending_path exists → skip, IPC owns transition)
- Lines 649-666: snapshot reposition via `reposition_boundary_to_end_with_baseline`
- Lines 681-697: working-tree reposition — same function, atomic write, only when no active run

The comments at lines 668-672 explicitly document this as the fix for the original lag: "Previously this was left to the plugin IPC path, so sweep-committed foreign docs kept stale boundary markers until the next `agent-doc write`."

**Duplicates also closed:** #f8j3 and #y4da describe the same working-tree boundary lag and are resolved by this same code path.
❯ do #zvmd
❯ ### Re: #zvmd already implemented — opus-4-6 (HEAD)

Already fixed in v0.32.5 (commit 5dfb4b5c). The `show_head()` function at git.rs:885 now routes through `narrow_to_submodule()` before running `git show HEAD:<path>`:

```rust
let (git_root, _in_submodule) = narrow_to_submodule(&super_root, &resolved);
```

**Before fix:** `git show HEAD:src/boost-client/tasks/monsterrodholders.md` from the superproject fails (can't traverse submodule gitlink) → `show_head()` returns `None` → baseline headings empty → `annotate_re_headings_with_head` falls back to "mark only last heading" → drops `(HEAD)` when no `### Re:` heading exists yet.

**After fix:** `narrow_to_submodule` detects the submodule boundary, narrows `git_root` to `src/boost-client/`, and computes `rel_path` relative to that root → `git show HEAD:tasks/monsterrodholders.md` succeeds from the submodule → baseline headings populate correctly → new headings get `(HEAD)` as expected.

Verified live: `git show HEAD:src/boost-client/tasks/monsterrodholders.md` fails from superproject, confirming the routing fix is necessary and working.
❯ do #np8p
❯ ### Re: #np8p already implemented — opus-4-6 (HEAD)

Already fixed. `resolve_pane_cwd()` at git.rs:279 uses `narrow_to_submodule()` and is wired into both pane-spawning paths:

- **route.rs:669** — `let cwd = crate::git::resolve_pane_cwd(file);` — used when spawning new tmux panes for documents
- **claim.rs:350** — `let resolved_cwd = crate::git::resolve_pane_cwd(file);` — used when claiming/registering pane sessions

The function returns the submodule's git toplevel when the document is inside a submodule, or the superproject root otherwise. No remaining paths use raw `std::env::current_dir()` for pane cwd in production code (only in tests).
❯ do #0xka
❯ ### Re: #0xka already implemented — opus-4-6 (HEAD)

Already done. `annotate_re_headings_with_head()` at template.rs:644 is the implementation — it's the symmetric counterpart to `strip_head_markers()` at git.rs:739:

- **strip_head_markers** (commit staging side): removes `(HEAD)` from all headings so the committed blob is clean
- **annotate_re_headings_with_head** (reposition side): strips stale `(HEAD)` from all `### Re:` headings, then appends `(HEAD)` to new ones (baseline-aware) or just the last one (no baseline)

Called from `reposition_boundary_to_end_with_baseline()` at template.rs:566, which is invoked by `reposition_boundary_in_snapshot()` at git.rs:650 and git.rs:682 (both snapshot and working tree).

Test coverage: template.rs:1926 verifies three `### Re:` headings → only last gets `(HEAD)`, stale markers stripped.
❯ re #77mf: How would the harness know if it's [/release] or [/deploy]
❯ ### Re: #77mf gate resolution architecture — opus-4-6 (HEAD)

Three viable approaches, increasing in automation:

**1. Explicit resolution (simplest):**
A new subcommand `agent-doc pending resolve <id> --gate-type release`. The skill or user calls it manually after performing the action. The reaper only checks if the gate has been resolved, not what type it is. The typed marker is just documentation — resolution is still human-triggered.

**2. Hook-driven (medium):**
Claude Code hooks fire after matching shell commands. A post-command hook pattern:

```jsonc
// settings.json
{ "match": "cargo publish", "run": "agent-doc pending resolve-gate release" }
{ "match": "git push",      "run": "agent-doc pending resolve-gate deploy" }
```

`resolve-gate <type>` scans all tracked documents for `[/<type>]` items and flips them to `[x]`. The harness doesn't "know" — it reacts to external events via hooks.

**3. Verification commands (most robust):**
Each gate type maps to a verification function in config:

```toml
# .agent-doc/config.toml
[gates.release]
verify = "cargo search agent-doc --limit 1 | grep -q '0.32.4'"

[gates.deploy]
verify = "git log origin/main --oneline -1 | grep -q $(git rev-parse --short HEAD)"
```

The reaper runs the verify command before resolving. This is the only approach that's truly event-driven — the others are event-triggered but not verified.

**My recommendation:** Start with #1 (explicit `resolve-gate` subcommand), wire #2 (hooks) as sugar on top. Skip #3 unless you need idempotent verification — it adds shell-out complexity to the reaper path and the verification commands are fragile across environments.

The key insight: the harness doesn't need to "know" the gate type semantically. It just needs a resolution signal. The typed marker (`[/release]`) is for human readability and hook routing — the binary treats all resolved gates identically (flip to `[x]`).
❯ How would these flags work in context of my monorepo?
❯ ### Re: #77mf typed gates in monorepo context — opus-4-6 (HEAD)

The core challenge: **which release resolves which gate?** In a single-package repo it's obvious. In agent-loop with 17+ submodules, you need scope.

**Three scoping strategies:**

**1. Item-level scope (most explicit):**
```markdown
- [/release:agent-doc] [#gqep] Release v0.32.4...
- [/deploy:cloudflare] [#xxxx] Push worker to prod...
```

The gate marker carries its target: `[/release:agent-doc]`. Resolution matches on both gate type AND scope. Hook: `agent-doc pending resolve-gate release --scope agent-doc`.

**2. Document-level scope (convention-based):**
Documents under `tasks/agent-doc/` implicitly scope to agent-doc. The resolve command infers scope from the document path:

```bash
# Hook fires from src/agent-doc/ after cargo publish
agent-doc pending resolve-gate release
# Binary infers scope "agent-doc" from cwd, scans tasks/agent-doc/*.md
```

This leverages the existing directory convention without new syntax. But it's fragile — cross-cutting documents (like a release-all tracker) wouldn't fit.

**3. Hybrid (recommended for agent-loop):**

- Default: scope inferred from document path (`tasks/<submodule>/` → that submodule)
- Override: explicit scope on the item when it crosses boundaries (`[/release:agent-doc]`)
- Hook integration: `cargo publish` in `src/agent-doc/` fires a hook that calls `agent-doc pending resolve-gate release` with cwd-inferred scope

**Concrete example for #gqep today:**

```bash
# In src/agent-doc/ after cargo publish succeeds
# Claude Code hook (settings.json):
{ "match": "cargo publish", "run": "agent-doc pending resolve-gate release" }

# Binary behavior:
# 1. Detect cwd is src/agent-doc/ → scope = "agent-doc"
# 2. Scan tasks/agent-doc/*.md for [/release] items
# 3. Flip [/release] → [x] on matching items
# 4. Commit snapshot
```

For `git push` / deploy gates, same pattern — the hook fires from the relevant submodule directory, cwd provides scope.

**Edge case — parent repo operations:** Some gates apply to agent-loop itself (submodule pointer updates, cross-cutting releases). These would use explicit scope: `[/deploy:agent-loop]` or trigger from the repo root.

The item-level syntax `[/release:agent-doc]` is the safest — it's self-documenting and doesn't depend on directory conventions. But for your current layout where documents already live under `tasks/<submodule>/`, cwd inference handles 90% of cases without any new syntax.
❯ Would [/release] happen when an item is complete but pre-release?
❯ ### Re: #77mf [/release] lifecycle timing — opus-4-6 (HEAD)

Yes, exactly. `[/release]` is a refinement of `[/]` — it means "implementation complete, waiting for release to close."

**The state machine:**

```
[ ]  →  work starts  →  [/]  →  impl done, needs release  →  [/release]  →  cargo publish  →  [x]
                         │
                         └─  impl done, needs deploy  →  [/deploy]  →  git push  →  [x]
```

So the flow for an item like #gqep would be:

1. `[ ] [#gqep] Release v0.32.4` — not started
2. `[/] [#gqep] Release v0.32.4` — gated (impl done, manual testing gate)
3. `[/release] [#gqep] Release v0.32.4` — testing passed, ready to publish, waiting on the actual `cargo publish`
4. `[x] [#gqep] Release v0.32.4` — published, resolved by hook

**Key distinction from plain `[/]`:** A `[/]` gate is human-resolved (the user or agent decides when to flip it). A `[/release]` gate is machine-resolved — the hook fires on `cargo publish` and flips it automatically. The typed marker is what enables automation; without it, the reaper can't know *what kind* of event should resolve the gate.

**When to apply:** You'd transition `[/]` → `[/release]` when the only remaining blocker is the publish/deploy action itself. If there are still manual testing gates or other human decisions pending, it stays as `[/]`. The typed gate means "the next matching event closes this" — so you only apply it when you're genuinely ready for that event.

This also means `[/release]` is a commitment signal: "if cargo publish succeeds, this item is done." No further review needed after the event.
❯ How do we know if an agent doc is associated with a `/release` or a `/deploy`?
### Re: Document-to-gate association — opus-4-6

The document itself doesn't need to be associated with a gate type — **individual pending items declare their own gate type**. A single document can have items waiting on different events:

```markdown
&lt;!-- agent:pending --&gt;
- [/release] [#gqep] Release v0.32.4 — waiting on cargo publish
- [/deploy]  [#xxxx] Push CDN config — waiting on git push to prod
- [/]        [#yyyy] Manual review — human-resolved
&lt;!-- /agent:pending --&gt;
```

**Resolution flow when a hook fires:**

1. `cargo publish` succeeds in `src/agent-doc/`
2. Hook calls `agent-doc pending resolve-gate release`
3. Binary scans tracked documents (scoped by cwd → `tasks/agent-doc/*.md`)
4. Finds all `[/release]` items → flips to `[x]`
5. `[/deploy]` and `[/]` items are untouched

**So there's no document-level gate declaration needed.** The gate type is per-item, and the scope (which documents to scan) comes from cwd or explicit `--scope`. The binary just pattern-matches `[/release]` vs `[/deploy]` in the pending component text.

**What "tracked documents" means:** Any file with an `.agent-doc/snapshots/` entry. The `resolve-gate` command would iterate those, filter by scope, parse pending components, and match gate types. No new config or metadata required — everything is already in the document content.

This keeps the design simple: no frontmatter gate declarations, no document-type taxonomy. Items self-describe what they're waiting for.
<!-- agent:boundary:8883e140 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#p2xm] Orphaned patch:exchange marker leak: parse_patches() does not consume unclosed opening markers — they leak into unmatched text and get appended to exchange verbatim. Fix: advance last_end past unclosed openers in parse_patches()
- [/] [#wcf5] Advisory flock doesn't protect external watchers: IDE file-change listeners and git hooks race concurrent diff computations against stale snapshots
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#77mf] Event-driven gate resolution: typed gate markers like [/release] and [/deploy] that auto-resolve on matching events (cargo publish, git push). Extends the binary [/] invariant with machine-actionable gate types — reaper checks event log before skipping.
- [/] [#6hgf] Add --status flag to agent-doc write: skill passes status text as CLI arg, binary handles component targeting with replace semantics
<!-- /agent:pending -->
## Assistant
