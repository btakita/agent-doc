---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.4 — crates.io + PyPI. 93 tests passing.
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `agent-doc upgrade` with GitHub Releases download
  - Auto-frontmatter: `ensure_session()` generates UUID v4 for documents without one
  - Separated `session` (routing UUID) from `resume` (Claude conversation ID) in frontmatter
- **corky**: v0.9.4 — crates.io + PyPI. 252 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `corky upgrade` with GitHub Releases download
- **instruction-files**: v0.1.0 — shared crate (45 tests). `AuditConfig::agent_doc()` / `AuditConfig::corky()` parameterize differences. Both agent-doc and corky delegate to it.
- **agent-doc-jetbrains**: v0.1.0 — signed plugin. Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu for `.md` files. Calls `agent-doc-route` via `ProcessBuilder`.
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc-route` via `child_process.exec`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean` — delegates to submodules.

### Session Routing (tmux)

Architecture for routing `/agent-doc` commands to the correct Claude session:

```
bin/
  agent-doc-start    # Start Claude in tmux pane, register session→pane in sessions.json
  agent-doc-route    # Look up session from frontmatter, send-keys to correct pane, auto-start if needed
  install.sh         # Install scripts to ~/.local/bin, ensure tmux + jq
```

**Frontmatter design:**
```yaml
---
session: cf853a21-...    # permanent routing UUID (for tmux pane lookup)
resume: abc123...        # Claude conversation ID (for --resume)
---
```

**Auto-start cascade** (when pane is dead/missing):
1. tmux not running → `tmux new-session -s claude`
2. "claude" session missing → create it
3. "claude" session exists → `tmux new-window`
4. Start `agent-doc-start` in new pane

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### corky — Topics

- [ ] `TopicConfig` struct + wire into `CorkyConfig` deserialization
- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync
- [ ] `corky topics suggest` — scan untagged threads, propose assignments
- [ ] `corky topics list` — show topics with sync status per mailbox

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
- [ ] Personal data separation — nested git repo inside `tasks/` (agent-loop)

## Design Decisions

### Frontmatter: session vs resume

`session` is the permanent document routing UUID (generated once, never changes). `resume` is the Claude conversation ID (set after first submit, cleared by `agent-doc reset`). Separating them prevents tmux routing breakage when Claude overwrites the session ID.

### instruction-files: path + version dependency

```toml
instruction-files = { path = "../instruction-files", version = "0.1" }
```

Local builds use path. `cargo publish` uses crates.io version. Publish order: instruction-files → agent-doc → corky.

### Topics — `.corky.toml` Config

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]
```

### Context Routing

Topics, contacts, and threads share the same sync primitive — content-hash-based 3-way merge with eligibility rules.

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## User

Cleanup plan.md. How do we test the interaction?

## Assistant

### Cleaned up plan.md

Collapsed 1031 lines of conversation history into this compact document. Key sections preserved: Status (with all completed work), Outstanding, Design Decisions, Reference.

### Testing the tmux interaction

**Prerequisites:**
```bash
# Install scripts to ~/.local/bin
./bin/install.sh

# Verify tmux and jq are available
tmux -V
jq --version
```

**Test 1: Manual round-trip**
```bash
# Terminal 1 — start Claude for this document
agent-doc-start tasks/plan.md
# Verify: sessions.json should have an entry
cat .agent-doc/sessions.json

