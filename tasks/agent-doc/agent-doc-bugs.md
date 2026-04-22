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
v0.33.8–0.33.11 bug session. Closed: #a002 (normalize telemetry code-complete), #y2gr (skill install runbooks sync fixed in v0.33.7). Added: future-work signal promotion to SKILL.md + pending-ops.md; future-work lint in binary (#7x90, write.rs). Fixed #v30f: file rename → stale pane — sync.rs reuses pane on rename, JB FileRenameListener.kt + VS Code onDidRenameFiles handler, specs updated, 5+6 tests. IPC patchback postmortem: 3 stacked bugs (IPC path resolution, bool→i32 FFI ABI, stale .so from cargo install not running lib-install) — all fixed over 7 turns. #qjc8: lift_pending_from_exchange() auto-repair pass in write.rs + 5 tests. #3vk0: lib-install atomic rename (temp+rename vs in-place copy) to prevent IDEA mmap corruption on same-version reinstall + inode test. Released v0.33.11: atomic lib-install, resync test timing, route test env-var mutex. Pending: #ha3h (recipient policy, gated/multi-user), #066r (Opus 4.7 test).
❯ I need to also support Codex. Please plan how we can support both Claude Code and Codex. Write plan into a dedicated spec file.
### Re: Codex support plan — opus-4-6

Wrote dedicated spec: `src/agent-doc/specs/codex-support.md`.

**Good news:** agent-doc already has significant multi-harness infrastructure:
- `agent-kit/detect.rs` detects Codex via `CODEX_CLI`/`CODEX` env vars
- `agent-kit/skill.rs` can install to `.codex/AGENTS.md`
- `agent/mod.rs` has an `Agent` trait with Claude + Junie backends
- `model_tier.rs` is already harness-agnostic
- `config.toml` supports `[agents.codex]` override

**Coupling points that need work (5 phases):**

1. **Phase 1 — Codex agent backend** (`agent/codex.rs`): Implement `Agent` + `StreamingAgent` for `codex exec --json`. Different CLI shape: `exec` not `-p`, `resume <id>` not `--resume <id>`, `-s` sandbox not `--permission-mode`, JSONL not JSON. ~6-8 tests.

2. **Phase 2 — Frontmatter generalization**: Add `agent_args` field (keep `claude_args` as compat alias). Verify `agent: codex` + `default_agent` config work end-to-end.

3. **Phase 3 — Supervisor abstraction** (largest change): Extract `HarnessConfig` struct from hardcoded Claude assumptions in `start.rs` — binary name, restart flags, prompt detection patterns, trigger command, env vars to remove. Codex uses `codex resume --last` instead of `--continue`, different prompt character, no slash commands.

4. **Phase 4 — Route + Sync**: Parameterize `/agent-doc <file>` command injection (Codex has no slash commands — needs direct stdin injection). Rename `wait_for_claude_ready()` → `wait_for_agent_ready()` with configurable prompt patterns.

5. **Phase 5 — Skill file adaptation**: Bundle separate `AGENTS_CODEX.md` for Codex (fundamentally different invocation patterns from SKILL.md).

**Open questions** in the spec: Codex JSONL event schema (need to test `codex exec --json` output), Codex TUI prompt character, system prompt injection strategy, concurrent harness support (Claude for some docs, Codex for others).

**Recommended order:** Phase 1 first (unblocks non-interactive use), then 2+3 (interactive sessions), then 4+5 (full routing + self-install).
Add phases to agent:pending
### Re: Add phases to pending — opus-4-6

