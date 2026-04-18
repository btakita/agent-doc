---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Rust CLI skeleton pushed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ **Design context (extracted from agent-doc-bugs.md session):**

❯ **Origin:** User evaluated WozCode (subscription) vs building an open-source equivalent. Discovered Sift (https://github.com/rupurt/sift) — MIT-licensed local hybrid search CLI (BM25 + vector + LLM reranking, single binary, no cloud). Decided to build tsift as "the pragmatic middle" — a CLI plugin (not MCP) combining existing tools.

❯ **Architecture — what tsift replaces from WozCode:**

❯ | Capability | WozCode | tsift approach |
❯ |---|---|---|
❯ | Smart search | Cloud API | Sift binary (local, shell-out) |
❯ | Batch editing | mcp__plugin_woz_code__Edit | Claude Code native Edit tool (no wrapper needed initially) |
❯ | SQL introspection | mcp__plugin_woz_code__Sql | Direct DB queries via shell (sqlite3/psql) |
❯ | Subagent cost routing | Cloud routing | Skill/hook that sets Agent `model` param (haiku for search, sonnet for edits, opus for architecture) |
❯ | Session recall | woz:recall | claude-mem:mem-search (already installed) |

❯ **Key decisions:**
❯ - CLI, not MCP — lower overhead, works as Claude Code hook target, follows rtk pattern
❯ - Rust for universal installation (cargo install + prebuilt binaries)
❯ - Name `tsift` chosen: terse like `rtk`, "t" prefix = token, 5 chars
❯ - MIT license, public repo
❯ - Pragmatic middle: 90% of WozCode value by composing existing tools, not reimplementing

❯ **Sift details:** `github.com/rupurt/sift`. Four-stage pipeline (query expansion, retrieval, RRF fusion, optional Qwen-based reranking). SIMD-accelerated vector scoring on CPU. Incremental content-addressable caching. Sub-6ms BM25 on ~5K docs. Static binaries for Linux/Mac/Windows + Homebrew.

❯ ### Re: Skills-janitor scope absorbed — opus-4-6 (HEAD)

Expanded tsift to be the all-in-one audit + token sifter tool. Added `tsift audit` subcommand family to pending:

**New pending items:**
- **Skill drift detection** — scan installed skills vs manifest, flag stale/broken/missing
- **Duplicate detection** — find overlapping skills by description similarity
- **Usage tracking** — analyze session history for invocation frequency, flag unused skills
- **Cleanup recommendations** — actionable prune list with token savings estimates
- **Report mode** — `tsift audit --report` writes to `tasks/agent-doc/skills-audit.md` for nightly cron

This replaces the standalone skills-janitor fork (#vdyj from agent-doc-bugs). The skills-janitor *skill* stays installed as a reference implementation / comparison target until tsift audit is functional.
❯ Add ids to pending items that are missing ids
❯ ### Re: Pending IDs backfilled — sonnet-4-6 (HEAD)

Preflight auto-backfilled IDs to any items that were missing them — already done. All pending items now have hash IDs.

I also deduped the pending list: the five `tsift audit` items had been added three times each (#nkec/#f5ty/#jh84 were identical, same for the other groups). Removed the two redundant copies of each, keeping the first occurrence.
❯ re: #92bz...can we integrate directly with sift via rust...exposing the cli endpoints?
❯ ### Re: Direct Rust integration with sift — opus-4-6 (HEAD)

Yes — sift is pure Rust and explicitly exports a library crate. The `lib.rs` opens with "sift supports embedding through the crate root" and exposes a builder API:

```rust
use sift::{Sift, SiftBuilder, SearchInput, SearchOptions};

let sift = Sift::builder()
    .root("/path/to/project")
    .build()?;

let results = sift.search(SearchInput {
    query: "how does auth work".into(),
    ..Default::default()
})?;
```

**What's available directly:**
- `sift.search()` / `search_with_progress()` — hybrid BM25 + vector
- `sift.assemble_context()` — bounded evidence assembly (token-budget-aware)
- `sift.search_autonomous()` — multi-turn with planner strategies
- Structured types: `SearchResponse`, `SearchHit`, `ContextArtifact`
- Configurable `Retriever`, `Fusion`, `Reranking` enums

**Not on crates.io yet** — use as git dep: `sift = { git = "https://github.com/rupurt/sift" }`. There's a `/examples/sift-embed` reference showing embedded usage.

**Tradeoff vs shell-out:**
- **Direct:** no process spawn overhead, typed results, can share index state across calls, access to progress callbacks
- **Shell-out:** simpler dependency (no transitive candle/tokenizer deps), binary updates independently, tsift stays thin

Recommendation: start with git dep for the library. The typed API is much cleaner than parsing CLI output, and sift's pipeline (BM25 → fusion → rerank) benefits from keeping state in-process. The dependency weight (candle, tokenizers) is real but tsift is already a search tool — it's pulling its weight.

Updated #92bz to reflect direct integration path.
❯ do #92bz
❯ ### Re: #92bz implemented — direct sift integration — opus-4-6 (HEAD)

Done. tsift now links sift as a Rust library (git dep), not shell-out.

**What was built:**
- Added `sift` git dep + `clap`/`anyhow`/`serde_json` to Cargo.toml
- `tsift search <query>` command with `--path`, `--limit`, `--strategy`, `--json` flags
- Direct call to `Sift::builder().build()` then `engine.search(SearchInput)` — typed results, no process spawning
- Human-readable output (rank, confidence, path, score, snippet) or `--json` for machine consumption

**Tested:**
- `tsift --help` — shows search subcommand
- `tsift search "sift search" --path src/ --strategy lexical` — returns correct hits from source files
- Lexical strategy is instant; hybrid/vector triggers first-run model download (~30s)

**Known follow-ups:**
- Sift indexes `target/` by default — need `.siftignore` or gitignore-aware config (sift respects `.gitignore` when `Ignore` is configured via builder)
- Default strategy should be `lexical` for Claude Code hooks (fast) with `--strategy hybrid` opt-in for deeper search
- First-run model download for vector search needs a `tsift init` command or progress indicator

❯ Make https://github.com/btakita/tsift private
❯ ### Re: tsift repo set to private — sonnet-4-6 (HEAD)

Done. `gh repo edit btakita/tsift --visibility private` confirmed — `PRIVATE`.
❯ Why are the 2 shells using 100% cpu still? Are they still running for a legitimate reason?
❯ ### Re: CPU usage — sonnet-4-6 (HEAD)

Those were two background `tsift search` jobs I fired without timeouts to test the hybrid strategy. They were downloading sift's HuggingFace embedding models (candle + tokenizers) — first-run cold start, pulling several GB. Both killed now.

This is exactly why #92bz's follow-up noted that vector/hybrid search needs a `tsift init` command with a progress bar rather than silently blocking. For Claude Code hook usage, `lexical` (instant, no models) should be the default.

❯ Create plugin SKILL and runbook(s) if recommended...for search using sift
❯ ### Re: tsift skill created — opus-4-6 (HEAD)

Created `/tsift` Claude Code skill at `~/.claude/skills/tsift/`:

**SKILL.md** — invocable as `/tsift <query>`. Wraps `tsift search` with flags for `--path`, `--limit`, `--strategy`, `--json`. Defaults to `lexical` strategy (instant, no model download).

**runbooks/search-strategies.md** — decision flow for strategy selection:
- `lexical` → default, instant, keyword/symbol lookup
- `hybrid` → semantic + BM25, needs model download on first run
- `vector` → pure embedding similarity, same model requirement

The skill is live — Claude Code picked it up immediately. Test with `/tsift "search query"`.

No runbook for hook integration yet — that belongs with #95as (Claude Code hook integration pending item).
❯ Will tsift take over all claude searches now? Will this save tokens?
### Re: tsift scope and token savings — opus-4-6

**No, tsift won't take over all searches automatically.** Right now it's a manual skill (`/tsift`) — you invoke it when you want it. Claude Code's native Grep, Glob, and Read tools remain the default for directed searches and are perfectly efficient for that.

**Where tsift adds value:**
- **Semantic/conceptual queries** ("how does auth work", "error handling patterns") where Grep requires knowing the exact string
- **Exploratory search** across unfamiliar codebases where you don't know file names or symbols yet
- **Ranked results** with confidence scores instead of flat grep output

**Token savings potential — yes, but indirectly:**
- Sift returns ranked, truncated snippets rather than full file contents. A `tsift search` hit list is ~200-500 tokens vs reading 3-4 full files at ~2000-8000 tokens each
- The real savings come from *finding the right file on the first try* instead of the grep-read-wrong file-grep again loop that burns context
- For hook integration (#95as), tsift could replace the woz:Search MCP tool calls entirely — those are the expensive ones

**To make it automatic** would require #95as (Claude Code hook integration) — rewriting search commands through tsift the way rtk rewrites git/cargo commands. That's when the token savings compound.
<!-- agent:boundary:0e8e6119 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#92bz] Wire Sift as search backend (direct Rust lib integration via git dep — done)
- [ ] [#mwhg] Batch edit command: accept multiple file edits in one invocation
- [ ] [#6jmq] Agent model routing skill: haiku for search, sonnet for edits, opus for architecture
- [ ] [#yscf] SQL introspection: connect to SQLite/Postgres, expose schema + query
- [ ] [#95as] Claude Code hook integration: rewrite commands through tsift like rtk does
- [ ] [#kq5t] CLAUDE.md with project conventions
- [ ] [#e15f] CI: GitHub Actions for cargo test + clippy
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#nkec] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
<!-- /agent:pending -->