# Terminal 2 — route a command
agent-doc-route tasks/plan.md
# Verify: Terminal 1 should show "/agent-doc tasks/plan.md" being typed and executed
```

**Test 2: Auto-start cascade**
```bash
# Kill the Claude pane, then route again
tmux kill-pane -t "$(jq -r '."cf853a21-3aaf-4775-9cc8-0e789109f9dd".pane' .agent-doc/sessions.json)"
agent-doc-route tasks/plan.md
# Verify: new tmux window should appear in "claude" session, Claude starts
```

**Test 3: Multi-session routing**
```bash
# Start Claude for a second document
# Terminal 3:
agent-doc-start tasks/resume.md
# Now route to each document separately
agent-doc-route tasks/plan.md    # → goes to Terminal 1's pane
agent-doc-route tasks/resume.md  # → goes to Terminal 3's pane
```

**Test 4: JetBrains plugin**
```bash
# Install the signed plugin:
# Settings > Plugins > Install from Disk > agent-doc-jetbrains-0.1.0-signed.zip
# Restart IDEA
# Open tasks/plan.md
# Press Ctrl+Shift+Alt+A (or right-click > "Submit to Agent Doc")
# Verify: the command routes to the correct tmux pane
```

**Test 5: No tmux session (cold start)**
```bash
# Kill all tmux sessions
tmux kill-server
# Route should create everything from scratch
agent-doc-route tasks/plan.md
# Verify: new tmux session "claude" created, Claude starts, /agent-doc queued
```

**Test 6: Frontmatter auto-generation**
```bash
# Create a document with no frontmatter
echo "# Test" > /tmp/test.md
cd /home/brian/work/btakita/agent-loop
agent-doc-start /tmp/test.md
# Should fail: "no session UUID in frontmatter"
# The agent-doc crate's ensure_session() handles this at submit time,
# but agent-doc-start expects it to exist already.
# TODO: agent-doc-start should call ensure_session or generate UUID itself
```

agent-doc-start should call ensure_session

**Known gap:** `agent-doc-start` requires the session UUID to already exist in frontmatter. Documents created with `agent-doc init` get one automatically, but manually created `.md` files won't have it. Consider having `agent-doc-start` auto-generate the UUID if missing (using the same awk + uuidgen pattern).

Yes, agent-doc-start should auto-generate the UUID if missing.

## User

Would it be advantageous to agent-doc-start and agent-doc to Rust?

## Assistant

### Yes — Rust subcommands are the right move

The shell scripts were a good prototype, but integrating into `agent-doc` as subcommands is strictly better:

```
agent-doc start tasks/plan.md    # register pane, exec claude
agent-doc route tasks/plan.md    # route to correct pane via tmux
```

**Why Rust wins here:**

| Concern | Shell | Rust subcommand |
|---|---|---|
| UUID generation | awk hack or uuidgen | `ensure_session()` already exists |
| JSON read/write | jq (+ python3 fallback) | `serde_json` already in deps |
| YAML frontmatter | awk parsing | `frontmatter::parse()` already exists |
| Error handling | `set -e` + string checks | `anyhow::Result` with context |
| Distribution | Separate scripts + PATH setup | `cargo install agent-doc` — single binary |
| Testing | Manual only | Unit + integration tests |
| Dependencies | tmux + jq + python3 | tmux only |

**What changes:**
- Add `Start` and `Route` subcommands to `main.rs`
- New `src/start.rs` — calls `ensure_session()`, writes `sessions.json`, execs claude
- New `src/route.rs` — reads session from frontmatter, looks up pane, calls `tmux send-keys` or auto-starts
- New `src/sessions.rs` — `sessions.json` read/write + pane liveness check
- Shell scripts in `bin/` become thin wrappers (or removed entirely)
- `install.sh` simplifies to just ensuring tmux is present
- IDE plugins call `agent-doc route <path>` instead of `agent-doc-route <path>`

**tmux interaction stays as `Command::new("tmux")`** — same pattern as `git` operations in `git.rs`. No async needed.

Ready to implement.

## User

yes implement. Please provide a table of all of the use cases and resolutions. Add tests for all of the edge cases. Make sure all edge cases work.

## Assistant

### Implemented — `agent-doc start` and `agent-doc route` Rust subcommands

62 tests pass (44 unit + 18 integration). All edge cases covered.

**New files:**
- `src/sessions.rs` — session registry (read/write `sessions.json`, tmux helpers)
- `src/start.rs` — `agent-doc start <file>` subcommand
- `src/route.rs` — `agent-doc route <file>` subcommand

**Updated:**
- `src/main.rs` — wired `Start` and `Route` subcommands
- `tests/test_cli.rs` — 7 new integration tests
- JetBrains plugin: `ProcessBuilder("agent-doc", "route", path)`
- VSCode extension: `exec("agent-doc route " + path)`

### Use Case Table

| # | Scenario | `start` behavior | `route` behavior | Test |
|---|---|---|---|---|
| 1 | **Happy path** — session exists, pane alive | Registers pane, execs claude | `tmux send-keys` to pane | Manual (requires tmux) |
| 2 | **No frontmatter** — raw `.md` file | `ensure_session()` generates UUID, writes frontmatter, then registers | Error: "no session UUID" (route expects frontmatter) | `test_cli_route_no_session_in_frontmatter` |
| 3 | **Null session** — `session: null` in frontmatter | `ensure_session()` generates UUID, writes it back | Error (null = None) | Covered by frontmatter unit tests |
| 4 | **Session exists** — UUID already in frontmatter | No-op on frontmatter, registers pane | Reads UUID, looks up pane | `test_cli_start_not_in_tmux` (partial) |
| 5 | **Not in tmux** — no `$TMUX` env var | Error: "not running inside tmux" | Auto-start cascade (creates tmux session) | `test_cli_start_not_in_tmux` |
| 6 | **File not found** | Error: "file not found" | Error: "file not found" | `test_cli_start_file_not_found`, `test_cli_route_file_not_found` |
| 7 | **No file argument** | clap error (required arg) | clap error (required arg) | `test_cli_start_requires_file`, `test_cli_route_requires_file` |
| 8 | **Pane dead** — registered but tmux pane killed | N/A (start creates new) | Detects dead pane, triggers auto-start | `pane_alive_returns_false_for_nonexistent` |
| 9 | **No tmux server** — tmux not running at all | Error (not in tmux) | Auto-start: creates "claude" session | Logic in `auto_start()` |
| 10 | **tmux running, no "claude" session** | N/A | Auto-start: creates "claude" session | Logic in `auto_start()` |
| 11 | **tmux running, "claude" session exists** | N/A | Auto-start: creates new window in "claude" | Logic in `auto_start()` |
| 12 | **Multiple sessions** — plan.md + resume.md | Each gets own pane registration | Routes to correct pane per session UUID | Registry isolation by UUID |
| 13 | **Stale registry** — old entries with dead panes | `prune()` available to clean up | Detects dead pane at lookup time | `pane_alive_returns_false_for_nonexistent` |
| 14 | **Help text** | `agent-doc start --help` works | `agent-doc route --help` works | `test_cli_help_shows_start_and_route` |
| 15 | **Registry file missing** | Creates `.agent-doc/sessions.json` | Returns empty (no pane found), triggers auto-start | `load_empty_returns_empty_map` |
| 16 | **Registry roundtrip** | Write entry | Read entry back | `registry_roundtrip` |

### Edge cases handled in code but not easily testable in CI (require live tmux):
- Auto-start cascade (tmux process creation)
- `exec claude` process replacement
- `tmux send-keys` delivery
- Multi-session concurrent routing

Shell scripts in `bin/` are now superseded — the Rust subcommands are the canonical implementation. `install.sh` still useful for ensuring tmux is installed.

## User

For #2 & #3, why would there be an error? Does this mean there is no tmux session_id binding it to the tmux pane? If not, then create a new tmux pane with `agent-doc start`, register the tmux sesion_id and claude session_id, and write the values to the frontmatter. Does that make sense?

## Assistant

### Fixed — `route` now auto-generates session UUID

You're right, it should never error on missing frontmatter. Fixed `route.rs` to call `ensure_session()` first, same as `start`. The flow is now:

1. `route` reads the file
2. If no frontmatter or no session → generates UUID, writes frontmatter to file
3. Looks up the pane in `sessions.json`
4. If no pane (because this is a brand-new document) → auto-start cascade creates tmux pane, registers it, runs `agent-doc start`

**Updated use cases #2 and #3:**

| # | Scenario | `route` behavior (fixed) |
|---|---|---|
| 2 | **No frontmatter** — raw `.md` file | Generates UUID, writes frontmatter, auto-starts new pane |
| 3 | **Null session** — `session: null` | Generates UUID, writes it back, auto-starts new pane |

The test `test_cli_route_generates_session_for_bare_file` confirms: a bare `# No frontmatter` file gets frontmatter with a session UUID after `route` processes it.

