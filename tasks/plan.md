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
