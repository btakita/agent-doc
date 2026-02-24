---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.4 — crates.io + PyPI. 90 tests passing.
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `agent-doc upgrade` with GitHub Releases download
- **corky**: v0.9.4 — crates.io + PyPI. 216 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `corky upgrade` with GitHub Releases download

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### corky — Telegram

- [ ] Phase 2: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Phase 3: Watch mode (real-time push)
- [ ] Phase 4: Send via `draft push`

### corky — Slack

- [ ] Phase 2: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`
- [ ] Phase 3: Socket Mode (real-time push)

### Future

- [ ] Streaming responses (agent-doc)
- [ ] Document-only permission flow — no terminal (agent-doc)
- [ ] Desktop notifications on completion (agent-doc)
- [ ] Multi-agent documents — sections routed to different agents (agent-doc)
- [ ] Email transport for async human-agent loops — bridge with corky (agent-doc)

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## User

agent-doc & corky changes run in separate threads.

create PR branch for agent-doc & corky.
Provide auto-upgrade if possible. Otherwise warn user that an upgrade is available.

Write tests for new feature.

commit both
push both

squash merge PRs

increment patch version
push tag
publish

## Assistant

Done. Both projects got upgrade check + auto-upgrade in parallel:

**Implementation** (both `src/upgrade.rs`):
- Startup: `warn_if_outdated()` queries crates.io API (`/api/v1/crates/{name}`) via `ureq`, 5s timeout, 24h file cache (`~/.cache/{name}/version-cache.json`). Prints yellow stderr warning if outdated. All errors silently swallowed.
- `upgrade` subcommand: fresh version check, then tries `cargo install` -> `pip install --upgrade` -> manual instructions.
- Version comparison: simple `X.Y.Z` tuple ordering.

**Tests**: agent-doc 88 tests (10 new), corky 244 tests (7 new). Clippy clean.

**Released**:
- agent-doc v0.1.3 — crates.io + PyPI
- corky v0.9.3 — crates.io + PyPI

PRs: agent-doc #2, corky #7 (both squash-merged to main, tagged, published).

## User

What is the developer experience to upgrade the binary curl | sh...they would have to run the command again? What is typical for this sort of install? Is there an upgrade command that is often run?

## Assistant

Right now `corky upgrade` only tries `cargo install` and `pip install --upgrade` — it doesn't cover users who installed via `curl | sh` (GitHub Releases binary). Those users would have to re-run the curl script manually.

**What's typical:**

- **Self-update** (rustup, Homebrew, claude-code): `tool update` detects install method, downloads new binary in-place. Best UX.
- **Idempotent re-run** (starship, nvm): `curl | sh` again overwrites the old binary. Simple but the user has to remember the URL.
- **No self-update** (bat, ripgrep, fd): rely on package managers or manual download.

The gap: `corky upgrade` should detect that the running binary lives at `~/.local/bin/corky` (or `/usr/local/bin/corky`) and download the new release from GitHub Releases directly — same logic as `install.sh`. Fallback chain becomes:

1. **GitHub Releases** — if binary is at a known install path, download + replace (like `install.sh`)
2. **`cargo install`** — if cargo is available
3. **`pip install --upgrade`** — if pip is available
4. **Manual instructions** — print the curl command

This would make `corky upgrade` the single upgrade command regardless of install method. Want me to implement this?

## User

Yes. This will be for corky & agent-doc.

## Assistant

Done. Both `corky upgrade` and `agent-doc upgrade` now download prebuilt binaries from GitHub Releases as the primary upgrade strategy.

**Upgrade fallback chain** (both projects):
1. **GitHub Releases** — `try_github_release_upgrade()` detects platform via `cfg!()`, downloads `{name}-{target}.tar.gz` from releases, extracts via `tar`, atomically replaces the running binary
2. **`cargo install`** — if available
3. **`pip install --upgrade`** — if available
4. **Manual instructions** — prints `curl | sh` URL, cargo, and pip commands

**New helpers**: `detect_target()` (compile-time platform detection), `try_github_release_upgrade()` (download + extract + replace). 30s download timeout. Cross-filesystem fallback via copy if rename fails.

**Released**:
- agent-doc v0.1.4 — crates.io + PyPI (PR #3, squash-merged)
- corky v0.9.4 — crates.io + PyPI (PR #8, squash-merged)

Tests: agent-doc 90 (2 new), corky 216 (2 new). Clippy clean.

## User


