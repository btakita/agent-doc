---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
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
- **Don't fork sift** — timeout wrapper is sufficient. Reconsider if fresh-index searches on ~6K-file repos regularly hit 30s. Upstream bug: [rupurt/sift#2](https://github.com/rupurt/sift/issues/2).
- **Binary JSON protocols not useful** — agents consume text tokens, not wire format. `rmp-serde` for machine-to-machine pipelines only.
- **Chinese/CJK compression not applicable** — tsift output is mostly identifiers, not translatable prose.
- **Reduce semantic content is safe in --compact mode only** — non-compact retains all fields, --json unaffected.
- **#paths and #caps remain highest-impact open items** — #paths needs project-root detection, #caps needs per-command default limits.

**Completed work:**
- **#cdet** — Louvain community detection: `tsift communities`. `btakita/tsift@400c199`.
- **#gpth** — Graph path queries: `tsift path` + `tsift explain`. `btakita/tsift@6ad5983`.
- **#nkec** — Skill drift detection: `tsift audit`. `btakita/tsift@a4235b7`.
- **#865b** — Duplicate skill detection via Jaccard similarity. `btakita/tsift@59e1e79`.
- **#ay3j** — Usage tracking (`--usage`), **#t3pq** — cleanup recommendations (`--cleanup`), **#dt80** — report output (`--report`), **#cs4r** — `tsift lint`. All in `btakita/tsift@056dbac`.
- **#summ** — `tsift summarize` — cached LLM analysis with batch extraction and local SQLite cache. `btakita/tsift@5801090`.
- **#hidx** — Auto-reindex hook + `--exit-code` flag. `btakita/tsift@1c712f7`.
- **#cmdx** — `tsift init` — idempotent AGENTS.md/CLAUDE.md injection + .gitignore setup. `btakita/tsift@d9af2b0`, `btakita/tsift@60ab0d3`.
- **#stat** — `tsift status` — session-start health check with `use:` list. `btakita/tsift@9f4a111`.
- **#srch** — Search hang fix: default to lexical strategy + scope sift path to submodule. `btakita/tsift@882421d`.
- **#hang** — 30-second search timeout (configurable via `--timeout`). `btakita/tsift@91aaab0`.
- **#iflood** — `tsift index --quiet` suppresses per-file change list. `--exit-code` implies `--quiet`. `btakita/tsift@936b2da`.
- **#compact** — Global `--compact` flag for terse human-readable output across all commands. `btakita/tsift@e01e680`.
- **#json** — Compact JSON default + `--pretty` flag. 22 call sites updated. `btakita/tsift@ce4855f`.
- **#rtk** — RTK TOML filters (`~/.config/rtk/filters.toml`) + hook v2 (`tsift-rewrite.sh`). Routes verbose commands through RTK for output capping. `btakita/tsift@951a28a`.
- **#semred** — Reduce semantic content: dropped per-community Q scores and language annotations in compact mode. `btakita/tsift@5b050af`.
- **Submodule inference** — `tsift init` resolves file paths to git/submodule root. `btakita/tsift@9269260`.
- **`.tsift/index.db` cleanup** — removed from agent-doc history via filter-repo + force push.
- **`.siftignore`** — created at agent-loop root (workaround until upstream sift fix lands).
- **Codex tsift setup** — `tsift init` ensures AGENTS.md alongside CLAUDE.md.
- **#wrka** — closed (workaround obsolete after #srch fix).
- **#mext** — closed (single-command Makefile wrapper, superseded by `tsift init` + `tsift status`).
- Test count progression: 125 → 139 → 153 → 166 → 174 → 197 → 211 → 222 → 229 → 236 → 239 → 243 → 246 → 249 → 257.

**Research notes:**
- Alternative tools to graphify: LightRAG, GraphRAG, sift-kg (Python, CLI-ready); EdgeQuake, Rig, petgraph (Rust, incomplete pipelines). No Rust tool ships the full ingest→extract→cluster→export pipeline as a CLI today.
- Token savings from `tsift summarize`: ~78% per bug-fix session (~25k → ~5.5k tokens), ~$73/month at Opus pricing for 8 sessions/day.
- Markdown is first-class: indexing extracts headings + code block languages via tree-sitter-md; search + lint work on .md/.mdx; call-graph intentionally skipped.
- Token savings investigation: 30 Claude Code + 227 Codex sessions analyzed. Priority: search hang > index flood > compact > JSON > paths > caps > codex > RTK.
- Practical compression strategies: (1) reduce semantic content, (2) shorter JSON field names, (3) tabular output, (4) abbreviation conventions, (5) schema-then-values. Items 1-2 worth building, 3-5 marginal.

❯ do #bags. spec-test-build-install-commit-push
### Re: do #bags — opus-4-6

Implemented `--terse` global flag for abbreviated JSON field names with inline schema header.

**What it does:**
- `tsift --terse <command>` outputs JSON with shortened field names (implies `--json`)
- Output format: `{"_s": {short→long mapping}, "d": {data with short keys}}`
- Schema header only includes keys present in the current response
- 100+ field name mappings covering all output structs (e.g., `caller_file`→`cf`, `name`→`n`, `modularity`→`q`)
- Unknown keys pass through unchanged
- Compatible with `--pretty` for indented terse output

**Commit:** `btakita/tsift@c2010bd`. 261 tests pass (5 new terse tests). Built + installed.

❯ do #paths. spec-test-build-install-commit-push
### Re: do #paths — opus-4-6

Implemented `--absolute` global flag. All output paths are now project-relative by default (token savings).

**What it does:**
- Default: paths stripped to project-relative (`src/main.rs` instead of `/home/user/project/src/main.rs`)
- `tsift --absolute <command>` restores full filesystem paths
- Affects: search, graph, explain, index, summarize, lint
- JSON: known path keys (`file`, `path`, `caller_file`, `file_path`) stripped via `relativize_json_paths()`
- Database storage stays absolute — stripping is display-only

**Implementation:**
- `relativize(path, root)` / `relativize_pathbuf()` utility functions
- `relativize_edges()`, `relativize_symbols()`, `relativize_symbol_hits()` batch helpers
- `relativize_json_paths()` for post-processing sift library JSON output
- Threaded through `cmd_graph`, `cmd_explain`, `cmd_search`, `cmd_index` via `absolute` parameter

**Commit:** `btakita/tsift@357af45`. 268 tests pass (7 new). Built + installed.

❯ do #caps. #spec-test-build-install-commit-push
### Re: do #caps — opus-4-6

Implemented `--limit N` flag for `graph`, `communities`, and `explain` commands.

**What it does:**
- `graph --limit N` (default 20): caps edges per direction (callers/callees separately)
- `communities --limit N` (default 10): caps number of communities displayed
- `explain --limit N` (default 15): caps callers and callees lists each
- `--limit 0` disables the cap (show everything)
- Short flag: `-l N`