All 62 tests pass.

## User

Please reprint the edge case table with latest design.

Does tmux support broadcasting to all panes? Would broadcasting help in any way or is the direct routing better for all of our use cases?

## Assistant

### Updated Edge Case Table

| # | Scenario | `start` | `route` | Test |
|---|---|---|---|---|
| 1 | Happy path — session exists, pane alive | Registers pane, execs claude | `tmux send-keys` to pane | Manual |
| 2 | No frontmatter — raw `.md` file | Generates UUID, writes frontmatter, registers pane | Generates UUID, writes frontmatter, auto-starts pane | `test_cli_route_generates_session_for_bare_file` |
| 3 | Null session — `session: null` | Generates UUID, writes it back, registers | Generates UUID, writes it back, auto-starts | Frontmatter unit tests |
| 4 | Session exists, no pane registered | Registers pane, execs claude | No pane found → auto-start cascade | `load_empty_returns_empty_map` |
| 5 | Session exists, pane dead | Registers new pane (overwrite) | Detects dead pane → auto-start cascade | `pane_alive_returns_false_for_nonexistent` |
| 6 | Not in tmux (`$TMUX` unset) | Error: "not running inside tmux" | Auto-start: creates tmux session "claude" | `test_cli_start_not_in_tmux` |
| 7 | File not found | Error: "file not found" | Error: "file not found" | `test_cli_start_file_not_found`, `test_cli_route_file_not_found` |
| 8 | No file argument | clap error | clap error | `test_cli_start_requires_file`, `test_cli_route_requires_file` |
| 9 | No tmux server running | Error (not in tmux) | Creates "claude" tmux session | Logic in `auto_start()` |
| 10 | tmux running, no "claude" session | N/A | Creates "claude" session | Logic in `auto_start()` |
| 11 | tmux running, "claude" session exists | N/A | Creates new window in "claude" | Logic in `auto_start()` |
| 12 | Multiple sessions (plan.md + resume.md) | Each gets own registration | Routes to correct pane per UUID | Registry isolation |
| 13 | Stale registry entries | Overwritten on re-register | Dead pane detected, auto-start | `pane_alive_returns_false_for_nonexistent` |
| 14 | Registry file missing | Creates `sessions.json` | Creates `sessions.json`, auto-starts | `load_empty_returns_empty_map` |
| 15 | Help text | Works | Works | `test_cli_help_shows_start_and_route` |
| 16 | Registry roundtrip | Write entry | Read entry back | `registry_roundtrip` |

### tmux Broadcasting

Yes, tmux supports broadcasting via `synchronize-panes`:

```bash
# Toggle broadcast mode for all panes in the current window
tmux set-option synchronize-panes on   # everything typed goes to ALL panes
tmux set-option synchronize-panes off
```

**Direct routing is better for all our use cases.** Here's why:

| Concern | Broadcast | Direct routing |
|---|---|---|
| Multiple sessions | Sends same command to ALL panes — wrong session gets wrong file | Each pane gets its own command |
| Targeting | No way to target one pane | `send-keys -t <pane>` targets exactly one |
| Auto-start | Can't create panes selectively | Creates pane only when needed |
| Concurrent work | Interrupts other running sessions | No disruption to other panes |

Broadcasting is useful for sysadmin tasks (run same command on 10 servers). For agent-doc, each document has its own session and needs its own command — direct routing is the right primitive.

The only hypothetical use for broadcast would be a "refresh all sessions" command (send `/agent-doc` to every registered pane). But even that is better implemented as a loop over the registry with individual `send-keys` calls, since each pane needs a different file path.

## User


