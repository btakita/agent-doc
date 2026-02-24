---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

v0.1.1 released. Core CLI functional. Dog-fooding via `/agent-doc plan.md`. 78 tests passing.

## What's Next

### agent-doc

1. Make `audit-docs` work outside Rust projects (currently requires Cargo.toml)
2. Codex frontend skill (same pattern as Claude skill)

### Future

- Streaming responses
- Document-only permission flow (no terminal)
- Desktop notifications on completion
- Multi-agent documents (sections routed to different agents)
- Email transport for async human-agent loops (bridge with corky)

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `AGENTS.md`

## User

Read https://x.com/heygurisingh/status/2025572300658287030 and look at the image in the tweet.

Please update our AGENTS.md with these instructions. I want the improve the experience for everyone.

Is there anything else to do from plan.md? Please clear out anything that's been done. Move the info to the README.md, SPECS.md, etc.

## Assistant

Done. Here's what happened:

**AGENTS.md updated** with workflow orchestration from Boris Cherny's CLAUDE.md template (via the tweet). Added:
- **Workflow Orchestration** — 6 sections: Plan Mode Default, Subagent Strategy, Self-Improvement Loop, Verification Before Done, Demand Elegance, Autonomous Bug Fixing
- **Task Management** — 6-step flow: Plan First, Verify Plan, Track Progress, Explain Changes, Document Results, Capture Lessons
- **Core Principles** — Simplicity First, No Laziness, Minimal Impact
- **Conventions** — branch naming (`yyyymmdd/short-description`), `make check`, `make precommit`

Adapted to our conventions: uses auto memory instead of `tasks/lessons.md`, uses `plan.md` instead of `tasks/todo.md`, references our existing tooling.

**plan.md cleaned up** — removed ~130 lines:
- Removed Decisions table (already in `agent-doc/SPECS.md`)
- Removed Repo Structure block (already in `SPECS.md`)
- Removed Modules/CLI blocks (already in `agent-doc/CLAUDE.md` and `agent-doc/SPECS.md`)
- Removed 25-line Completed checklist (historical, info preserved in git)
- Removed 6 struck-through Immediate items (all done)
- Added Reference section pointing to the canonical locations
- Trimmed conversation history

**Nothing else outstanding from plan.md.** The only remaining items are future work (agent-doc: audit-docs portability, Codex skill) and long-term features (streaming, notifications, multi-agent, email transport).

## User

commit and push changes.

subagents:
1. Make `audit-docs` work outside Rust projects (currently requires Cargo.toml)
2. Codex frontend skill (same pattern as Claude skill)
3. corky: Telegram integration strategy. Support export/import. API integration would be nice.
4. corky: Slack integration strategy. Support export/import. API integration would be nice.

## Assistant

Committed and pushed. All 4 research subagents completed. Here are the findings:

### 1. audit-docs portability (agent-doc)

**Problem**: `find_root()` hard-exits if no `Cargo.toml` found. `check_staleness()` only scans `.rs` files.

**Plan** (3 changes to `agent-doc/src/audit_docs.rs`):