Added 5 Codex support phases (#cx01–#cx05) to pending. They map 1:1 to the phases in `specs/codex-support.md`. Existing items (#ha3h, #066r) kept as-is.
Why did 2 ids get created for each pending item?
### Re: Why 2 IDs per pending item — opus-4-6

My fault. `--pending-add` auto-generates a 4-char hash ID (e.g., `#kne3`), and I included `[#cx01]` as literal text in the item body — so the binary treated it as content, not as an ID override. Result: `[#kne3] [#cx01]` — two bracketed IDs per item.

Fixed: edited all 5 items via `--pending-edit` to strip the redundant `[#cxNN]` tags. The canonical IDs are now `#kne3`–`#stk1` (the auto-generated ones). 
Can we fix the skill, runbook, binary logic? Should the agent-doc command have an id option to allow the skill to set the id?
### Re: Custom ID for pending-add — opus-4-6

Yes, this is worth fixing. The root cause is `op_add()` in `pending.rs:484` — it always auto-generates a hash ID via `assign_unique_hash(text, doc_id, &taken)`. There's no code path for a caller-specified ID.

#### Recommended fix: `id=` prefix syntax on `--pending-add`

Mirror the existing `--pending-edit "id=text"` pattern:

```
--pending-add "Phase 1: ..."           # auto-generate ID (current behavior)
--pending-add "cx01=Phase 1: ..."      # use "cx01" as the ID
```

**Changes needed:**

1. **`pending.rs`**: Add `op_add_with_id(body, id, text, doc_id, gated)` — same as `op_add` but skips `assign_unique_hash`, validates the caller-provided ID isn't already taken.

2. **`main.rs`**: Parse `--pending-add` values — if text contains `=` and the prefix matches `[a-z0-9]{2,8}`, split into `(id, text)` and call `op_add_with_id`. Otherwise fall through to `op_add` (auto-generate). Same for `--pending-add-gated`.

3. **Runbook `pending-ops.md`**: Document the `id=text` syntax.

4. **SKILL.md**: No change needed — the skill just passes the right format.

#### Why not extract `[#id]` from the text?

Considered it — but `[#id]` is a rendering convention (how items look in markdown), not an input format. Mixing input and output formats creates fragile parsing and could conflict with legitimate text containing `[#...]`. The `id=` prefix is unambiguous and consistent with `--pending-edit`.

~15 lines of Rust + 3-4 tests. Want me to implement it?
❯ Add to pending
### Re: Add to pending — opus-4-6

Done — added the custom ID feature as a pending item.
do #kne3. add tests. create a feature branch. increment version. build + install for local testing
### Re: do #kne3 — opus-4-6

Implemented Phase 1: Codex agent backend.

**What shipped (v0.33.12):**

- **`src/agent/codex.rs`** — new file, `Agent` + `StreamingAgent` impl for Codex CLI
  - Parses Codex JSONL event stream: `thread.started` → session ID, `item.completed` (type=`agent_message`) → response text, `turn.completed` → final signal
  - Session resume: `codex exec resume <id> --json`
  - Session fork: `codex exec resume --last --json` (Codex has no non-interactive fork ... resume --last is the closest equivalent)
  - Model override: `-m <model>`
  - Default sandbox: `-s workspace-write`
  - Removes `CODEX_CLI` + `CODEX` env vars from child process
  - Streaming iterator propagates `thread_id` as `session_id` on the final chunk
- **`src/agent/mod.rs`** — registered `"codex"` in `resolve()` dispatch
- **11 unit tests**: 9 event parser tests + 1 stream iterator propagation test + 1 missing-text edge case

**Branch:** `20260422/codex-agent-backend` (inside `src/agent-doc/`)
**Version:** 0.33.12 (Cargo.toml + pyproject.toml + VERSIONS.md)
**Build:** `cargo install --path .` + `cargo build --release` ... both binaries live

Pausing for your local testing before proceeding further.
do #t46w. add tests. add to spec. build + install for local testing
### Re: do #t46w — opus-4-6

Implemented Phase 2: Frontmatter generalization.

**Changes:**

- **`frontmatter.rs`** — added `agent_args: Option<String>` field to `Frontmatter` struct. Parsed in `merge_fields()`. Serde handles serialization/deserialization. 4 new tests: parse, parse both fields, write roundtrip, merge.
- **`config.rs`** — added `agent_args: Option<String>` to `Config` struct. 3 new tests: deserialization, precedence, fallback.
- **`start.rs`** — updated resolution chain: `fm.agent_args > fm.claude_args > config.agent_args > config.claude_args > AGENT_DOC_CLAUDE_ARGS env`. Backward-compatible: existing `claude_args` in frontmatter or config still works.
- **`specs/codex-support.md`** — Phase 2 section marked complete.

**Precedence chain:** `agent_args` always takes precedence over `claude_args` at each level (frontmatter, config). When only `claude_args` is set, it works exactly as before.

Build: both binaries rebuilt and installed.

Pausing for your local testing.
<!-- agent:boundary:e3e56ab1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#t46w] Phase 2: Frontmatter generalization — add agent_args field (claude_args compat alias), verify agent: codex + default_agent config end-to-end
- [ ] [#3jh0] Phase 3: Supervisor harness abstraction — extract HarnessConfig from start.rs (binary, restart flags, prompt patterns, trigger command, env removal)
- [ ] [#g37s] Phase 4: Route + Sync generalization — parameterize command injection in route.rs, rename wait_for_claude_ready → wait_for_agent_ready with configurable prompt patterns
- [ ] [#stk1] Phase 5: Skill file adaptation — bundle AGENTS_CODEX.md for Codex harness, extend skill install_all()
- [ ] [#9b2k] pending-add custom ID: add id= prefix syntax to --pending-add (pending.rs op_add_with_id, main.rs parse, pending-ops.md docs)
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
<!-- /agent:pending -->
## Assistant
