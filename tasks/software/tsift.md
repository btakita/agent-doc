---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
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
<!-- agent:boundary:9bb648be -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [x] [#stal] [recommended] Add built-in stale precheck and opt-in auto-reindex path for unhooked tsift sessions
- [ ] [#vxtz] [recommended] Investigate why `tsift index .` enters D-state and leaves `.tsift/index.db-journal` on agent-loop root
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