1. **Replace `find_root()`** with multi-strategy root finder:
   - Pass 1: Walk up from CWD looking for project markers (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Gemfile`, `pom.xml`, `Makefile`, etc.)
   - Pass 2: Walk up looking for `.git` directory
   - Pass 3: Fall back to CWD with warning (no hard exit)
   - Two-pass design avoids monorepo `.git` being chosen over a nested project's `Cargo.toml`

2. **Replace `check_staleness()`** with language-agnostic scanner:
   - Scan multiple source dirs: `src/`, `lib/`, `app/`, `pkg/`, `cmd/`, `internal/`
   - Scan 25+ file extensions: `.rs`, `.ts`, `.tsx`, `.js`, `.py`, `.go`, `.rb`, `.java`, etc.
   - Skip: `node_modules/`, `target/`, `build/`, `dist/`, `.git/`, `__pycache__/`, `vendor/`

3. **Add `--root` CLI flag** to override root detection when needed

### 2. Codex frontend skill (agent-doc)

**Problem**: agent-doc only has a Claude backend. Need a Codex backend following the same pattern.

**Plan**:

1. **Create `src/agent/codex.rs`** implementing the `Agent` trait:
   - Non-interactive: `codex exec --json --full-auto -` (prompt via stdin)
   - Session resume: `codex exec resume <session_id>` (vs Claude's `--resume`)
   - Output: JSONL stream (vs Claude's single JSON blob)
   - Extract `thread_id` from `thread.started` event, response text from `item.completed` events
   - System prompt: Prepend instructions to user prompt (no `--append-system-prompt` equivalent)

2. **Update `src/agent/mod.rs`**: Add `pub mod codex;` and `"codex"` match arm in `resolve()`

3. **Update `CLAUDE.md`**: Add `codex.rs` to module layout diagram

Key difference table:

| Feature | Claude CLI | Codex CLI |
|---------|-----------|-----------|
| Non-interactive | `claude -p` | `codex exec` |
| JSON output | single JSON blob | JSONL stream |
| Session resume | `--resume <id>` | `codex exec resume <id>` |
| Session fork | `--continue --fork` | not available in exec mode |
| Response field | `.result` | `item.completed` events |
| Session ID | `.session_id` | `thread.started.thread_id` |

### 3. Telegram integration (corky)

**Recommended crate**: `grammers` (MTProto user API -- full history access, unlike bot API)

**Phased plan**:

- **Phase 1: Offline import** (`corky sync telegram-import FILE`) -- parse Telegram Desktop JSON exports. Zero new deps. Thread ID: `tg:{chat_id}`. Reuses `merge_message_to_file()`.
- **Phase 2: Live sync** via `grammers-client` -- connect with user auth, iterate dialogs, fetch history incrementally. Feature-gated: `[features] telegram = ["grammers-client", "grammers-session"]`. Sync state: per-chat `last_message_id`.
- **Phase 3: Watch** -- extend `corky watch` to call `sync_telegram_account()`. Or use `client.next_update()` for real-time push.
- **Phase 4: Send** -- extend `draft push` to detect `tg:` prefix in `**To**` field and send via grammers.

Config: `[accounts.tg-personal] provider = "telegram"`, with `api_id`, `api_hash_cmd`, `phone`, optional `chats`/`chat_ids` filters.

### 4. Slack integration (corky)

**Recommended crate**: `slack-morphism` (full type-safe Web API + Socket Mode)

**Phased plan**:

- **Phase 1: Export/Import** (`corky slack import FILE.zip`) -- parse Slack workspace export ZIPs. Only needs `zip` crate. Thread ID: `slack:{channel_id}:{thread_ts}`. Groups messages by `thread_ts`. Subject = first line of parent message.
- **Phase 2: API sync** via `slack-morphism` -- `conversations.history` + `conversations.replies`, incremental via `oldest` param. Feature-gated. Needs Slack mrkdwn-to-plain text conversion.
- **Phase 3: Socket Mode** -- real-time push via WebSocket. Needs app-level token (`xapp-...`). No public HTTP endpoint needed.

**Rate limit warning**: Slack imposed harsh limits in May 2025 -- non-Marketplace apps get 1 req/min on `conversations.history`. Mitigation: register as an internal app (retains Tier 3: ~50 req/min).

Config: `[accounts.slack-work] provider = "slack"`, reuse `password_cmd` for bot token, `labels` for channel filter.

### Both Telegram and Slack share the same architecture pattern:

- Thread IDs namespaced with prefix (`tg:`, `slack:`) to avoid collisions with email
- Feature-gated Cargo dependencies to keep default binary small
- `merge_message_to_file()` reused for all dedup/routing/manifest infrastructure
- Export/import first (validates data model), live sync second

## User

