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
<!-- agent:boundary:27c6c5b7 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [ ] [#kne3] [#cx01] Phase 1: Codex agent backend (agent/codex.rs) — implement Agent + StreamingAgent for codex exec --json, parse JSONL, session resume/fork. Spec: specs/codex-support.md
- [ ] [#t46w] [#cx02] Phase 2: Frontmatter generalization — add agent_args field (claude_args compat alias), verify agent: codex + default_agent config end-to-end
- [ ] [#3jh0] [#cx03] Phase 3: Supervisor harness abstraction — extract HarnessConfig from start.rs (binary, restart flags, prompt patterns, trigger command, env removal)
- [ ] [#g37s] [#cx04] Phase 4: Route + Sync generalization — parameterize command injection in route.rs, rename wait_for_claude_ready → wait_for_agent_ready with configurable prompt patterns
- [ ] [#stk1] [#cx05] Phase 5: Skill file adaptation — bundle AGENTS_CODEX.md for Codex harness, extend skill install_all()
<!-- /agent:pending -->
## Assistant
