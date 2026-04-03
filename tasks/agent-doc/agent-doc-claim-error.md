---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.4 released. IPC reposition socket-only, max_lines= inline attr, boundary hash fix.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260403-194619.md`*

Go
- [ ] Inline `max_lines=` attribute on component tags
### Re: Inline `max_lines=` attribute

Implemented `max_lines=N` as an inline attribute on component tags. Usage: `&lt;!-- agent:exchange patch=append max_lines=50 --&gt;` trims content to the last 50 lines after patching.

**Changes:**
- `template.rs`: Added post-patch processing — checks each component for `max_lines` (inline attr > `components.toml` > unlimited). Uses `limit_lines()` to keep last N lines.
- `patch.rs`: Added `max_lines` field to `ComponentConfig` for `components.toml` support.
- Both inline attrs and config file work; inline takes precedence.

`make check` passes (clippy + all tests).
continue
### Re: Watch daemon restart

No watch daemon running — only `agent-doc start` session managers. Release binary rebuilt in-place (`cargo build --release`), so the new code (simplified IPC reposition + max_lines) takes effect on all new command invocations. Existing `start` processes shell out to the binary for each operation, so they pick up changes automatically.

All three items complete.

build and install binary to test watch daemon
### Re: Build and install

Done. `cargo install --path .` installed to `~/.cargo/bin/agent-doc`. Both the `.bin/` symlink (release target) and `~/.cargo/bin/` (tmux-spawned sessions) now have the updated binary with IPC reposition simplification and max_lines support.

audit docs
add tests
Update SPEC.md, README.md, ontology, module-harness
### Re: Audit, tests, docs

**Tests added** (4 new tests in `template.rs`):
- `max_lines_inline_attr_trims_content` — verifies trimming to last N lines
- `max_lines_noop_when_under_limit` — content under limit unchanged
- `max_lines_from_components_toml` — config file path works
- `max_lines_inline_beats_toml` — inline attr takes precedence over config

**Docs updated:**
- `SPEC.md:45` — added `max_lines=N` to inline attributes description
- `SPEC.md:455` — added `max_lines` to `components.toml` example
- `README.md:54` — updated feature list to mention `max_lines=`
- `README.md:56` — removed "file-based fallback" from IPC description (socket-only now)
- `CLAUDE.md:38` — updated inline attributes section with `max_lines=` example

**Audit-docs:** Down from 5 issues to 2 (pre-existing: large table warning + SKILL.md age).

`make check` passes (clippy + all 612 tests including 4 new max_lines tests).

release
### Re: Release v0.31.4 (HEAD)

Released agent-doc v0.31.4:

1. Committed all changes (14 files, +249/-80)
2. Tagged `v0.31.4`, pushed to origin
3. Published to crates.io
4. PyPI skipped (manylinux platform tag issue — needs container build)
5. GitHub Release created with prebuilt binary: https://github.com/btakita/agent-doc/releases/tag/v0.31.4
6. `cargo install --path .` — binary installed to `~/.cargo/bin/`

`agent-doc --version` → `agent-doc 0.31.4`
<!-- agent:boundary:37129e13 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] IPC reposition → FFI event
- [x] Inline `max_lines=` attribute on component tags
- [x] Build and install binary
- [x] Audit docs, tests, doc updates
- [x] Release v0.31.4 (crates.io + GitHub Release)
<!-- /agent:pending -->