**Truncation indicators:**
- Text output: `(+N more)` suffix with hint to use `--limit 0`
- JSON output: `truncated: true` boolean + `total` / `callers_total` / `callees_total` counts

**Implementation:**
- Truncation applied after query, before display — no database changes
- JSON combined mode (`graph` with both directions) includes per-direction truncation fields
- Communities truncation applied after `min_size` filtering

**Commit:** `btakita/tsift@e72e982`. 277 tests pass (9 new limit tests). Built + installed.

❯ do #codex. #spec-test-build-install-commit-push
### Re: do #codex — opus-4-6

Implemented `tsift init --codex` for Codex hook integration.

**What it does:**
- `tsift init --codex` merges a `UserPromptSubmit` auto-reindex hook into `.codex/hooks.json`
- Creates `.codex/` directory and `hooks.json` if they don't exist
- Appends to existing hooks when `.codex/hooks.json` already has other hooks (e.g., agent-doc)
- Idempotent — detects existing tsift hook by status message and skips
- Hook command: `tsift index --check --exit-code . >/dev/null 2>&1 || tsift index . >/dev/null 2>&1`

**Why not the rewrite hook?** Codex doesn't support `PreToolUse` hooks, so the `grep`/`rg` → `tsift search` rewrite can't be ported. The AGENTS.md code-navigation instructions already tell Codex agents to prefer tsift over grep/rg.

**Commit:** `btakita/tsift@474ef45`. 282 tests pass (5 new Codex hook tests). Built + installed.

❯ do #tabular. spec-test-build-install-commit-push
### Re: do #tabular — opus-4-6

Implemented `--tabular` global flag for TSV output across four commands.

**What it does:**
- `tsift --tabular search <query>` — two TSV tables: symbols (`match_type`, `kind`, `name`, `file`, `line`, `score`) then hits (`rank`, `path`, `confidence`, `score`)
- `tsift --tabular graph <sym>` — edges table (`direction`, `name`, `file`, `line`) with `caller`/`callee` direction values
- `tsift --tabular communities` — table (`id`, `size`, `members`) where members are comma-separated
- `tsift --tabular explain <sym>` — definition table, then caller/callee edges, then community summary
- Truncation: `# (+N more)` comment lines; sections separated by blank lines
- Compatible with `--limit`, `--absolute`

**Commit:** `btakita/tsift@d83f1eb`. 289 tests pass (7 new tabular tests). Built + installed.

❯ do #abbrev. spec-test-build-install-commit-push
### Re: do #abbrev — opus-4-6

Implemented abbreviation conventions in `--compact` mode for shorter human-readable output.

**What it does:**
- Symbol kinds: `function`→`fn`, `method`→`meth`, `class`→`cls`, `interface`→`iface`, `type_alias`→`type`, `data_class`→`data_cls`, `sealed_class`→`sealed_cls`, `enum_class`→`enum_cls`, `companion_object`→`comp_obj`, `object`→`obj`, `heading`→`h`, `code_block`→`code`
- Match types: `exact_name`→`exact`, `partial_tags`→`partial`
- Section headers: `symbols`→`syms`, `callers`→`crs`, `callees`→`ces`, `community`→`comm`, `communities`→`comms`, `members`→`mbrs`, `symbol`→`sym`, `definitions`→`defs`
- Short kinds pass through unchanged: `struct`, `trait`, `enum`, `const`, `static`, `mod`, `impl`
- Applied in search, explain, graph, and communities compact output

**Commit:** `btakita/tsift@ea59263`. 291 tests pass (2 new abbreviation tests). Built + installed.

❯ do #schema. spec-test-build-install-commit-push
### Re: do #schema — opus-4-6

Implemented `--schema` global flag for columnar JSON output — arrays of same-structured objects become `{"_c": [cols], "_r": [[vals]...]}`.

**What it does:**
- `tsift --schema <command>` converts homogeneous arrays (2+ objects with identical keys) to `{"_c": [...], "_r": [...]}` format (implies `--json`)
- Non-homogeneous arrays, single-element arrays, and non-object arrays pass through unchanged
- Applied recursively to nested objects (e.g., `symbols`, `callers`, `callees`, `hits` within a response)
- Combines with `--terse`: abbreviated column names in `_c`, plus `_s` schema mapping
- Combines with `--pretty` for indented output

**Example:**
```json
// --schema
{"symbols":{"_c":["kind","line","name"],"_r":[["fn",10,"main"],["fn",20,"helper"]]}}

// --schema --terse
{"_s":{"k":"kind","l":"line","n":"name"},"d":{"sy":{"_c":["k","l","n"],"_r":[["fn",10,"main"],["fn",20,"helper"]]}}}
```

**Commit:** `btakita/tsift@cfc73ea`. 297 tests pass (6 new schema tests). Built + installed.

❯ In the skill file, add a version check similar to agent-doc. Also add versioning to tsift even though tsift is currently private.
### Re: tsift version check + versioning — gpt-5

Implemented the tsift versioning surface in three places:

- `.claude/skills/tsift/SKILL.md` now has `tsift-version: "0.1.0"` frontmatter plus a `tsift --version` check. If the installed binary is newer, the skill now treats itself as stale and refreshes from `src/tsift/CLAUDE.md`, `src/tsift/VERSIONS.md`, and live `tsift --help` output.
- `src/tsift/VERSIONS.md` now establishes a private-project changelog and documents `Cargo.toml` / `tsift --version` as the canonical version source.
- `src/tsift/CLAUDE.md` now documents the versioning contract so the skill has a repo-local source of truth.

**Verification:**
- `cargo test` passed: 297 tests total.
- `make check` is still failing on pre-existing clippy findings in `src/main.rs` (`clippy::collapsible-if` around lines 863-868, plus existing `clippy::too_many_arguments` findings on several command helpers). I did not change those code paths in this cycle.

❯ If a tmux session is stale, please run `tsift index`.
❯ This happened in /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md
```
• Ran tsift search "session digest evidence strategy"
  └ Error: tsift search timed out after 30s (strategy: lexical). The index may be stale — run `tsift index .` to
    rebuild, or use `--timeout 0` to disable the timeout.
```
### Re: stale index + lock recovery — gpt-5

I ran `tsift index .` as requested. It did not complete: the process is still stuck in `D` state after more than 4 minutes, and the repo currently has `.tsift/index.db-journal`. That points to a blocked OS / filesystem wait, not a normal long-running search or a simple stale-index timeout.

The stale-session guidance is already present in `.claude/skills/tsift/SKILL.md` and `src/tsift/CLAUDE.md`, so I did not add another documentation patch for that path. I fixed the lock-handling gap in the code instead:

- `src/index.rs` and `src/summarize.rs` now set a 5s SQLite busy timeout and `journal_mode=WAL` for writable DBs.
- Those modules now expose read-only open helpers, and `src/status.rs` plus the query paths in `src/main.rs` use those read-only opens so status / graph / explain / path / federated search do not grab writer-capable handles unnecessarily.
- Added DB-open tests that assert the WAL + busy-timeout configuration.

On the design question: no, a different "local file DB" is not the right fix. `.tsift/index.db` is already a local SQLite file; the issue is concurrent access plus a blocked writer, so WAL + read-only query paths are the correct first recovery step.

**Verification:**
- `cargo test` passed: 295 unit tests + 6 integration tests.
- `make check` still fails on existing clippy debt in `src/main.rs`: `clippy::collapsible-if` around lines 1043-1046 and the pre-existing `clippy::too_many_arguments` helper warnings. I did not change those APIs in this cycle.

❯ If index.db is locked, please attempt to recover. Should we use a local file db that does not lock?
❯ This happened in /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md:
```
• Explored
  └ Search pending_capture|pending capture|backlog-capture|#pcap|recommended in agent-doc

• Ran tsift search pending_capture
  └ Error: database is locked

    Caused by:
        Error code 5: The database file is locked
```
### Re: index.db recovery + locking — gpt-5

I attempted recovery directly.

- I quarantined the locked index files to `.tsift/recovery-20260427-180342/` and started a fresh rebuild at `.tsift/index.db`.
- That cleared the immediate SQLite lock symptom: `tsift status` now works again instead of failing with `database is locked`.
- The rebuild itself is still not healthy. The new `tsift index .` process also entered `D` state in `jbd2_l`, so the remaining blocker is a kernel / filesystem write stall, not ordinary SQLite contention.

So the answer to "should we use a local file DB that does not lock?" is no. SQLite already is a local file DB, and this failure is below the database layer: a blocked filesystem write can wedge any embedded file-backed store. Swapping SQLite for another local-file database would not remove a `D`-state I/O stall.

Current state:

- `tsift status` succeeds and reports a partial fresh rebuild instead of a lock failure.
- `tsift search pending_capture` no longer errors with `database is locked`; it now times out because the index rebuild is incomplete/stale.
- The previous locked DB is preserved in `.tsift/recovery-20260427-180342/` if we need to inspect it later.

The right follow-up remains #vxtz: investigate why `tsift index .` is entering `D` state and leaving a journal file on this host. That is the root cause behind both the earlier lock error and the failed rebuild.

❯ do #vxtz
### Re: do #vxtz — gpt-5

I investigated #vxtz in the code, not just the host state, and there were two distinct issues.

First, several read/query paths were still opening the index with writer-capable SQLite handles. I fixed that in `src/tsift/src/main.rs`:

- `search` now uses `IndexDb::open_read_only()` in both the default and `--scope` symbol-query paths.
- `graph` and `communities` now go through the shared read-only `open_index_db()` path, matching `path` / `explain`.
- Added regression tests that hold a write lock open and assert `cmd_search` and `cmd_graph` still succeed.

Second, the writer side was trusting `PRAGMA journal_mode=WAL` without verifying that SQLite actually accepted it. I tightened both writable DB opens:

- `src/tsift/src/index.rs` now queries `PRAGMA journal_mode` after setup and fails closed unless the mode is actually `wal`.
- `src/tsift/src/summarize.rs` now does the same for `summaries.db`.

That matters because the live root DB is still reproducing the bad state:

- `cargo run -- sql /home/brian/work/btakita/agent-loop/.tsift/index.db --query "SELECT COUNT(*) AS count FROM symbols"` still returns `database is locked`.
- `cargo run -- search pending_capture --strategy lexical --timeout 5 --path /home/brian/work/btakita/agent-loop` still returns `database is locked`.
- `tsift status` still succeeds because it only needs `file_state` / staleness checks, while the locked reads are hitting the symbol/call-edge side of the DB.
- The presence of `.tsift/index.db-journal` instead of a WAL file is the key signal: the currently running index writer is still in rollback-journal mode, so concurrent readers can block even with read-only opens.

So the root cause is narrower now:

- Part of the earlier lock behavior was a real tsift bug: some query commands were still opening write-capable handles.
- The deeper remaining problem is that the active writer on this host is not actually running in WAL mode, and tsift previously allowed that to happen silently. New writers will now fail fast instead of partially indexing and leaving a misleadingly "configured" but lock-prone DB behind.

**Verification:**
- `cargo test` passed: 297 unit tests + 6 integration tests.
- `make check` still fails on the pre-existing clippy debt in `src/main.rs`: `clippy::collapsible-if` around lines 1043-1046 plus the existing `clippy::too_many_arguments` findings on command helpers. I did not change those warning surfaces in this cycle.

❯ The current turn on /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md got the following error. Why?
```
• Ran tsift search sdig2arch
  └ Error: tsift search timed out after 30s (strategy: lexical). The index may be stale — run `tsift index .` to
    rebuild, or use `--timeout 0` to disable the timeout.
```
### Re: session-share timeout — gpt-5

That specific failure in `src/session-share` is a stale-index timeout, not the root-repo lock bug.

I checked it directly in `/home/brian/work/btakita/agent-loop/src/session-share`:

- `tsift status` succeeds there and reports `index: stale (last indexed 14h ago, 700 files tracked, 51 stale)`.
- Re-running `tsift search sdig2arch --strategy lexical --timeout 5` in that submodule reproduces the same timeout message.
- The timeout text comes from `run_search_with_timeout()` in `src/tsift/src/main.rs`: the sift engine call runs in a worker thread, and if it does not return before the timeout, tsift emits the generic `The index may be stale` guidance.

So the immediate reason for that turn is: lexical sift search in `session-share` did not finish within the timeout window, and the local `session-share/.tsift/index.db` is stale enough that the stale-index explanation is credible.

That is different from the current root `agent-loop` failure mode. At the repo root, `tsift status` / `tsift search` are now hitting `database is locked`, and there is an active `tsift index .` plus `.tsift/index.db-journal`, which is the separate #vxtz investigation.

So, short version:

- `src/session-share` timeout: stale local index; rebuild that submodule with `tsift index .` there.
- root `agent-loop` lock/journal issue: separate writer/locking problem still under #vxtz.

❯ We should be able to quickly detect if the tsift db is stale...not wait for a timeout.
❯ If the tsift db is stale, tsift should automatically reindex the db.
### Re: stale detection + auto-reindex — gpt-5

Yes on the first part: tsift should detect staleness quickly, and it already can.

The fast path is `tsift index --check --exit-code .` / `tsift status`. Those do not wait for search timeout; they compare the indexed file-state against the working tree and report stale state directly. That is also what the existing auto-reindex hooks use.

