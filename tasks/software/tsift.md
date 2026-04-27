---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
## Exchange Summary (compacted)

**Key decisions:**
- **graphify boundary rule:** tsift owns deterministic, AST-derived graph operations. graphify owns LLM-derived semantic operations. Interop via `tsift graph --json`. (#bamc closed)
- **Presets:** `#spec-test-commit-push` and `#spec-test-build-install-commit-push` established for common workflows.
- **#9jc8 (graphify integration: entity extraction) is graphify-owned.** tsift's role (structured JSON export) is already complete. #zv4m (HTML viz) and #rd2f (semantic similarity) are also graphify-side.
- **`tsift summarize` is opt-in** — high-value for large codebases and exploration-heavy sessions; skip for small repos, heavy active dev, or no API key. Free commands (search, explain, graph) always apply.
- **`tsift status` is the single session-start check** — reports index freshness, summary state, and a machine-parseable `use:` list so agents know exactly which commands are available.
- **Search default strategy: lexical** (not hybrid). Hybrid/vector available via explicit `--strategy hybrid`. (#srch fix)
- **Auto-reindex hook scope:** per-project, not workspace-wide. Install in each submodule you actively develop in.
- **pending_capture_guard enforcement:** plan for pre-commit gate in `finalize` (phase 1) and `plan` emitting `pending_mutations` (phase 2) transferred to agent-doc-feat.md backlog.

**Completed work:**
- **#cdet** — Louvain community detection: `tsift communities`. `btakita/tsift@400c199`.
- **#gpth** — Graph path queries: `tsift path` + `tsift explain`. `btakita/tsift@6ad5983`.
- **#nkec** — Skill drift detection: `tsift audit`. `btakita/tsift@a4235b7`.
- **#865b** — Duplicate skill detection via Jaccard similarity. `btakita/tsift@59e1e79`.
- **#ay3j** — Usage tracking (`--usage`), **#t3pq** — cleanup recommendations (`--cleanup`), **#dt80** — report output (`--report`), **#cs4r** — `tsift lint`. All in `btakita/tsift@056dbac`.
- **#summ** — `tsift summarize` — cached LLM analysis with batch extraction and local SQLite cache. `btakita/tsift@5801090`.
- **#hidx** — Auto-reindex hook + `--exit-code` flag. `btakita/tsift@1c712f7`.
- **#cmdx** — `tsift init` — idempotent AGENTS.md/CLAUDE.md injection. `btakita/tsift@d9af2b0`.
- **#stat** — `tsift status` — session-start health check with `use:` list. `btakita/tsift@9f4a111`.
- **#srch** — Search hang fix: default to lexical strategy + scope sift path to submodule. `btakita/tsift@882421d`.
- **#wrka** — closed (workaround obsolete after #srch fix).
- **#mext** — closed (single-command Makefile wrapper, superseded by `tsift init` + `tsift status`).
- Test count progression: 125 → 139 → 153 → 166 → 174 → 197 → 211 → 222 → 229 → 236.

**Research notes:**
- Alternative tools to graphify: LightRAG, GraphRAG, sift-kg (Python, CLI-ready); EdgeQuake, Rig, petgraph (Rust, incomplete pipelines). No Rust tool ships the full ingest→extract→cluster→export pipeline as a CLI today.
- Token savings from `tsift summarize`: ~78% per bug-fix session (~25k → ~5.5k tokens), ~$73/month at Opus pricing for 8 sessions/day.
- Markdown is first-class: indexing extracts headings + code block languages via tree-sitter-md; search + lint work on .md/.mdx; call-graph intentionally skipped.

❯ Does `tsift init` or install add `.tsift/` to .gitignore?
### Re: .gitignore handling — opus-4-6

No. Neither `tsift init` nor `tsift index` adds `.tsift/` to `.gitignore`. It's a manual step today.

Both agent-loop and the tsift repo itself already have `.tsift/` in their `.gitignore`, but that was added by hand. A new project using tsift for the first time would need to add it manually or risk committing the index database, summary cache, and config.

`tsift init` is the natural place to add this — it already creates/modifies project files and is idempotent. The check would be: if `.gitignore` exists and doesn't contain `.tsift/`, append it; if no `.gitignore`, create one with `.tsift/`. Same pattern as `cargo init` adding `/target` or `npm init` adding `node_modules`.

❯ do #giti. spec-test-build-install-commit-push
### Re: do #giti — opus-4-6

`tsift init` now adds `.tsift/` to `.gitignore` automatically.

**Changes:**
- `src/init.rs` — `ensure_gitignore()` runs before the AGENTS.md injection. Creates `.gitignore` if missing, appends `.tsift/` if absent, skips if already present. `InitResult.gitignore_added` field for reporting.
- `src/main.rs` — `cmd_init` prints `.gitignore: added .tsift/` when the entry was added.
- `SPEC.md` — updated Init behavior list (gitignore is now step 1).

**Tests:** +3 → 239 total. Covers: create new `.gitignore`, append to existing, skip when already present.

**Commit:** `btakita/tsift@60ab0d3`. Pushed. Submodule pointer updated.

❯ If I run `/tsift init` on a repo twice, will the second time reindex everything?
### Re: tsift init idempotency — opus-4-6

No. `tsift init` does not index anything — it only touches two files:

1. **`.gitignore`** — checks if `.tsift/` is already present, skips if so
2. **`AGENTS.md` / `CLAUDE.md`** — checks if the `<!-- tsift:code-navigation -->` marker exists, skips if the section matches

Running it twice produces "already present (no changes needed)" on the second run. There's a dedicated test for this (`init_is_idempotent`).

`tsift init` and `tsift index` are separate commands with no overlap. Init sets up project instructions; index builds the AST symbol database. Neither calls the other.

❯ Is tsift currently active on this repo?
### Re: tsift status on agent-loop — opus-4-6

Yes. tsift is initialized and has an index, but it's stale:

```
index: stale (last indexed 31m ago, 5885 files tracked, 14364 stale)
summaries: none
use: search, explain, graph
```

The index exists with 5,885 tracked files but 14,364 files need reindexing. No summaries are cached. The core commands (`search`, `explain`, `graph`) are available — `summarize` is not (no summaries built).

To bring it current: `tsift index .`

❯ `/tsift init` on the claude code session on src/session-share/tasks/claudescore-3.md did not add `tsift/` to src/session-share/.gitignore. Why?
### Re: tsift init didn't update session-share .gitignore — opus-4-6

`tsift init` operates on the directory you pass it, defaulting to `.` (cwd). Claude Code's cwd is always the project root — `/home/brian/work/btakita/agent-loop` — regardless of which session document is active. So `/tsift init` modified (or would modify) agent-loop's `.gitignore`, not `src/session-share/.gitignore`.

Notably, agent-loop's `.gitignore` doesn't have `.tsift/` either right now, so either `tsift init` wasn't actually run on agent-loop, or the entry was lost in a subsequent `.gitignore` edit.

To fix session-share specifically: `tsift init src/session-share`

The broader gap: the `/tsift` skill doesn't infer the target submodule from the session document path. It could resolve `src/session-share/tasks/claudescore-3.md` → submodule root `src/session-share` and pass that to `tsift init` instead of `.`. That would be a skill-level change (not a tsift binary change), since the binary already accepts an explicit path argument.

❯ Please update the skill/runbook to infer submodules. #spec-test-build-install-commit-push
### Re: submodule inference for tsift init — opus-4-6

Done. `tsift init` now resolves file paths to their git/submodule root via `git rev-parse --show-toplevel`.

**Changes:**
- `src/init.rs` — `resolve_project_dir()`: if path is a file, uses parent dir; then resolves to git root. Falls back to the directory itself outside git repos.
- `src/main.rs` — `cmd_init` calls `resolve_project_dir` before `init()`. Prints `resolved: <input> → <target>` when the path changes.
- `SPEC.md` — new "Path Resolution" section documenting the 3-step resolution.
- `/tsift` skill (`SKILL.md`) — added Init section with submodule inference docs.

**Tests:** +4 → 243 total. Covers: directory passthrough, file→parent, git root from subdirectory, git root from file.

**Integration verified:**
```
$ tsift init src/session-share/tasks/claudescore-3.md
resolved: .../claudescore-3.md → .../src/session-share
AGENTS.md: created (tsift Code Navigation section added)
.gitignore: added .tsift/
```

**Commits:** `btakita/tsift@9269260`. Session-share initialized (`0a5fa2a`). Agent-loop pointers + skill updated. All pushed.

❯ Please look in agent-loop and all submodules to see if `.tsift/index.db` was committed. Please remove `.tsift/index.db` from the git history and force push.
### Re: .tsift/index.db audit + history rewrite — opus-4-6

**Audit results:**
- **agent-loop + all submodules (working tree):** `.tsift/index.db` not tracked anywhere. Clean.
- **agent-loop history:** never committed.
- **agent-doc history:** `.tsift/index.db` (7.4MB) was committed in early test-author commits (`7a7855a`, `e43a9ed`, `7e8055b`, `c718591`) on the `20260421/release-0.33.10` branch and tags `v0.33.11`–`v0.33.13`. Not on main/HEAD.
- **All other submodules:** clean history.

**Cleanup:**
- Ran `git filter-repo --invert-paths --path .tsift/index.db --force` in `src/agent-doc`.
- Verified: zero commits now reference `.tsift/index.db` across all refs.
- Force-pushed all branches and tags to `origin`.

No other submodules were affected.

❯ Please investigate ways that tsift can save on token usage. You may look through as much of my session history + git history as needed. Also note that I'm using Codex so look through Codex history as well.
### Re: tsift token savings investigation — opus-4-6

Investigated across Claude Code sessions (30 recent), Codex sessions (227), tsift source, git history, and RTK integration. Here's what I found, ordered by impact.

#### Critical: two showstoppers

1. **`tsift search` hangs on stale index.** The rewrite hook transparently converts `rg`/`grep -r` → `tsift search --strategy lexical`. When the index is stale (14K+ files currently), search blocks indefinitely. The agent has no idea why its grep stopped responding. Fix: fail fast with an error when index staleness exceeds a threshold, or add a timeout to the sift engine.

2. **`tsift index --check` dumps 1.7MB (433K tokens).** It lists every stale file path. The autoindex hook redirects to `/dev/null`, but any agent calling it directly would inject 433K tokens into context. Even `--exit-code` doesn't suppress the list. JSON variant is worse at 2.6MB. Fix: add `--quiet`/`--summary` that only outputs the summary line; make `--exit-code` suppress the file list.

#### High: global compact mode

No `--compact` flag exists on any command today. Adding a global `--compact` to the `Cli` struct would cut 40-60% of human-readable output across all commands:

| Command | Current tokens | Compact estimate | Savings |
|---------|---------------|-----------------|---------|
| search (10 results) | 300-800 | 120-300 | ~60% |
| explain (typical) | 200-4,700 | 100-1,500 | ~50-70% |
| communities | 9,300 | 2,800 | ~70% |
| graph --callees | 4,400 | 1,800 | ~60% |
| path (multi-hop) | 50-100 | 25-50 | ~50% |

Specific cuts: drop score precision to 2 decimals, drop `Strategy: X | Indexed: N | Skipped: N` metadata line, cap community member lists at 5 with `(+N more)`, group explain callees by file, remove blank separator lines.

#### High: JSON is 1.5-2.7x larger than human output

Every `--json` call uses `serde_json::to_string_pretty()`. Switching to `to_string()` (compact JSON) saves 30-50% of JSON tokens. There are ~19 call sites in `main.rs`. Add `--pretty` flag for human debugging, make compact the default.

#### Medium: path prefix stripping

All file paths in output are absolute. Stripping the common project root saves ~15-20 tokens per path occurrence. `explain main` lists 232 callees each with a full path — that's ~3,500 tokens just in path prefixes.

#### Medium: Codex has zero tsift integration

Across 227 Codex sessions: **zero** tsift commands, **1,894** raw `rg` invocations, **184** `find` calls. The tsift rewrite hook only runs in Claude Code (`PreToolUse` in `~/.claude/settings.json`). Codex doesn't benefit at all. Options:
- Add a Codex `UserPromptSubmit` hook that rewrites rg/grep-r
- Or add tsift to `.codex/AGENTS.md` instructions so Codex calls tsift directly

#### Medium: output caps for high-connectivity symbols

`explain main` → 18.9KB (4,726 tokens, 232 callees). `graph Some --callers` → 20.5KB (5,128 tokens). `communities` → 37KB (9,303 tokens, 1,532 members). Add `--limit N` to explain/graph/communities to cap result lists. Default to 20 for explain/graph, 10 communities with top-5 members each.

#### Low: RTK integration

RTK saves 87.6% across 16K commands but has no tsift awareness (`~/.config/rtk/filters.toml` is empty). Volume is low (35 tsift invocations in 30 sessions) so ROI is limited, but adding filters for the verbose commands would be a safety net.

#### Already efficient (no action needed)

- `tsift status` — 43 tokens, well-designed
- `tsift route` — 14 tokens, minimal
- `tsift init` — 33 tokens when no-op
- `tsift sql` — ~87 tokens average

#### Context: summarize savings are theoretical

The "78% savings" claim in the compacted summary is projected from the formula `1925 saved tokens per summary lookup × N lookups`. No summaries have been built on any project yet (`summaries: none` on agent-loop). Activating summarize would provide actual measured data.

#### Recommended priority order

1. Fix search hang on stale index (blocks sessions via rewrite hook)
2. Fix `index --check` output flood (433K token risk)
3. Global `--compact` flag (40-60% savings across all commands)
4. Compact JSON default (30-50% savings on JSON paths)
5. Path prefix stripping (cross-cutting, ~15-20 tokens per path)
6. Output caps for explain/graph/communities (`--limit`)
7. Codex rewrite hook or AGENTS.md instructions
8. RTK filters (safety net)

❯ do #hang. spec-test-build-install-commit-push
### Re: do #hang — opus-4-6

`tsift search` now has a 30-second timeout (configurable via `--timeout`).

**Problem:** The sift engine's `search()` call blocks indefinitely on large/stale repos. The rewrite hook transparently converts `rg`/`grep -r` → `tsift search`, so agents hang silently with no error or progress.

❯ Should we fork sift to not exhibit this behavior or is our current fix good enough?

**Fix:** Wrapped `engine.search()` in a spawned thread with `mpsc::recv_timeout`. On timeout, exits with a clear error:

```
tsift search timed out after 30s (strategy: lexical). The index may be stale — run `tsift index .` to rebuild, or use `--timeout 0` to disable the timeout.
```

**Changes:**
- `src/main.rs` — `--timeout` flag on Search (default 30, 0 = disabled). New `run_search_with_timeout()` function wraps the sift call in a thread+channel with configurable timeout.
- `SPEC.md` — new "Search Timeout" section documenting the flag and behavior.

**Tests:** +3 → 246 total. Covers: normal search completes within timeout, timeout=0 disables, channel timeout mechanism.

**Verified:** `tsift search "main" --timeout 5` on the stale agent-loop repo correctly times out with the error message. Small directories complete normally.

**Commit:** `btakita/tsift@91aaab0`. Pushed. Submodule pointer updated (`205e7dea5`).
### Re: fork sift? — opus-4-6

The current fix is good enough. The timeout wrapper solves the user-facing problem — agents no longer hang silently — with zero maintenance overhead.

Forking sift would let us fix the root cause inside the engine (why it blocks on large/stale directories), but it adds ongoing burden to track upstream changes from rupurt/sift. That tradeoff only makes sense if the 30-second timeout triggers frequently on **legitimate** searches with a fresh index. That would mean the engine itself is fundamentally too slow for the workload, not just blocking on stale data.

**When to reconsider:** if fresh-index searches on agent-loop-sized repos (~6K files) regularly hit the 30s timeout, fork and investigate the sift internals. Until then, the thread+timeout wrapper is the right layer to fix this at.

❯ Please create a bug report with a reproducible case to the sift github repo.
### Re: sift bug report — opus-4-6

Filed: [rupurt/sift#2](https://github.com/rupurt/sift/issues/2) — "search() hangs on large repos — walkdir doesn't respect .gitignore"

**Root cause found during investigation:** sift uses `walkdir::WalkDir` in `collect_file_paths()` (corpus.rs:283) which walks every file in the tree. The `Ignore` struct only checks `.siftignore` files plus two hardcoded exclusions (`target/**`, `.git/**`). It never reads `.gitignore`. Agent-loop has 1.87M files total — with only `.git/` and `target/` excluded, sift tries to walk and index all of them.

**Suggested fix in the issue:** replace `WalkDir` with the `ignore` crate's `WalkBuilder` (already a dependency), which natively respects `.gitignore` at every tree level. The `Ignore` struct would layer `.siftignore` on top.

**Our workaround:** the `--timeout 30` wrapper from the previous cycle prevents the hang. If the upstream fix lands, the timeout becomes a safety net rather than the primary defense.

❯ Add `.siftignore` with appropriate files.
### Re: .siftignore — opus-4-6

Created `.siftignore` at the agent-loop root:

```
target/**
dist/**
build/**
.next/**
__pycache__/**
node_modules/**
.venv/**
vendor/**
.agent-doc/**
.idea/**
.wrangler/**
*.db
*.wasm
*.so
*.dylib
*.lock
```

**Caveat:** this won't fix the search timeout today. Sift's `collect_file_paths()` uses `WalkDir` which still traverses every directory entry before filtering against `.siftignore` patterns — it can't prune subtrees. With 1.87M files, just the walk takes >40 seconds regardless of filtering.

The `.siftignore` will become effective when the upstream fix ([rupurt/sift#2](https://github.com/rupurt/sift/issues/2)) replaces `WalkDir` with `ignore::WalkBuilder`, which prunes entire directories from the walk. Until then, the `--timeout 30` wrapper remains the active defense, and scoped searches (`--scope <submodule>`) work fine on individual submodules.

❯ do #iflood. spec-test-build-install-commit-push
### Re: do #iflood — opus-4-6

`tsift index` now has `--quiet` (`-q`) which suppresses the per-file change list.

**Changes:**
- `src/main.rs` — `--quiet` flag on Index command. `--exit-code` implies `--quiet`. In human mode, suppresses the per-file list. In JSON mode, omits the `changes` array and uses compact serialization. Both single-repo and workspace paths updated.
- `SPEC.md` — new "Index Quiet Mode" section, updated subcommand reference.

**Before/after on tsift submodule (3 stale files):**
```
# Without --quiet: summary + 3 file paths
Index (check): 17 files tracked
  new: 0  modified: 3  deleted: 0  unchanged: 14

  ~ .../SPEC.md [markdown]
  ~ .../src/init.rs [rust]
  ~ .../src/main.rs [rust]

# With --quiet: summary only
Index (check): 17 files tracked
  new: 0  modified: 3  deleted: 0  unchanged: 14
```

On the original 14K stale files scenario, this cuts output from 1.7MB (433K tokens) to ~80 bytes. JSON goes from 2.6MB to ~120 bytes.

**Tests:** +3 → 249 total. Covers: quiet suppresses file list, exit-code implies quiet, quiet JSON omits changes.

**Commit:** `btakita/tsift@936b2da`. Pushed. Submodule pointer updated (`83d9942ac`).

❯ Please ensure that codex can use tsift on src/tsift.
### Re: Codex tsift on src/tsift — gpt-5

Done. The gap was that `tsift init` only updated `CLAUDE.md` when a repo had no `AGENTS.md`, so repos like `src/tsift` left Codex without any local tsift navigation instructions.

**Changes:**
- `src/tsift/src/init.rs` — `tsift init` now always ensures `AGENTS.md` exists, and still updates `CLAUDE.md` when present. Added tests for Claude-only repos and dual-file sync.
- `src/tsift/src/main.rs` — `tsift init` now reports each instruction file it updated.
- `src/tsift/SPEC.md` and `src/tsift/CLAUDE.md` — docs updated to describe the Codex-visible init behavior.
- Ran `cargo run -- init .` in `src/tsift`, which created `src/tsift/AGENTS.md` and appended the Code Navigation section to `src/tsift/CLAUDE.md`.

**Verification:**
- `cargo test` in `src/tsift`: passed (`243` unit tests + `6` integration tests).
- `make check` in `src/tsift`: still fails on pre-existing Clippy errors outside this change (`src/audit.rs` and several existing spots in `src/main.rs`), so the repo is not fully green yet.

Codex should now pick up tsift usage instructions directly from `src/tsift/AGENTS.md`.

❯ do #compact. spec-test-build-install-commit-push 
### Re: do #compact — gpt-5

Done. `tsift` now accepts a global `--compact` flag for terse human-readable output across the CLI, and the earlier Codex-visible `tsift init` / `AGENTS.md` work is now actually committed in the submodule instead of only living in the worktree.

**Changes:**
- `src/tsift/src/main.rs` — added global `--compact`; compact human-output renderers for `search`, `index`, `graph`, `communities`, `path`, `explain`, `audit`, `summarize`, `lint`, `sql`, and `status`.
- `src/tsift/src/status.rs` — compact status formatter + regression coverage.
- `src/tsift/src/audit.rs` / `src/tsift/examples/dump_ast.rs` — clippy cleanup needed to keep `make check` green.
- `src/tsift/SPEC.md` and `src/tsift/CLAUDE.md` — documented `--compact`; retained the Codex `tsift init` / `AGENTS.md` setup behavior from the previous local change set.
- `src/tsift/AGENTS.md` — now tracked in git.

**Verification:**
- `make check`
- `cargo build`
- `cargo install --path .`
- `./target/debug/tsift --compact status .`

**Commit:** `btakita/tsift@e01e680` — pushed.
The parent repo now has the updated `src/tsift` submodule pointer staged for the session commit.

❯ do #json. spec-test-build-install-commit-push
### Re: do #json — opus-4-6

All `--json` output now uses compact (single-line) serialization by default. Added global `--pretty` flag for indented output when human readability is needed.

**Changes:**
- `src/main.rs` — added `--pretty` global flag to `Cli` struct. New `to_json()` helper dispatches between `serde_json::to_string` (default) and `serde_json::to_string_pretty` (when `--pretty`). Replaced 22 `to_string_pretty` call sites across all commands. Threaded `pretty: bool` through all `cmd_*` functions.
- `src/status.rs` — updated test to use compact JSON.
- `SPEC.md` — new "Compact JSON Default" section documenting `--pretty` flag and usage.
- `CLAUDE.md` — updated global flags description.

**Before/after:**
```
# default (compact) — 1 line
{"index":{"state":"stale","total_files":17,...},"summaries":{"state":"none"},...}

# --pretty — indented
{
  "index": {
    "state": "stale",
    "total_files": 17,
    ...
  }
}
```

**Tests:** +3 → 257 total. Covers: compact default has no newlines, pretty has indentation, compact is shorter than pretty.

**Verification:**
- `cargo test` — 257 passed
- `cargo install --path .`
- `cargo build --release`
- `tsift status src/tsift --json` — compact output confirmed
- `tsift --pretty status src/tsift --json` — indented output confirmed

**Commit:** `btakita/tsift@ce4855f` — pushed. Submodule pointer updated.

❯ Can we use msgpack, a rust equivalent of msgspec, or another binary json-compatible protocol for even more compression of json?
### Re: binary JSON protocols — opus-4-6

Not useful here. The consumers of tsift's `--json` output are LLM agents (Claude Code, Codex) — they read the JSON as text tokens in their context window. A binary format like msgpack would be smaller on disk/wire, but the agent would need to decode it back to text before it can reason about it, which means either piping through a decoder (adding a tool call) or embedding a text representation anyway. The token cost is the same or worse.

The real compression lever for agent consumption is what we're already doing: compact JSON (done), plus the remaining backlog items — path prefix stripping (#paths) and output caps (#caps) — which reduce the *semantic* content the agent has to ingest, not just the encoding overhead.

Binary protocols make sense for machine-to-machine pipelines (e.g., tsift feeding data to graphify's ingest). If that becomes a use case, `rmp-serde` (msgpack for Rust) is the natural choice — it's a drop-in `Serialize`/`Deserialize` backend, same as `serde_json`. But for agent-facing output, text JSON is the right format.

❯ I heard that translating to Chinese saves tokens. Would there be too much drift for this strategy? Is there another way to compress english to use less tokens?
### Re: Chinese tokens and English compression — opus-4-6

The Chinese token savings are real but don't apply to tsift output. CJK characters pack more meaning per token in BPE tokenizers — a Chinese sentence can use 30-50% fewer tokens than its English equivalent. But tsift output is mostly **identifiers** (file paths, symbol names, JSON keys) that can't be translated, plus short structural labels. The translatable prose is a tiny fraction of the output. The drift risk is high and the savings negligible.

More practical compression strategies for English tokens, roughly ordered by applicability to tsift:

1. **Reduce semantic content** — don't output what the agent doesn't need. This is what #paths (strip common path prefixes) and #caps (limit result counts) do. Highest ROI by far.
2. **Shorter field names in JSON** — `"total_files"` → `"tf"`, `"last_indexed_secs_ago"` → `"age_s"`. Saves ~15-25% on JSON but kills human readability. Could be a `--terse` mode paired with a schema header.
3. **Tabular output over prose** — `--compact` already does this for human mode. Could go further: `search` results as TSV instead of formatted blocks.
4. **Abbreviation conventions** — `fn` for function, `mod` for modified, `unch` for unchanged. `--compact` already uses some of these.
5. **Schema-then-values pattern** — print column headers once, then rows of just values. Good for repeated structures (search results, edges).

Items 1-2 are worth building. Items 3-5 are marginal — the savings exist but the complexity cost is high relative to just trimming what you output. The backlog items #paths and #caps remain the best next moves.

❯ Should we add 3-5 into the backlog anyways?
### Re: backlog items 3-5 — opus-4-6

Added all three as recommended. They're low priority behind #paths and #caps but worth having on the radar so they don't get lost if the higher-priority items reveal further optimization opportunities.
<!-- agent:boundary:ca1406a4 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [ ] [#tabular] [recommended] Tabular output for repeated structures (search results as TSV, edges as columns)
- [ ] [#abbrev] [recommended] Abbreviation conventions in --compact (fn/mod/unch/del)
- [ ] [#schema] [recommended] Schema-then-values pattern for repeated output (headers once, rows of values)
- [ ] [#rtk] [recommended] RTK filters for verbose tsift commands (safety net)
- [ ] [#paths] [recommended] Path prefix stripping — relative paths instead of absolute
- [ ] [#caps] [recommended] Output caps for explain/graph/communities (--limit N)
- [ ] [#codex] [recommended] Codex tsift integration — rewrite hook or AGENTS.md instructions
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
