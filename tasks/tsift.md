---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
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

### Re: Skills-janitor scope absorbed — opus-4-6

Expanded tsift to be the all-in-one audit + token sifter tool. Added `tsift audit` subcommand family to pending:

**New pending items:**
- **Skill drift detection** — scan installed skills vs manifest, flag stale/broken/missing
- **Duplicate detection** — find overlapping skills by description similarity
- **Usage tracking** — analyze session history for invocation frequency, flag unused skills
- **Cleanup recommendations** — actionable prune list with token savings estimates
- **Report mode** — `tsift audit --report` writes to `tasks/agent-doc/skills-audit.md` for nightly cron

This replaces the standalone skills-janitor fork (#vdyj from agent-doc-bugs). The skills-janitor *skill* stays installed as a reference implementation / comparison target until tsift audit is functional.
<!-- agent:boundary:1fa78a30 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#92bz] Wire Sift as search backend (shell-out to `sift` binary, parse output)
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
- [ ] [#f5ty] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#aed6] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#hfh5] tsift audit: usage tracking — analyze session history for skill invocation frequency, flag never-used skills
- [ ] [#9xdz] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#y7sf] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
- [ ] [#jh84] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#q82z] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#214d] tsift audit: usage tracking — analyze session history for skill invocation frequency, flag never-used skills
- [ ] [#hzhx] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#mdth] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
<!-- /agent:pending -->