The second part already exists too, but only at the hook layer today:

- `tsift init --codex` installs a `UserPromptSubmit` hook that runs `tsift index --check --exit-code . || tsift index .`
- `examples/hooks/tsift-autoindex.sh` does the same for Claude Code
- when that hook is installed in the project you are actively working in, stale detection is quick and reindex happens before the next search/explain/graph call

So the `session-share` timeout was not because tsift lacks a fast stale check. It was because that turn still reached `tsift search` with a stale local index, which means the submodule was effectively running without that auto-reindex guard on the path that mattered.

I would still separate the product decisions:

- fast stale detection should be unconditional
- automatic reindex should be opt-in or harness-managed, not silently baked into every `search`

Reason: hidden mutation inside `search` is a worse default than a fast fail/redirect. Reindex can be expensive, can race with active writers, and on this host we already know root `agent-loop` can wedge in `D` state during indexing. So the right hardening path is not "wait for timeout", but also not "every stale search secretly rebuilds".

What I would change is narrower:

- before running sift, `search` can do a cheap stale precheck and emit `index is stale; run tsift index .` immediately instead of spending 30s on a likely-bad query
- optionally add an explicit auto mode such as hook-managed behavior or a future `--autoindex` / config flag for users who want mutation before query

That gets the fast detection you want without making every search command implicitly stateful.

❯ do #stal. spec-test-build-install-commit-push
### Re: do #stal — gpt-5

Implemented the stale-search hardening in tsift.

What changed:
- `tsift search` now does a fast freshness precheck before it invokes the sift engine.
- If an existing index is stale, search now fails immediately with a direct `tsift index ...` / `--autoindex` recovery message instead of waiting for the 30s timeout path.
- Added `tsift search --autoindex` for unhooked sessions. It incrementally builds missing or stale indexes before search, and it also works with `--scope` and `--federated`.
- Updated `SPEC.md` and `CLAUDE.md` to document the new behavior.

Verification:
- `cargo test` passed in the clean tsift clone: 295 unit tests + 6 exit-code tests.
- `make check` passed.
- `cargo install --path .` passed.

Commit: `btakita/tsift@f017c4d` pushed to `origin/main`.

I committed and pushed from a clean local clone because the checked-out `src/tsift` submodule inside `agent-loop` already had unrelated in-flight edits; that avoided folding those local changes into the `#stal` commit.

❯ Is #stal complete?
### Re: #stal completion — gpt-5

Yes. `#stal` is complete in `btakita/tsift@f017c4d`.

That scope was:
- fast stale precheck before `tsift search`
- immediate stale-index failure instead of waiting for the timeout path
- opt-in `tsift search --autoindex` for unhooked sessions

The remaining open item is `#vxtz`, which is separate repo-root indexing/lock behavior, not an unfinished part of `#stal`.

❯ do #vxtz. spec-test-build-install-commit-push
### Re: do #vxtz — gpt-5

Implemented `#vxtz` in a clean `origin/main` worktree and pushed `btakita/tsift@48a4c31`.

What changed:
- query/status paths now use read-only SQLite opens with a 5-second busy timeout (`search`, `graph`, `communities`, `path`, `explain`, `status`, and summary-status checks)
- writable index/summary opens now require `PRAGMA journal_mode=WAL` and fail closed if SQLite does not actually enter WAL mode
- added regression coverage for search/graph succeeding while a writer lock is held, plus WAL/busy-timeout assertions
- updated `SPEC.md` with the SQLite concurrency contract

While porting the patch onto `origin/main`, `make check` exposed an upstream regression on the root `tsift index` path: the non-workspace code path was opening `.tsift/index.db` read-only, which broke fresh index creation. I fixed that in the same commit because the `#vxtz` verification run could not pass otherwise.

Verification:
- `make check` passed in the clean worktree: 301 unit tests + 6 exit-code tests
- `cargo install --path .` passed

Commit: `btakita/tsift@48a4c31` pushed to `origin/main`.
### Re: `#vxtz` completion + backlog miss — gpt-5

Yes, `#vxtz` is done.

The previous turn already established completion: the work landed in `btakita/tsift@48a4c31`, verification passed, and the binary install succeeded. The reason the backlog item stayed open is narrower: that cycle wrote and committed the response, but it did **not** record the matching pending mutation (`--pending-done vxtz`). `agent-doc session-check` recognized that exact mismatch and warned:

- the response appeared to complete existing pending `#vxtz`
- no matching `--pending-done` was recorded that cycle

So this was not a tsift-status ambiguity. It was an `agent-doc` closeout gap: the system knew the backlog item was resolved, but the closeout path only warned instead of auto-resolving it or failing closed before commit.

I treated your `#agent-doc-bug` note as actionable and recorded that follow-up in `tasks/agent-doc/agent-doc-bugs2.md` as `#pdone1`, with plan file `tasks/agent-doc/plan-resolve-existing-backlog-closeout.md`.

I am closing `#vxtz` in this cycle via the binary-owned pending mutation path now.

❯ In tasks/agent-doc/agent-doc-bugs2.md, this issue happened. Why?
```
• Ran tsift search --autoindex "pending capture guard strict warn existing pending done"
  └ Error: autoindexing index

    Caused by:
        0: database is locked
        1: Error code 5: The database file is locked
```
### Re: `agent-doc-bugs2.md` autoindex lock — gpt-5

That error happened because `tsift search --autoindex ...` did not stay on the read-only search path. It first hit the stale-index precheck, then took the `autoindex` branch in `precheck_search_indexes()` and tried to run `apply_search_index_update()` against the root repo index (`src/tsift/src/main.rs:2669` and `src/tsift/src/main.rs:2677`).

At that point the root `agent-loop` index was already contended:

- `.tsift/index.db-journal` exists right now, which means the active writer is using rollback-journal mode instead of a clean WAL sidecar.
- there are other live `tsift search --autoindex ...` processes in this workspace right now, and both are stuck in `D` state:
  - `PID 927595` — `tsift search --autoindex corky gmail codex adapter`
  - `PID 2002091` — `tsift search --autoindex pending-done finalize session-check closeout`
- when a later `--autoindex` search tried to open the same DB for write/update, SQLite returned `Error code 5: database is locked`.

So the answer is: the `agent-doc-bugs2.md` failure was caused by `--autoindex` attempting a write while another root-repo tsift autoindex/update was already holding the DB in a lock-prone state. It was not a plain search timeout, and it was not just the read-only query path.

Why this still happens even after `#vxtz`:

- `#vxtz` fixed the normal query/status paths to use read-only opens with a busy timeout.
- `--autoindex` is intentionally different: it escalates into the write path when the index is stale or missing.
- the write path still has to acquire the SQLite writer lock, so if another autoindex/index process is already wedged, the later one fails exactly the way you saw.

So the narrower root cause for that note in `tasks/agent-doc/agent-doc-bugs2.md` is: concurrent root-level `--autoindex` writers, not a regression in the read-only search fix.

❯ Please add fix to backlog.
### Re: backlog fix — gpt-5

Added to backlog as `#alck`.

❯ do #alck. spec-test-build-install-commit-push
### Re: do #alck — gpt-5

Implemented the concurrent autoindex writer guard in tsift.

What changed:
- writable index updates now claim a sibling `.tsift/index.lock` sidecar before opening SQLite, so competing `tsift index` / `tsift search --autoindex` writers fail fast with a tsift-owned error instead of surfacing raw lock contention
- stale lock files are recovered automatically when the recorded PID is no longer alive
- `tsift index --check` now uses read-only DB opens, so hook-style freshness checks do not contend with active writers
- added regression coverage for both the fast-fail autoindex path and the read-only `index --check` path
- updated `SPEC.md` with the sidecar-lock behavior

Verification:
- `cargo test` passed: 303 unit tests + 8 exit-code tests
- `make check` passed
- `cargo install --path .` passed

I pushed the minimal `#alck` patch from a clean `origin/main` worktree because the checked-out `src/tsift` submodule still had a broader local uncommitted stack.

Commit: `btakita/tsift@9516710` pushed to `origin/main`.

❯ Please make autoindex default to true on `tsift search`. We should only have an opt-out option.

```
• Ran tsift search "claudescore-applies-when-plan"
  └ Error: tsift search aborted: index is stale (10 files). Run `tsift index /home/brian/work/btakita/agent-loop/src/
    session-share` or re-run with `--autoindex`.

• Ran tsift search "session digest"
  └ Error: tsift search aborted: index is stale (10 files). Run `tsift index /home/brian/work/btakita/agent-loop/src/
    session-share` or re-run with `--autoindex`.
```
### Re: search autoindex default — gpt-5

Implemented in tsift.

What changed:
- `tsift search` now autoindexes by default when the targeted index is missing or stale.
- added `--no-autoindex` as the explicit opt-out when you want the old fail-fast non-mutating behavior.
- kept `--autoindex` as a compatibility flag, but it is now redundant because default search already autoindexes.
- updated the stale-index messaging, `SPEC.md`, and `CLAUDE.md` to reflect the new default.
- added CLI/runtime coverage for default, opt-out, scoped, and federated search behavior.

Verification:
- `cargo test` passed: 307 unit tests + 10 exit-code tests.
- `make check` passed.
- `cargo install --path .` passed, and the local `tsift` binary now shows `--no-autoindex` in `tsift search --help`.

I pushed the patch from a clean `origin/main` worktree because the checked-out `src/tsift` submodule in `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@66fdd19` pushed to `origin/main`.

❯ Are we currently saving an internal version? Can we auto upgrade the skill/runbook(s) if the version is out of sync? See how src/agent-doc does this.
### Re: skill/runbook version sync — gpt-5

No. `tsift` is not currently saving an internal installed-instructions version.

What `agent-doc` does today:
- the bundled skill template carries `agent-doc-version` in frontmatter
- install rewrites that field from `CARGO_PKG_VERSION` and swaps in harness-specific auto-update instructions
- install also owns bundled runbooks, so `agent-doc skill install` can refresh the whole instruction surface and signal `SKILL_RELOAD=restart|compact`

What `tsift` does today:
- `tsift init` writes a hard-coded `DEFAULT_SECTION` into `AGENTS.md` / `CLAUDE.md`
- it only keys off the `<!-- tsift:code-navigation -->` markers and replaces that block in place
- there is no installed version marker, no runbook bundle, and no "check installed instructions vs binary" path

So yes, we can add the same pattern to `tsift`, but that would be new work rather than wiring up something that already exists.

The minimal design would be:
1. move the injected section into a bundled template with a `tsift-version` marker
2. have `tsift init` render that marker from `CARGO_PKG_VERSION`
3. add a check path that compares installed instructions vs binary version and re-runs `tsift init` when stale
4. if we want more than the single AGENTS/CLAUDE block, add bundled runbooks the way `agent-doc` does

If you want, I can turn that into a backlog item and implement it next.

❯ An agent-doc turn in /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md got this issue. Why?
```
• Ran tsift status
  └ Error: database is locked

    Caused by:
        Error code 5: The database file is locked
```
### Re: `agent-doc-bugs2.md` status lock — gpt-5

That one is the root-repo writer-lock case, not an autoindex precheck case.

`tsift status` itself stays on the read-only path: `status::check_status()` opens `.tsift/index.db` through `IndexDb::open_read_only()` and then reads `file_state` to compute freshness. It does not try to rebuild the index.

The reason it still failed is that the root `agent-loop` index is currently in a bad live lock state:
- there is still a long-running `tsift search --autoindex ...` process stuck in `D` state (`PID 927595`, now over an hour old)
- the root repo has `.tsift/index.db-journal`, which means that writer is holding the DB in rollback-journal mode instead of a clean WAL sidecar
- in that state, even a read-only status open/read can hit SQLite `Error code 5: database is locked`

So the immediate cause for that `agent-doc-bugs2.md` note is: `tsift status` tried to read the root `.tsift/index.db` while an already-wedged root writer still owned a lock-prone rollback-journal session.

That is different from the earlier `search --autoindex` failure:
- `search --autoindex` failed because it escalated into the write path and collided with another writer
- this `status` failure happened without any write attempt; the read-only status path itself was blocked by the existing bad writer state

So the short answer is: the current root `.tsift/index.db` is still contaminated by an older stuck writer, and `status` touched the same DB.

❯ It happened in the last cancelled turn on this document as well.

```
• Ran tsift status
  └ Error: database is locked

    Caused by:
        Error code 5: The database file is locked
```

❯ How can we fix this? Can tsift autofix the issue? Please test on this repo. spec-test-build-install-commit-push
### Re: status lock autofix — gpt-5

Implemented and tested in tsift.

Short answer: yes, tsift can auto-recover this specific health-check failure, but only in the safe read-only case.

What I changed:
- `tsift status` and `tsift index --check` now retry against a temporary snapshot copy of `.tsift/index.db` when the live DB is locked **and** a rollback-journal sidecar is present.
- that lets freshness checks complete instead of failing with raw `database is locked` output.
- tsift still does **not** auto-kill a live writer or delete an active journal. That would be unsafe.
- normal search/graph semantics were left alone; this fix is scoped to the read-only health-check path.

Why this is the right boundary:
- the current root-repo failure is caused by an older stuck rollback-journal writer.
- we can safely read from a copied snapshot for status/check.
- we cannot safely mutate or remove the live DB/journal while that writer may still exist.

Spec/docs/tests/build/install:
- updated `SPEC.md` and `CLAUDE.md`
- added regression coverage for the rollback-journal snapshot fallback
- `cargo test` passed: 308 unit tests + 10 exit-code tests
- `make check` passed
- `cargo install --path .` passed

Tested on this repo after install:
- `tsift status` now succeeds in `/home/brian/work/btakita/agent-loop` instead of failing with `database is locked`
- `tsift index --check .` also completed against the same locked root index

I pushed from a clean `origin/main` tsift worktree because the checked-out `src/tsift` submodule inside `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@12cb84f` pushed to `origin/main`.

❯ Any follow up tasks to put into the backlog?
### Re: backlog follow-ups — gpt-5

Yes. I would put these into the backlog:

1. `#sprch` — extend the same locked-DB snapshot fallback to search freshness prechecks (`inspect_search_index()`), so scoped/federated/`--no-autoindex` search paths do not regress relative to `status` and `index --check`.
2. `#sdia` — surface when snapshot fallback was used in `tsift status` human output and JSON. Right now the recovery is intentionally silent, but agents should be able to tell they are in a degraded-read path and still need a real index repair later.
3. `#tdoc` — add a small diagnostics/remediation command for lock states (`index.lock`, `index.db-journal`, live PID hints, recommended next step). The current behavior is safer now, but the operator story is still too implicit.

I added those as recommended backlog items in this cycle.

❯ sync orchestra
❯ preset #spec-test-build-install-commit-push
❯ - do #sprch
❯ - do #sdia
❯ - do #tdoc
### Re: `#sprch` + `#sdia` + `#tdoc` — gpt-5

Implemented the full locked-index follow-up stack in tsift.

What changed:
- `#sprch`: search freshness prechecks now reuse the same rollback-journal snapshot fallback as `tsift status` / `tsift index --check`, so root, scoped, federated, and `--no-autoindex` stale checks resolve to stale/missing/fresh instead of surfacing raw `database is locked` errors.
- `#sdia`: `tsift status` now surfaces snapshot fallback explicitly in human output (`recovery: snapshot fallback ...`) and JSON (`index.recovery: "snapshot_fallback"`).
- `#tdoc`: added `tsift locks [path] [--scope <submodule>] [--json]` to report `index.lock`, `index.db-journal`, PID hints, the exact reindex command, and the recommended next step.

Verification:
- `cargo test` passed: 317 unit tests + 10 exit-code tests.
- `make check` passed.
- `cargo build --release` passed.
- `cargo install --path .` passed.
- Installed-binary smoke check: `tsift locks /home/brian/work/btakita/agent-loop` reports the current root index has no active lock or rollback-journal state.

I pushed from a clean `origin/main` tsift worktree because the checked-out `src/tsift` submodule inside `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@9a18b25` pushed to `origin/main`.

❯ #follow-up-backlog
### Re: `#follow-up-backlog` — gpt-5

Yes. The next backlog items I’d add are:

1. `#vsync` — add a tsift instruction-version marker plus auto-refresh for injected `AGENTS.md` / `CLAUDE.md` content and any bundled runbooks, modeled on `agent-doc`.
2. `#subhk` — add a repo-wide install/update path for tsift hooks across active submodules so stale-index protection does not rely on manual setup in each one.
3. `#lerr` — surface lock-owner and rollback-journal diagnostics directly in autoindex/write-contention errors, without requiring a separate `tsift locks` follow-up.

I added those to the backlog in this cycle.

❯ do #lerr. spec-test-build-install-commit-push
### Re: do `#lerr` — gpt-5

Implemented in tsift.

What changed:
- all mutating index updates now flow through a shared wrapper, so `tsift index`, rebuild/prune paths, scoped/workspace indexing, and search autoindex all attach inline lock diagnostics on writer-lock or SQLite lock failures
- stderr now includes the same `lock` / `journal` / `run` / `next` block as `tsift locks`, so you get the live pid when available, rollback-journal presence, the exact reindex command, and the recommended next step without a separate follow-up command
- added regression coverage for both the explicit sidecar-lock case and the rollback-journal writer-block case
- updated `SPEC.md` and `CLAUDE.md`

Verification:
- `cargo test` passed: 318 unit tests + 11 exit-code tests
- `make check` passed
- `cargo build --release` passed
- `cargo install --path .` passed

Tested on this repo after install:
- `tsift index /home/brian/work/btakita/agent-loop` now fails with inline `lock diagnostics:` showing the live writer pid, lock path, reindex command, and retry guidance
- `tsift search --path /home/brian/work/btakita/agent-loop "agent-doc"` now fails through the same inline diagnostics path instead of only surfacing a bare lock error

I pushed from a clean `origin/main` tsift worktree because the checked-out `src/tsift` submodule inside `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@cfcad16` pushed to `origin/main`.

❯ do #subhk. spec-test-build-install-commit-push
### Re: do `#subhk` — gpt-5

Implemented in tsift.

What changed:
- `tsift init --codex` now upgrades existing `.codex/hooks.json` entries in place instead of treating any matching status message as already present, and it automatically switches to a workspace-wide hook when the resolved target has `.gitmodules`
- `tsift init --codex --workspace` now resolves through `git rev-parse --show-superproject-working-tree`, so you can run it from inside a submodule and still install the parent workspace hook
- generated Codex hook commands now use absolute roots, deduplicate duplicate tsift hooks, and report whether the installed hook is `project` or `workspace` scoped
- `examples/hooks/tsift-autoindex.sh` now auto-detects workspace roots and uses `--workspace` when appropriate, so one root hook covers initialized submodules
- updated `SPEC.md` and `CLAUDE.md`, and added init coverage for project/workspace creation, hook updates, dedupe, and the no-submodules error case

Verification:
- `make check` passed: 325 unit tests + 11 exit-code tests
- `cargo build --release` passed
- `cargo install --path .` passed

Smoke checks:
- `tsift init --codex <tmp>` created a project hook with absolute-path `tsift index --check --exit-code <root>`
- `tsift init --codex --workspace <tmp>` created a workspace hook with `tsift index --check --exit-code --workspace <root>`

I pushed from a fresh `origin/main` clone because the checked-out `src/tsift` submodule still had unrelated local edits and `origin/main` moved during the turn.

Commit: `btakita/tsift@7145b9c` pushed to `origin/main`.
### Re: do `#subhk` — gpt-5

Implemented in tsift.

What changed:
- `tsift init --codex` now upgrades existing `.codex/hooks.json` entries in place instead of treating any matching status message as already present, and it automatically switches to a workspace-wide hook when the resolved target has `.gitmodules`
- `tsift init --codex --workspace` now resolves through `git rev-parse --show-superproject-working-tree`, so you can run it from inside a submodule and still install the parent workspace hook
- generated Codex hook commands now use absolute roots, deduplicate duplicate tsift hooks, and report whether the installed hook is `project` or `workspace` scoped
- `examples/hooks/tsift-autoindex.sh` now auto-detects workspace roots and uses `--workspace` when appropriate, so one root hook covers initialized submodules
- updated `SPEC.md` and `CLAUDE.md`, and added init coverage for project/workspace creation, hook updates, dedupe, and the no-submodules error case

Verification:
- `make check` passed: 325 unit tests + 11 exit-code tests
- `cargo build --release` passed
- `cargo install --path .` passed

Smoke checks:
- `tsift init --codex <tmp>` created a project hook with absolute-path `tsift index --check --exit-code <root>`
- `tsift init --codex --workspace <tmp>` created a workspace hook with `tsift index --check --exit-code --workspace <root>`

I pushed from a fresh `origin/main` clone because the checked-out `src/tsift` submodule still had unrelated local edits and `origin/main` moved during the turn.

Commit: `btakita/tsift@7145b9c` pushed to `origin/main`.

❯ do #vsync. spec-test-build-install-commit-push
### Re: do `#vsync` — opus-4-6

Implemented in tsift.

What changed:
- `tsift init` now embeds `v=X.Y.Z` in the `<!-- tsift:code-navigation -->` opening marker, using the binary's Cargo.toml version
- `tsift status` reports a new `instructions` field: `current`, `stale` (with found/expected versions), or `missing`
- When instructions are stale or missing, `tsift init` is prepended to the `run:` recommendation
- Pre-versioned markers (old format without `v=`) are detected and treated as stale
- `tsift init` upgrades pre-versioned sections in place on re-run

Verification:
- `cargo clippy -- -D warnings` passed
- `cargo test` passed: 349 tests (11 new for version extraction, status integration, upgrade paths)
- `cargo build --release` passed
- `cargo install --path .` passed

Live verification after install:
- `tsift status /home/brian/work/btakita/agent-loop` reports `instructions: missing (run tsift init)` with `run: tsift init && tsift index .`
- `tsift status --json --pretty` includes `"instructions": {"state": "current", "version": "0.1.0"}` after init

Updated SPEC.md (version markers section), CLAUDE.md (command table), and VERSIONS.md.

I pushed from a clean `origin/main` tsift worktree because the checked-out `src/tsift` submodule still has unrelated local edits.

Commit: `btakita/tsift@f27c761` pushed to `origin/main`.

❯ do #code-review
### Re: \`#code-review\` — opus-4-6

Reviewed all 10 source files (~11.4K lines), integration tests, spec, and dependencies. Findings organized by severity.

#### Critical (data corruption / incorrect behavior)

1. **No SQLite transactions in \`apply_changes_inner\`** — \`index.rs:395-478\`. Multiple DELETEs and INSERTs per file run without a transaction. A crash between \`delete_symbols\` and \`insert_symbol\` leaves the index missing symbols for that file. Same issue in \`rebuild\` (\`index.rs:497-501\`): sequential deletes without atomicity.

2. **Inverted path filter in diff-mode summarize** — \`main.rs:2309-2318\`. The condition \`extract_path.starts_with(f.parent().unwrap_or(f))\` is backwards. If extracting a directory, it should filter files *within* that directory. Currently filters the opposite way, causing wrong files to be summarized in \`--diff\` mode.

3. **Partial DB writes in summarize loop** — \`main.rs:2353-2362\`. \`summary_db.delete_by_file(&rel_path)\` runs before the insert loop. If any \`summary_db.insert(&s)\` fails mid-loop, old data is deleted but not replaced. Needs a transaction wrapper.

4. **API response status not checked** — \`summarize.rs:425-440\`. HTTP 4xx/5xx from the Anthropic API is deserialized as JSON without status code validation. The \`response["content"]\` extraction silently returns \`""\` on error responses, producing empty summaries stored as if they succeeded.

#### High (concurrency / resource)

5. **Write lock vulnerable to PID recycling** — \`index.rs:750-775\`. Lock stores PID, checks \`/proc/<pid>\` on Linux. If a PID recycles, the stale lock is never cleared. On non-Linux, \`process_exists()\` always returns \`true\`, so stale locks permanently block. Two threads can also race through \`clear_stale_lock_if_possible\` simultaneously.

6. **Unbounded symbol loading in \`symbol_search\`** — \`index.rs:610-695\`. Loads every row from \`symbols\` table into memory with no LIMIT. On a large codebase (100K+ symbols), this allocates 20MB+ per search call.

7. **Search timeout thread never joined** — \`main.rs:5028-5030\`. \`std::thread::spawn\` runs the Sift search with a channel timeout, but the thread is never joined on timeout. The search continues consuming CPU/memory in the background. Multiple timeouts accumulate leaked threads.

8. **WAL checkpoint never triggered** — \`index.rs:117\`. WAL mode enabled but no \`wal_autocheckpoint\` pragma set and no manual checkpoint. The \`-wal\` file grows unbounded across sessions.

#### Medium (edge cases / silent failures)

9. **Silent file read/parse failures** — \`index.rs:435-436, 456-457\`. \`std::fs::read(&change.path).ok()\` and \`extract_call_sites().ok()\` swallow permission errors and parse failures. Files are marked as indexed in \`file_state\` but have no symbols or edges. No warning emitted.

10. **Directory mtime pruning fragility** — \`walk.rs:107-112\`. Mtime equality check for pruning decisions is filesystem-dependent. HFS+ has 1-second granularity; a directory modified within the same second appears unchanged and gets pruned, missing new files.

11. **File metadata failure masks stale index** — \`status.rs:81-87\`. If \`.metadata()\` or \`.modified()\` fails, returns 0 seconds ago (appears freshly indexed). Could prevent users from noticing a stale or corrupted index.

12. **SQL table name interpolation** — \`main.rs:4567, 4581\`. \`format!("SELECT COUNT(*) FROM \\\"{}\\\"", tbl)\` uses string interpolation for table names. A table name containing \`"\` breaks the query. Low practical risk since table names come from the schema, but the pattern is fragile.

13. **Init unwrap after contains** — \`init.rs:165\`. \`content.find(SECTION_MARKER_PREFIX).unwrap()\` after a \`contains()\` check. Safe in single-threaded CLI, but panics on impossible-but-theoretically-concurrent modification. Should use \`if let\`.

14. **Kotlin tree-sitter queries may miss interfaces** — \`lang/mod.rs:161-165\`. Pattern \`(class_declaration "interface" ...)\` looks for literal "interface" as child of \`class_declaration\`, but Kotlin's grammar uses a separate node type for interfaces.

15. **Token estimation is bytes/4** — \`summarize.rs:280\`. Divides byte count by 4, which is inaccurate for multi-byte UTF-8 and operator-heavy code. Borderline files may exceed the API context window.

#### Architecture observations

- **No E2E test coverage for core commands.** Integration tests only cover \`index --check\` and lock behavior. Search, graph, explain, communities, path, edit, sql, lint, audit, summarize, status, and init have zero end-to-end tests. Unit tests exist for internal logic but don't exercise the CLI→index→search→output pipeline.

- **\`sift\` is a git dependency** (Cargo.toml), not versioned on crates.io. HEAD changes can silently break tsift without any version gate.

- **Edition 2024** in Cargo.toml requires nightly Rust. No CI shown to enforce this.

- **Parser created per file** in \`lang/mod.rs:201-209\`. A new \`Parser\` + \`set_language()\` call for every file during indexing. Cacheable for modest throughput gain on large indexes.

❯ The tmux pane for this document has crashed 2 times. #agent-doc-bug
### Re: tmux pane crashes — gpt-5

I treated this as an `agent-doc` issue rather than a `tsift` backlog item.

Tracked externally:
- added `#tmuxcrash` to `tasks/agent-doc/agent-doc-bugs2.md`
- created plan: `tasks/agent-doc/plan-tmux-pane-crash-stability.md`

The plan is intentionally diagnostic first:
- reconstruct both crash episodes from the session/supervisor logs and tmux ownership state
- distinguish child-harness exit vs supervisor exit/panic vs tmux pane replacement during route/sync recovery
- add crash provenance logging and a short-window crash-loop fail-closed guard so repeated pane deaths stop churning tmux state

I’m not claiming a root cause from the symptom alone yet. The main fix target is the tmux/supervisor boundary in `src/agent-doc`, not `tsift` itself.

❯ do #txn. spec-test-build-install-commit-push
### Re: do #txn — gpt-5

Implemented in tsift.

What changed:
- `apply_changes` now wraps the full file-state / symbol / edge / dir-state mutation batch in a SQLite savepoint and rolls back the whole batch on any mid-update failure.
- `rebuild` now wraps its table-clearing phase in an outer savepoint, so a failure after the bulk DELETEs still restores the prior index.
- added rollback regression tests that inject failures inside both paths and prove the old index contents survive.
- updated `SPEC.md` and `CLAUDE.md` to document the transactional index contract.

Verification:
- `make check` passed: 342 unit tests + 11 exit-code tests.
- `cargo build --release` passed.
- `cargo install --path .` passed.

I pushed from a clean `origin/main` worktree because the checked-out `src/tsift` submodule in `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@b4a3c5b` pushed to `origin/main`.

❯ do #diffpath. spec-test-build-install-commit-push
### Re: do #diffpath — gpt-5

Implemented in tsift.

What changed:
- `tsift summarize --extract <path> --diff` now resolves the requested extract scope against the project root before filtering changed files, so relative file and directory targets match correctly.
- removed the inverted parent/child path check and replaced it with a direct "changed file is inside requested scope" match.
- added regression tests for relative directory and relative file scopes.
- updated `SPEC.md` and `CLAUDE.md` so `--extract --diff` explicitly documents path-scoped behavior.

Verification:
- `make check` passed: 344 unit tests + 11 exit-code tests.
- `cargo build --release` passed.
- `cargo install --path .` passed.

I pushed from a clean `origin/main` worktree because the checked-out `src/tsift` submodule in `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@d15a66d` pushed to `origin/main`.

❯ do #sumtxn. spec-test-build-install-commit-push
### Re: do #sumtxn — gpt-5

Implemented in tsift.

What changed:
- `SummaryDb::replace_file` now wraps each file's summary-cache rewrite in a SQLite savepoint, so `tsift summarize --extract` no longer leaves a file empty if an insert fails after the delete.
- `cmd_summarize` now stages the per-file summary rows, swaps them in atomically, and only then increments the extraction counters.
- added a regression test that injects a mid-replacement failure and proves the prior file summaries survive.
- updated `SPEC.md` and `CLAUDE.md` to document the transactional per-file cache rewrite.

Verification:
- `make check` passed: 345 unit tests + 11 exit-code tests.
- `cargo build --release` passed.
- `cargo install --path .` passed.

I pushed from a clean `origin/main` worktree because the checked-out `src/tsift` submodule in `agent-loop` still has unrelated local edits.

Commit: `btakita/tsift@62c35bf` pushed to `origin/main`.

❯ commit all uncommited files in src/tsift + push
### Re: commit all uncommited files in src/tsift + push — gpt-5

Done.

What changed:
- committed the checked-out `src/tsift` worktree and rebased it onto `origin/main` before pushing, because the local checkout was still 12 commits behind remote.
- the rebase conflicted in files that had already changed upstream; after resolving against the current `origin/main` side, the remaining net local delta was in `src/config.rs`, `src/graph.rs`, `src/lang/mod.rs`, `src/lint.rs`, and `src/walk.rs`.
- `src/tsift` is now clean and aligned with `origin/main`.

Verification:
- `make check` passed: 345 unit tests + 11 exit-code tests.
- `cargo build --release` passed.

Commit: `btakita/tsift@8e0c5ef` pushed to `origin/main`.

❯ do #apicheck. spec-test-build-install-commit-push
### Re: do #apicheck — gpt-5

Implemented in tsift.

What changed:
- `call_anthropic_api()` now reads the response body explicitly and checks the HTTP status before trying to parse summary content.
- non-2xx Anthropic responses now fail closed with the HTTP status, API error type, and API message when present; successful responses keep the existing content-cleanup and token-accounting path.
- added regression tests for HTTP error bodies, missing error-message fallback, and normal success parsing.
- updated `SPEC.md` and `CLAUDE.md`.

Verification:
- `make check` passed: 348 unit tests + 11 exit-code tests.
- `cargo build --release` passed.
- `cargo install --path .` passed.

Commit: `btakita/tsift@beb3bb5` pushed to `origin/main`.
<!-- agent:boundary:c59cbfeb -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [x] [#apicheck] Check HTTP status before parsing API response (summarize.rs:425-440)
- [ ] [#lockfix] Replace PID-based write lock with flock() or SQLite pragma exclusive (index.rs:698-775)
- [ ] [#symlimit] Add LIMIT to symbol_search query (index.rs:610-695)
- [ ] [#threadjoin] Join or abort search timeout thread (main.rs:5028-5030)
- [ ] [#walckpt] Set wal_autocheckpoint pragma (index.rs:117)
- [ ] [#silentok] Log warnings on silent .ok() file read/parse failures (index.rs:435-436, 456-457)
- [ ] [#e2e] Add E2E integration tests for search, graph, explain, communities, path commands
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
