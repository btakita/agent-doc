---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.5.1 — crates.io + PyPI + GitHub Releases. 93 tests passing (72 unit + 21 integration).
  - Session routing: `start`, `route`, `claim`, `prompt`, `commit` subcommands with auto-start cascade
  - `claim`: bind document to current tmux pane (last-call-wins, skill-invokable via `/agent-doc claim`)
  - `focus`: focus tmux pane for a session document (cross-window support)
  - `layout`: arrange tmux panes to mirror editor split layout
  - `skill install` / `skill check`: embedded SKILL.md management via `include_str!`
  - `commit`: git root resolution (superproject-aware, no CWD drift issues)
  - Multi-session polling: `prompt --all`, plugin multi-file tracking
  - Prompt answering: `prompt --answer <N>` navigates + confirms via arrow keys
  - Upgrade check + self-update via GitHub Releases
- **corky**: v0.12.0 — crates.io + PyPI + GitHub Releases. 357 tests passing.
  - Social media posting: LinkedIn OAuth2, draft creation/publishing
  - `_cmd` suffix fields for credential managers (pass, op, etc.)
  - Contact sync v2: conversation-aware eligibility, 3-way merge
  - Topics: config, list, add, info, suggest
  - Scheduling: `corky schedule run/list`
- **instruction-files**: v0.1.3 — crates.io. 47 tests passing.
  - Shared audit crate: staleness, tree paths, line budget, actionable content checks
  - SPEC.md discovery; line budget excludes non-agent reference docs
- **agent-doc-jetbrains**: v0.1.0 — signed plugin.
  - Hotkey (`Ctrl+Shift+Alt+A`) + right-click context menu
  - Multi-session PromptPoller with `prompt --all`, file-context in PromptPanel
  - PromptPanel: bottom-anchored bar with numbered hotkeys, IDE editor font scaling
- **agent-doc-vscode**: v0.1.0 — extension. Same hotkey, calls `agent-doc route`.
- **agent-loop**: Root Makefile with `make release`, `make update`, `make check`, `make precommit`, `make publish`, `make clean`.

### Session Routing (tmux)

```
agent-doc start <file>                 # register pane, exec claude
agent-doc route <file>                 # send-keys to pane, auto-start if needed
agent-doc claim <file>                 # bind file to current tmux pane
agent-doc focus <file>                 # focus tmux pane (cross-window)
agent-doc layout <files...> --split h|v  # arrange panes to mirror editor splits
agent-doc prompt <file>                # detect permission prompts → JSON
agent-doc prompt --all                 # poll all live sessions
agent-doc prompt --answer <N> <file>   # answer a prompt
agent-doc commit <file>                # git add + commit with timestamp
agent-doc skill install                # write bundled SKILL.md to project
agent-doc skill check                  # compare installed vs binary version
```

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### instruction-files

- [ ] Collapse `AuditConfig` presets to single polyglot `Default` (always audit both AGENTS.md + CLAUDE.md)
- [ ] `.instruction-files.toml` project-root config file (override extensions, root markers, skip dirs)

### corky — Topics

- [ ] Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
- [ ] Topic sync: eligibility check (label/contact match OR `shared_with`), delegate to shared merge
- [ ] Thread tagging: auto-add `**Topic**: {name}` to matching conversations during sync

### corky — Integrations

- [ ] Telegram: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Slack: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`

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

### instruction-files: polyglot default + .instruction-files.toml

Collapse to a single `AuditConfig::default()` that always discovers and audits both `AGENTS.md` and `CLAUDE.md`. Convention: `CLAUDE.md -> AGENTS.md` symlink so both names resolve.

Config at `.instruction-files.toml` in project root (optional). Resolution: config file → `AuditConfig::default()`. Consuming binaries read the TOML — zero new deps in the library crate.

### tmux routing: direct, not broadcast

Each document has its own session UUID → tmux pane. Direct `send-keys -t <pane>` routing. Broadcasting (`synchronize-panes`) would send the same command to all panes. Extract as separate crate when a second consumer exists.

### Prompt detection: tmux capture-pane polling

`agent-doc prompt` captures the visible terminal via `tmux capture-pane -t <pane> -p`, strips ANSI escape codes, and searches for Claude Code's numbered-option prompt pattern (e.g., "Do you want to proceed? 1. Yes 2. No"). Returns structured JSON. The JetBrains plugin polls this every 1.5s after submit and shows a bottom-anchored bar with hotkey support.

Fragile by nature (terminal scraping), but works for common cases. Future: Claude Code hooks API or `--json-events` mode.

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

- **Decisions & architecture:** `agent-doc/SPEC.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPEC.md`
- **Workflow orchestration:** `CLAUDE.md`

## Session Log

### Session 1 (2026-02-25)

1. Pending releases audited — agent-doc v0.3.0 released (crates.io + PyPI + GitHub Release)
2. JetBrains PromptPanel font — scaled with IDE editor font
3. Topics implemented — Phases 1-3 complete (config, CLI, suggest). 290 corky tests passing.
4. PromptPoller fixes — sync refresh, one-shot reload via `modificationStamp`
5. Multi-session polling — 4-phase implementation (SessionEntry file → `--all` CLI → plugin multi-poll → file context)
6. `agent-doc commit <file>` — new subcommand, replaces `$()` shell substitution
7. `agent-doc claim <file>` — bind document to current tmux pane (last-call-wins)
8. `agent-doc skill install` / `check` — embedded SKILL.md management
9. `agent-doc commit` — git root resolution (superproject-aware)
10. v0.4.0 released — [PR #6](https://github.com/btakita/agent-doc/pull/6) squash merged, published to crates.io + PyPI + GitHub Release
11. PromptPanel fix — `JDialog` → `JLayeredPane` overlay (no WM leak, persists on focus change). Plugin rebuilt and signed.
12. Document cleanup — condensed from 685 → 150 lines
13. SKILL.md: comment stripping in diff — `<!-- ... -->` and `[//]: # (...)` stripped before comparing snapshot vs current
14. SKILL.md: `agent-doc-version` frontmatter + auto-update check in step 0
15. v0.4.1 released — [PR #7](https://github.com/btakita/agent-doc/pull/7) squash merged, published to crates.io + PyPI + GitHub Release
16. Prompt cycling fix — sticky queue in `PromptPoller.handlePollResults` (keep current prompt until resolved, `totalActive` count)
17. SPECS.md audit — filled §4 comment stripping, §7.6 `--root`, §7.12 version frontmatter, §7.13 `warn_if_outdated`
18. Flaky test fix — `skill.rs` refactored to `install_at(root)` / `check_at(root)` with tempdir (no CWD race)
19. v0.4.2 released — [PR #8](https://github.com/btakita/agent-doc/pull/8) squash merged, published to crates.io + PyPI + GitHub Release
20. Document cleanup — condensed conversation thread into session log
21. Confirmed prompt cycling fix working. Pending items review.
22. Moved corky icon: `docs/corky_icon.svg` → `assets/corky_icon.svg`
23. Prompt reappearance fix — `answeredPromptKey` grace period in `PromptPoller.kt` (filter answered prompt from poll results until answer takes effect). Plugin rebuilt.
24. Document cleanup
25. SKILL.md `$()` prohibition — added explicit "NEVER use `$()`" warning to step 5, synced all 3 copies
26. Social credential `_cmd` fields — [corky PR #11](https://github.com/btakita/corky/pull/11): `client_id_cmd`/`client_secret_cmd` in `SocialClientConfig`, shared `resolve_secret()` in `util.rs`. 297 corky tests.
27. SPECS.md in audit-docs — [instruction-files PR #1](https://github.com/btakita/instruction-files/pull/1): root-level discovery, line budget excludes non-agent files. 47 instruction-files tests.
28. PromptPanel WrapLayout fix — `FlowLayout` → custom `WrapLayout` for multi-row button overflow when labels are long. Plugin rebuilt.
29. Document cleanup
30. `tmux-setup.sh` — `--dev` / `--claude` flags for selective window opening
31. Credential resolution error propagation — `resolve_credentials()` silent fallthrough fix in corky
32. `agent-doc commit` CWD fix — [agent-doc PR #9](https://github.com/btakita/agent-doc/pull/9): `resolve_to_git_root()` returns `(git_root, resolved_path)`, `commit()` uses `.current_dir(&git_root)`
33. v0.4.3 released — [PR #9](https://github.com/btakita/agent-doc/pull/9) squash merged, published to crates.io + PyPI + GitHub Release
34. v0.11.1 released — [corky PR #11](https://github.com/btakita/corky/pull/11) squash merged, published to crates.io + GitHub Release
35. v0.1.1 released — [instruction-files PR #1](https://github.com/btakita/instruction-files/pull/1) squash merged, published to crates.io
36. PromptPanel label truncation + WrapLayout `isShowing` fix — long labels truncated to 80 chars with tooltip, removed `isShowing` gate in `computeSize`. Plugin rebuilt.
37. Document cleanup
38. Watch Ctrl+C fix — `tokio::select!` + watch channel for immediate wakeup
39. Test coverage expansion — 22 new tests (357 total): watch helpers (8), schedule dispatch (3), LinkedIn API mocks (11)
40. v0.11.3 released — [corky PR #13](https://github.com/btakita/corky/pull/13) squash merged, published to crates.io + GitHub Release
41. v0.1.2 released — instruction-files Cargo.lock sync, published to crates.io

### Session 2 (2026-02-26)

42. Tmux session routing diagnosis — `agent-doc start` creates windows in `claude` tmux session, not user's active session. Fix: `tmux switch-client -t claude:1` or use `agent-doc claim`.
43. LinkedIn OAuth 401 fix — proper `urlencode()` for all form body params, better token error reporting. Still unresolved: `invalid_client` credential mismatch in LinkedIn Developer Portal.
44. CWD-as-data-dir — `data_dir()` checks CWD for `.corky.toml` before `mail/` subdir. Run corky from inside `mail/` now works.
45. v0.11.2 released — [corky PR #12](https://github.com/btakita/corky/pull/12): OAuth encoding, error reporting, CWD detection, schedule dry-run, LinkedIn API version. 335 tests.
46. Watch Ctrl+C fix — `tokio::select!` + `watch::channel` for immediate wakeup
47. Test coverage expansion — 22 new tests (357 total): watch helpers (8), schedule dispatch (3), LinkedIn API mocks (11). Added `mockito` dev-dep, `_at` variants in `linkedin.rs`.
48. v0.11.3 released — [corky PR #13](https://github.com/btakita/corky/pull/13): Ctrl+C fix, test expansion, tokio `sync` feature. Published crates.io + GitHub Release.
49. v0.1.2 released — instruction-files Cargo.lock sync, published to crates.io.
50. Submodule Cargo.lock syncs — agent-doc + corky synced instruction-files 0.1.2, pushed.
51. Document cleanup — condensed session 2 conversation into log entries.
52. Plugin merge-safe reload — `PromptPoller.kt`: auto-save tracked files every poll cycle, 3-way merge via `git merge-file` when disk + editor both changed. Plugin rebuilt.
53. SPECS.md → SPEC.md rename — all 4 repos + instruction-files source code + 15+ markdown references updated. 47 + 357 tests pass.
54. v0.1.3 released — [instruction-files PR #2](https://github.com/btakita/instruction-files/pull/2): SPEC.md rename. Published to crates.io.
55. v0.4.4 released — [agent-doc PR #10](https://github.com/btakita/agent-doc/pull/10): SPEC.md rename. Published to crates.io + PyPI + GitHub Release.
56. v0.11.4 released — [corky PR #14](https://github.com/btakita/corky/pull/14): SPEC.md rename. Published to crates.io + PyPI + GitHub Release.
57. PyPI catch-up — v0.4.4 + v0.11.4 published to PyPI (missed during release). Release Process section added to `corky/AGENTS.md` and `agent-doc/CLAUDE.md`.
58. `corky label clear` — new command for bulk IMAP label removal. `--account`, `--search`, `--dry-run` flags. Batch processing (500 UIDs).
59. v0.11.5 released — [corky PR #15](https://github.com/btakita/corky/pull/15): label clear. Published to crates.io + PyPI + GitHub Release.
60. Gmail label cleanup — cleared "correspondence" from 10,438 messages across honk.com, pivotallabs.com, pivotalsf.com, googlegroups.com (FROM + TO).

## User

commit agent-loop

## Assistant

Done. `b4c771c` — updated corky submodule pointer (v0.11.5) + task files. Pushed to `20260225/topic-sync`.

## User

commit .agent-doc/ files too

## Assistant

Done. `318c8e2` — committed `.agent-doc/` (sessions.json + snapshots) and pushed. Working tree is clean.

## User

plugin:  Pressing Esc does not escape the prompt

## Assistant

Fixed. The panel's `KeyListener` only fires when the panel has focus — once you click the editor, focus leaves and Esc stops working.

Fix: registered Esc at the frame's `rootPane` level via `InputMap(WHEN_IN_FOCUSED_WINDOW)` + `ActionMap`. Works regardless of which component has focus. Number hotkeys (1-9) stay on the panel `KeyListener` since they'd conflict with editor typing. Binding is cleaned up in `dismiss()` to prevent leaks.

Plugin rebuilt and signed.

## User

I have an existing copy on my local drive at ~/work/Presence-AI/presence. It would be faster to symlink if we can than to symlink. But if you can associate it with a git submodule pointing to git@github.com:btakita/presence.git that would be ideal.

## Assistant

Done. Added `presence` as a git submodule pointing to `git@github.com:btakita/presence.git`. Used `--reference ~/work/Presence-AI/presence` to share objects from your local copy (instant clone, no network fetch for objects). Checked out at `c6caea1` on `dev`.

## User

Let's make a builder for mailFilters.xml. The from filters are long, so making a formatted list would make it more readable. Also, the labels should be bi-directional, matching from, to, and cc.

## Assistant

Done. Created two files:

- **`mail/filters.toml`** — readable source with one address per line. Each filter has `match = ["from"]` (one-directional) or `match = ["from", "to"]` (bidirectional). Gmail's `to` property matches both To and Cc headers, so `["from", "to"]` covers all three directions.
- **`bin/build-mail-filters`** — Python script that generates `mailFilters.xml` from the TOML. Supports `label`, `forward_to`, `star`, `never_spam`, `always_important`.

Usage: `build-mail-filters [TOML_FILE] [OUTPUT_FILE]` (defaults to `mail/filters.toml` → `mail/mailFilters.xml`).

Tested — output matches the original XML structure. The "for-lucas" filter matches `from` only; "correspondence" matches both `from` and `to`.

## User

Make this part of corky...would it be corky gmail filters build?
Can we use the gmail api to sync with the existing filters? Where we download the existing filters (if any) and upload to replace the existing filters?

## Assistant

Yes — proposed CLI: `corky filter build/pull/push`.

```
corky filter build [--input FILE] [--output FILE]  # TOML → XML (local, no API)
corky filter pull  [--account]                      # Gmail API → display current filters
corky filter push  [--account] [--dry-run]          # TOML → Gmail API (replace all)
```

**Gmail API** has `users.settings.filters.{list,get,create,delete}` — no bulk replace, so push would: list existing → batch delete → batch create (up to 100 per batch request). Requires OAuth2 scope `gmail.settings.basic`.

**Corky already has** OAuth2 infrastructure (LinkedIn in `social/auth.rs`), token storage (`token_store.rs`), and `ureq` HTTP client — all reusable for Gmail.

**Phased approach:**
1. **Phase 1** (now): `corky filter build` — port Python script to Rust
2. **Phase 2**: Gmail OAuth2 + `corky filter pull` (read-only, low risk)
3. **Phase 3**: `corky filter push` with `--dry-run` (destructive, needs safeguards)

Ready to implement Phase 1?

## User

yes

## Assistant

Done. `corky filter build` implemented.

**Files:**
- `corky/src/filter/mod.rs` + `corky/src/filter/build.rs` — Rust implementation (TOML parsing via `serde` + `toml`, XML string builder, `chrono` for timestamps)
- `corky/src/cli.rs` — `FilterCommands::Build { input, output }`
- `corky/src/main.rs` — dispatch
- `corky/src/lib.rs` — `pub mod filter`

**Tests:** 3 unit tests (XML generation, default match field, XML escaping). `make check` passes — 360 tests, clippy clean.

**Usage:** `corky filter build` (from data dir) or `corky filter build --input FILE --output FILE`.

## User

Still having this error:

● Bash(cp tasks/plan.md .agent-doc/snapshots/2ca8a80b31ae5e069e73ca4cdd5fb5eca81f5492c015a247a640f00f50e46c5f.md)
⎿  Error: Exit code 1
cp: cannot stat 'tasks/plan.md': No such file or directory

     cp: cannot stat 'tasks/plan.md': No such file or directory

## Assistant

That was CWD drift — after I ran `cargo run` from `~/work/btakita/mail` to test `corky filter build`, the working directory stayed there. The relative `cp tasks/plan.md .agent-doc/snapshots/...` failed because it was relative to `mail/`, not `agent-loop/`.

**Fix applied:** All snapshot copy operations now use absolute paths (`/home/brian/work/btakita/agent-loop/tasks/plan.md`), not relative ones. This prevents the error regardless of where previous commands ran.

61. `corky filter build` — Phase 1 implemented. TOML → XML, 3 unit tests, 360 total tests.
62. Snapshot CWD fix — absolute paths for all snapshot operations.

## User

Implement phase 2.

## Assistant

Done. Phase 2 implemented — `corky filter auth` + `corky filter pull`.

**New files:**
- `corky/src/filter/gmail_auth.rs` — Gmail OAuth2 (authorization code flow, token exchange, token refresh). Reuses `TokenStore` for persistence, `parse_callback` from social auth for the callback server. Credentials from `[social.gmail]` in `.corky.toml` or `CORKY_GMAIL_CLIENT_ID`/`CORKY_GMAIL_CLIENT_SECRET` env vars.
- `corky/src/filter/pull.rs` — Calls `GET /users/me/settings/filters` via Gmail API, pretty-prints each filter's criteria (from, to, subject, query, size) and actions (add/remove labels, forward).

**Modified files:**
- `corky/src/config/corky_config.rs` — Added `gmail: Option<SocialClientConfig>` to `SocialConfig`
- `corky/src/filter/mod.rs` — Added `pub mod gmail_auth; pub mod pull;`
- `corky/src/cli.rs` — Added `Auth` and `Pull` variants to `FilterCommands`
- `corky/src/main.rs` — Dispatch for new commands

**Auth flow:**
1. `corky filter auth` — opens browser → Google consent → localhost callback → token stored in `~/.config/corky/tokens.json`
2. `corky filter pull` — auto-authenticates if no token, refreshes expired tokens, then fetches filters
3. Scope: `gmail.settings.basic` (minimal, read/write filter settings only)
4. Token key: `gmail:default` or `gmail:<name>` with `--account`

**Config:**
```toml
[social.gmail]
client_id = "..."
client_secret = "..."
# or use _cmd variants for credential managers
```

**Tests:** 10 new tests (7 gmail_auth + 4 pull). `make check` passes — 370 tests, clippy clean.

63. `corky filter auth` + `corky filter pull` — Phase 2 implemented. Gmail OAuth2 + read-only filter listing. 370 tests.

## User

corky filter pull

## Assistant

Needs Gmail OAuth2 credentials. Error: `Gmail client_id not found.`

**Setup steps:**

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a project (or use existing)
3. Enable the **Gmail API** (APIs & Services → Library → search "Gmail API" → Enable)
4. Create OAuth2 credentials:
   - APIs & Services → Credentials → Create Credentials → OAuth client ID
   - Application type: **Desktop app**
   - Name: "corky" (or whatever)
   - Note the **Client ID** and **Client Secret**
5. Add authorized redirect URI: `http://127.0.0.1:8484/callback`
6. Configure in `.corky.toml`:

```toml
[social.gmail]
client_id = "YOUR_CLIENT_ID.apps.googleusercontent.com"
client_secret = "YOUR_CLIENT_SECRET"
```

Or use credential manager:
```toml
[social.gmail]
client_id_cmd = "pass google/corky/client-id"
client_secret_cmd = "pass google/corky/client-secret"
```

Then run:
```
corky filter auth    # authenticate (opens browser)
corky filter pull    # list filters
```

## User

corky filter pull

## Assistant

Credentials loaded successfully (client_id resolved from `.corky.toml`). Browser opened for Google consent but the OAuth callback timed out after 120s — likely because the browser popup wasn't visible from Claude's terminal.

**Run `corky filter auth` from your own terminal** so you can see the Google consent screen:

```
corky filter auth
```

Once authenticated, `corky filter pull` will work without re-auth (token stored in `~/.config/corky/tokens.json`).

## User

Google OAuth error: "Access blocked: correspondence has not completed the Google verification process." Error 403: access_denied.

## Assistant

The app is in "Testing" mode. Fix in Google Cloud Console:

1. **APIs & Services → OAuth consent screen**
2. Under **Test users**, click **Add users**
3. Add `brian.takita@gmail.com`
4. Save

Then retry `corky filter auth`. The consent screen will show an "unverified app" warning — click **Continue** to proceed. Verification is only needed for public apps, not personal tools.

## User

Do I need to Add billing account?

## Assistant

No. The billing warning is for paid Google Cloud services (Compute Engine, Cloud Storage, etc.). The Gmail API is free for personal use — OAuth2 and the Settings API have no cost. Skip that warning.

The only blocker is adding your email as a **test user** under **OAuth consent screen → Test users**.

## User

I don't see **OAuth consent screen → Test users**. Am I on the correct page?

## Assistant

Google recently reorganized the console. Try these paths:

**New layout:** APIs & Services → **OAuth consent screen** → look for the **Audience** tab/section → **Test users** is there.

**Alternative:** In the left sidebar under "APIs & Services", look for **OAuth overview** or **Branding & Audience**. The test users section may be under "Audience" rather than directly visible.

**Direct URL:** `https://console.cloud.google.com/apis/credentials/consent` — scroll down past "App information" and "Scopes" to find "Test users".

If the app's publishing status is "Testing" (not "In production"), you should see an "Add users" button. If your publishing status is "In production", you won't need test users — but you'd need to go through Google's consent screen verification for restricted scopes like `gmail.settings.basic`.

If you still can't find it, take a screenshot and I'll guide you.

## User

Added brian.takita@gmail.com try again

## Assistant

Auth succeeded, pull works. 26 filters found.

```
corky filter auth   → Gmail token stored as 'gmail:default'
corky filter pull   → 26 filter(s) displayed
```

Filters include old mailing list rules (ruby-talk, rspec-devel, rails, selenium, etc.) plus the two active ones from `filters.toml`:
- **Filter 25** (for-lucas): `*@upwork.com OR to-dev@briantakita.me OR ...` → label + star + forward + never spam
- **Filter 26** (correspondence): `tomtaira@gmail.com OR *@martin-investments.com OR ...` → label + important

Note: label IDs are shown as `Label_123...` for custom labels (Gmail API returns IDs, not names). The `gmail.settings.basic` scope doesn't cover `labels.list` — resolving names would need `gmail.labels` or `gmail.readonly` scope. The raw IDs are still usable for Phase 3 (`push`).

64. Gmail OAuth2 working — `corky filter auth` + `corky filter pull` tested end-to-end. 26 filters retrieved.

## User

Can filters.toml be merged into .corky.toml?

## Assistant

Yes. It follows corky's existing pattern (topics, social, contacts are all in `.corky.toml`). The `[meta]` section becomes unnecessary — `owner.name` and the account email already exist.

Current `filters.toml`:
```toml
[meta]
name = "Brian Takita"
email = "brian.takita@gmail.com"

[[filters]]
label = "for-lucas"
match = ["from"]
addresses = ["*@upwork.com", ...]
```

Merged into `.corky.toml`:
```toml
[[filters]]
label = "for-lucas"
match = ["from"]
forward_to = "dev@briantakita.me"
star = true
never_spam = true
always_important = true
addresses = [
    "*@upwork.com",
    "to-dev@briantakita.me",
    ...
]

[[filters]]
label = "correspondence"
match = ["from", "to"]
always_important = true
addresses = [
    "tomtaira@gmail.com",
    ...
]
```

**Changes needed:**
1. Add `filters: Vec<Filter>` to `CorkyConfig` (reuse existing `Filter` struct from `filter/build.rs`)
2. Update `filter/build.rs` to read from `CorkyConfig` instead of a separate file
3. `build` command: `--input` flag still supported for standalone files, but default reads from `.corky.toml`
4. `meta.name` → `owner.name`, `meta.email` → first Gmail account's `user` field
5. Migrate existing `filters.toml` content into `.corky.toml`

Want me to implement this?

## User

Should filters be namespaced as `[[social.gmail.filters]]` instead?

## Assistant

I'd recommend `[[filters]]` at the top level. Reasoning:

1. **Filters are core email, not social.** The `social` section is for posting platforms (LinkedIn, Bluesky). Gmail filters are email infrastructure — closer to `accounts` and `routing` than to social publishing.
2. **Flat is better for user-edited config.** Less nesting = easier to read and edit.
3. **Extensible.** If corky ever supports ProtonMail Sieve filters or IMAP rules, `[[filters]]` extends naturally with an `account` field to scope by provider.

The Gmail OAuth2 credentials stay at `[social.gmail]` since that's where OAuth client config infrastructure lives. The filter definitions are separate from the API credentials.

```toml
# OAuth credentials (social infra)
[social.gmail]
client_id = "..."
client_secret = "..."

# Filter rules (email infra)
[[filters]]
label = "for-lucas"
match = ["from"]
addresses = ["*@upwork.com", ...]

[[filters]]
label = "correspondence"
match = ["from", "to"]
addresses = ["tomtaira@gmail.com", ...]
```

If you prefer `[[gmail.filters]]` as a middle ground (Gmail-specific but not under `social`), that works too. Your call.

## User

Why do we have `[social.gmail]` then? Why not just `[gmail]`?
`[gmail.filters]` would work then. We could also have a global `[filters]` as well but filters don't apply everywhere. I guess we could have corky act as the fallback for other email hosts that don't support filters.

But would it also apply to social media like Telegram, Slack, LinkedIn, etc?

Also, `[social.linkedin]` could be `[linkedin]`...

## Assistant

You're right on both counts. The `[social]` namespace is unnecessary — each platform should be top-level.

**Proposed restructure:**

```toml
# Gmail API + filters
[gmail]
client_id = "..."
client_secret = "..."

[[gmail.filters]]
label = "for-lucas"
match = ["from"]
addresses = ["*@upwork.com", ...]

# LinkedIn API
[linkedin]
client_id = "..."
client_secret = "..."

# Email accounts (IMAP/SMTP — unchanged)
[accounts.gmail]
provider = "gmail"
user = "brian.takita@gmail.com"
```

Each platform owns its namespace: `[gmail]`, `[linkedin]`, future `[telegram]`, `[slack]`. Filters live under their platform (`[[gmail.filters]]`). The `[social]` wrapper goes away.

**For social platforms (Telegram, Slack, LinkedIn):** These don't have server-side filter APIs. Corky's `[routing]` handles cross-platform message routing. A future `[[filters]]` at the top level could add corky-level auto-labeling rules across all sources, but that's separate from Gmail's server-side filters.

**Migration:**
1. `[social.gmail]` → `[gmail]`
2. `[social.linkedin]` → `[linkedin]`
3. `filters.toml` → `[[gmail.filters]]`
4. Remove `[social]` wrapper
5. Update all code references (`SocialConfig` → individual platform configs in `CorkyConfig`)

Want me to implement?

## User

yes...implement

## Assistant

Done. Config restructured: `[social]` removed, platforms are top-level.

**Changes:**

1. **`corky_config.rs`** — `SocialConfig` removed. New structs:
   - `GmailConfig` (client creds + `filters: Vec<GmailFilter>`) at `CorkyConfig.gmail`
   - `OAuthClientConfig` (renamed from `SocialClientConfig`) at `CorkyConfig.linkedin`
   - `GmailFilter` (label, match, addresses, forward_to, star, never_spam, always_important)

2. **`filter/gmail_auth.rs`** — `cfg.social.gmail` → `cfg.gmail`. Error messages updated.

3. **`social/auth.rs`** — `cfg.social.linkedin` → `cfg.linkedin`. Error messages updated.

4. **`filter/build.rs`** — Two modes:
   - Default: reads `[[gmail.filters]]` from `.corky.toml` + meta from `owner.name` / first Gmail account
   - `--input FILE`: reads standalone TOML (backward compat)

**New `.corky.toml` format:**
```toml
[gmail]
client_id = "..."
client_secret = "..."

[[gmail.filters]]
label = "for-lucas"
match = ["from"]
addresses = ["*@upwork.com", ...]

[linkedin]
client_id = "..."
```

**Tests:** 511 total (47 + 157 + 307), clippy clean. `filter pull` and `filter build --input` both verified working.

**Remaining:** Migrate `mail/filters.toml` content into your `.corky.toml` and update `[social.gmail]` → `[gmail]`, `[social.linkedin]` → `[linkedin]` in your config file.

65. Config restructured — `[social]` removed, `[gmail]` + `[linkedin]` top-level, `[[gmail.filters]]` for filter rules. 511 tests.

## User

Complete merging filters.toml into .corky.toml?

## Assistant

Done. `.corky.toml` now has everything — `filters.toml` content merged in, `[social.*]` flattened to top-level.

**What changed in `mail/.corky.toml`:**
- `[social.gmail]` → `[gmail]` (with `client_id_cmd`/`client_secret_cmd`)
- `[social.linkedin]` → `[linkedin]`
- Two `[[gmail.filters]]` entries added (for-lucas + correspondence) with all addresses from `filters.toml`

**Verified:**
```
$ corky filter build --output /tmp/test-from-config.xml
Generated /tmp/test-from-config.xml
  2 filter(s)
```

Output matches the original XML — both filters with correct addresses, labels, and actions.

`filters.toml` is now redundant. You can delete it when ready (`rm mail/filters.toml`), or keep it as a reference.

66. `.corky.toml` migration complete — `[social.*]` → top-level, `[[gmail.filters]]` merged from `filters.toml`. Verified with `corky filter build`.

## User

Implement phase 3

Make sure that any new filters apply to existing emails and any removed filters remove the label on the gmail side.

This should be a sync.

## Assistant

Implemented the basic `corky filter push` — but your request goes beyond simple push into **filter sync** territory. Here's what's done and what's needed:

**Done: `corky filter push [--dry-run]`**
- Fetches Gmail labels (name→ID resolution)
- Converts `[[gmail.filters]]` from `.corky.toml` → Gmail API format
- `--dry-run`: shows existing filters and what would be created
- Without `--dry-run`: deletes all existing filters, creates from config
- 12 new unit tests, 519 total, clippy clean

**What you're asking for (sync) requires more:**

1. **Retroactive label application** — when a new filter is added, search existing emails matching the criteria and apply the label. Requires `gmail.modify` scope (broader than current `gmail.settings.basic`).

2. **Label removal on filter delete** — when a filter is removed, search emails with that label and remove it. Also requires `gmail.modify`.

3. **Change detection** — compare local config vs existing Gmail filters to determine adds/removes/unchanged. This means matching by criteria (from/to addresses + label), not just bulk replace.

**Proposed `corky filter sync` approach:**
```
1. Fetch labels (name→ID)
2. Fetch existing Gmail filters
3. Diff: match existing filters to config entries by criteria
   - New: in config but not in Gmail → create filter + search & label existing emails
   - Removed: in Gmail but not in config (matched by label) → delete filter + remove label from emails
   - Unchanged: skip
4. Use messages.list + messages.batchModify for retroactive changes
```

**Scope expansion needed:**
```
gmail.settings.basic  (current — filter CRUD)
gmail.labels          (current — label name→ID)
gmail.modify          (NEW — add/remove labels on messages)
```

Users will need to re-run `corky filter auth` after this change for the new scope.

**Question:** For "removed filters" — should this only remove filters that corky previously pushed (tracked in a state file), or should it match any existing Gmail filter by label name? The former is safer (won't touch manual Gmail filters); the latter is simpler but could delete filters you set up manually in Gmail.

67. `corky filter push` implemented — basic push with `--dry-run`. 519 tests. Sync behavior pending design decision.

## User

corky filter push --dry-run

## Assistant

Got 403 Insufficient Permissions — expected. The existing token only has `gmail.settings.basic` scope, but `push` now needs `gmail.labels` too (for label name→ID resolution).

**Re-authenticate from your terminal:**
```
corky filter auth
```

This will open a fresh Google consent screen requesting both scopes. After that, `corky filter push --dry-run` will work.

## User

How do I setup the permissions?

plugin: The prompts are not showing up anymore.

## Assistant

**Permissions — no extra setup needed.** Just run from your terminal:

```
corky filter auth
```

This opens a new Google consent screen that requests the expanded scopes (`gmail.settings.basic` + `gmail.labels`). Since your app is in "Testing" mode and your email is already a test user, it will work — just click through the "unverified app" warning and "Continue". No Cloud Console changes needed.

The new scope (`gmail.labels`) is a non-sensitive scope, so Google doesn't require additional verification for it.

**Plugin prompts:** Investigating — checking `agent-doc prompt --all` and the PromptPoller for issues.

## User

corky filter auth

did you find any issues with the plugin?

How can we setup whisper integration for transcripts? I can run whisper on my machine with my 5090. But other people may want to use a service.
Use case: I had a phone call with a contact and I want to save the transcription.

## Assistant

**`corky filter auth`** — run this from **your terminal** (not from the editor). It opens a browser for Google consent. Once done, `corky filter push --dry-run` will work.

**Plugin prompts — CLI side is working.** `agent-doc prompt --all` returns 4 sessions, 1 active prompt (qwire-holdings). If the plugin isn't showing it, check:

1. **JetBrains project root** — the plugin runs `agent-doc prompt --all` from `project.basePath`. Make sure your JetBrains project is opened at `agent-loop/`, not a subdirectory.
2. **Plugin restart** — try closing/reopening the project, or Ctrl+Shift+A → "Restart IDE". The poller might have silently stopped.
3. **Plugin version** — confirm you have the latest build installed (merge-safe reload build from session 2).

**Whisper for phone call transcription** — this fits naturally into corky as a conversation tool. Proposed flow:

```
corky transcribe <audio-file> [--contact NAME] [--output FILE]
```

1. Transcribe audio → text (local whisper or cloud API)
2. Format as corky conversation markdown with speaker diarization
3. Optionally attach to a contact's thread or create a new conversation file
4. Save to `conversations/` or `mailboxes/<name>/conversations/`

**Config in `.corky.toml`:**
```toml
[transcription]
provider = "local"           # or "openai", "deepgram", "assemblyai"
model = "large-v3"           # whisper model size
device = "cuda"              # for local GPU inference
# api_key_cmd = "pass ..."   # for cloud providers
```

**Local backend options:**
- `faster-whisper` (Python, CUDA-optimized) — best for your 5090
- `whisper.cpp` (C++, very fast inference)
- Either run as subprocess or HTTP server (e.g., `whisper-server`)

**Speaker diarization** (who said what):
- `pyannote-audio` for local diarization
- Deepgram and AssemblyAI include built-in diarization

Want me to start implementing? I'd suggest Phase 1: local `faster-whisper` subprocess with basic transcription, no diarization yet.

## User
audit docs

yes...implement Phase 1 of whisper using local faster-whisper

## Assistant

**Audit docs:** All clean. agent-loop: no issues. corky: updated SPEC.md (filter push docs), README.md (command reference), CLAUDE.md (module list). Audit passes.

**Plugin fix:** Rebuilt and signed. The `invokeAndWait` → `invokeLater` fix prevents the poller deadlock. Reinstall from `agent-doc-jetbrains/build/distributions/` and restart the IDE.

**Whisper Phase 1:** Implementing now — `corky transcribe` with local `faster-whisper`.

68. Audit docs updated — SPEC.md, README.md, CLAUDE.md for filter push. Audit clean.
69. Plugin prompt fix — `invokeAndWait` → `invokeLater` in `autoSaveTrackedFiles()`. Rebuilt.

## User
Can the faster-whisper integration be in the corky rust code?

## Assistant

**Not directly** — `faster-whisper` is a Python library (backed by CTranslate2). But there are two solid Rust alternatives:

**Option A: `whisper-rs`** (whisper.cpp bindings)
- Most mature Rust whisper crate (v0.15.1, actively maintained)
- CUDA support via feature flag: `whisper-rs = { features = ["cuda"] }`
- Single binary, no Python dependency
- Audio in: f32 PCM, 16kHz mono (use `symphonia` crate for decoding + `rubato` for resampling)
- Models: ggml format files from HuggingFace (~2.9 GB for large-v3)

**Option B: `candle` whisper** (HuggingFace pure Rust ML)
- Zero C++ dependency, pure Rust
- CUDA + cuDNN support via `--features cuda`
- Uses safetensors models from HuggingFace Hub
- Less battle-tested for CLI usage

**The tradeoff:** faster-whisper is **~5-10x faster on GPU** than whisper.cpp (CTranslate2 has optimized batched beam search, cuDNN integration). For a 12-minute audio file on an RTX 4080: whisper.cpp takes ~3.4 min vs faster-whisper ~20 sec.

**My recommendation:** Start with `whisper-rs` + CUDA for the pure Rust path. It's simpler (single binary, no Python), and performance is still good for phone call lengths (a 30-min call would process in ~8-10 min on your 5090). If GPU speed becomes a bottleneck, we can add a `provider = "faster-whisper"` backend that shells out to Python.

**Proposed config:**
```toml
[transcription]
provider = "whisper-rs"     # or "faster-whisper" (subprocess)
model = "large-v3-turbo"    # best speed/quality ratio
device = "cuda"
model_path = "~/.cache/corky/models/"
```

Want to go with `whisper-rs` for Phase 1?

70. Whisper Rust research — `whisper-rs` (whisper.cpp bindings, CUDA) vs `candle` (pure Rust). faster-whisper is 5-10x faster on GPU but requires Python.

## User

whisper-rs phase 1

## Assistant

Done. `corky transcribe` implemented with `whisper-rs` + CUDA support.

**New files:**
- `corky/src/transcribe/mod.rs` — module root with feature-gated compilation
- `corky/src/transcribe/engine.rs` — whisper-rs inference engine (model load → transcribe → timestamped output)
- `corky/src/transcribe/audio.rs` — audio decoding pipeline: any format (WAV/MP3/FLAC/OGG/AAC) → f32 PCM 16kHz mono via `symphonia` + `rubato` resampling + `hound` WAV fallback
- `corky/src/transcribe/model.rs` — model cache + auto-download from HuggingFace

**Modified files:**
- `corky/Cargo.toml` — optional deps: `whisper-rs`, `symphonia`, `rubato`, `hound`. Feature flags: `transcribe` (CPU), `transcribe-cuda` (GPU)
- `corky/src/config/corky_config.rs` — `TranscriptionConfig` (model, model_path, language)
- `corky/src/lib.rs` — `pub mod transcribe`
- `corky/src/cli.rs` — `Transcribe` variant
- `corky/src/main.rs` — dispatch

**Feature flags:**
- `transcribe` — enables whisper-rs CPU-only transcription
- `transcribe-cuda` — enables whisper-rs with CUDA GPU acceleration
- Default build has no transcribe deps (zero impact on normal users)

**Usage:**
```
cargo build --release --features transcribe-cuda
corky transcribe recording.wav                    # auto-downloads large-v3-turbo, outputs to stdout
corky transcribe recording.mp3 --model base.en    # faster, English-only
corky transcribe call.flac -o transcript.txt      # save to file
```

**Config (optional):**
```toml
[transcription]
model = "large-v3-turbo"
model_path = "~/.cache/corky/models/"
language = "en"
```

**Tests:** 388 total (174 unit + 214 integration), clippy clean. Release binary built with CUDA support.

71. `corky transcribe` — Phase 1 whisper-rs implementation. Feature-gated (transcribe/transcribe-cuda), auto-download models, multi-format audio decoding. 388 tests.

## User

Where do you suggest I place the audio files for transcription?

## Assistant

**Recommended: `mail/recordings/` or per-contact.**

```
mail/
  recordings/                      # standalone recordings
    2026-02-26-call-with-alice.wav
    2026-02-26-call-with-alice.txt  # transcription output
  contacts/
    alice/
      recordings/                   # contact-specific recordings
        2026-02-26-call.wav
        2026-02-26-call.txt
  mailboxes/
    alex/
      recordings/                   # mailbox-specific recordings
```

**Why `recordings/`:**
- Follows corky's existing pattern (`conversations/`, `contacts/`, `drafts/`)
- Keeps audio files alongside transcription output
- Lives in `mail/` (private data repo), not in the corky source tree
- Per-contact placement links recordings to people naturally

**Future `--contact` flag:**
```
corky transcribe call.wav --contact alice
# → mail/contacts/alice/recordings/call.txt
```

Without `--contact`, output goes to `mail/recordings/` or stdout.

**For now**, just put audio files wherever and use `-o` to direct output:
```
corky transcribe ~/Downloads/call.wav -o mail/contacts/alice/recordings/2026-02-26-call.txt
```

The directory convention can be formalized when we add the `--contact` flag in a later phase.

## User

I don't want to commit the recordings. Only the transcripts. Do the transcripts understand who was in the conversation?

## Assistant

**Recordings gitignore:** Add `*.wav`, `*.mp3`, `*.flac`, `*.ogg`, `*.m4a` to `mail/.gitignore`. Transcripts (`.txt` or `.md`) get committed, audio files don't.

**Speaker identification:** Not currently. Phase 1 outputs plain timestamped text with no speaker labels. whisper-rs does expose `next_segment_speaker_turn()` (a boolean hint that the speaker changed), but doesn't identify *who* is speaking.

**Two approaches for speaker-aware transcripts:**

**Approach A: Manual speaker labels via `--speakers`**
```
corky transcribe call.wav --speakers "Brian,Alice"
```
Uses whisper's speaker turn detection to alternate labels. Simple but can misattribute on overlapping speech.

**Approach B: Speaker diarization (Phase 2)**
Full diarization with `pyannote-audio` or Rust equivalent. Segments audio by speaker, aligns with whisper timestamps. More accurate, requires separate model.

**Proposed transcript format (markdown):**

```markdown
---
date: 2026-02-26
type: phone-call
participants:
  - Brian Takita
  - Alice Chen
duration: "12:34"
model: large-v3-turbo
---

**Brian** [00:00:00 → 00:00:15]
Hey Alice, I wanted to follow up on the proposal we discussed last week.

**Alice** [00:00:16 → 00:00:42]
Sure, I've been thinking about it...
```

This integrates naturally with corky's conversation files. The `--contact` flag could auto-populate participants from contact info.

**For now:** I can add `--speakers` using whisper's built-in speaker turn hints. Want me to implement that, or save for Phase 2?

## User

implement --speakers
are *.amr files supported?

## Assistant

Done. Both implemented.

**`--speakers` flag:** Uses whisper's built-in tdrz speaker turn detection. When speakers are provided, output is markdown with YAML frontmatter and speaker-labeled blocks:

```
corky transcribe call.wav --speakers Brian,Alice
```

Output:
```markdown
---
date: 2026-02-26
type: phone-call
participants:
  - Brian
  - Alice
duration: "12:34"
---

**Brian** [00:00:00.000 → 00:00:15.230]
Hey Alice, I wanted to follow up on the proposal.

**Alice** [00:00:16.100 → 00:00:42.500]
Sure, I've been thinking about it...
```

Speaker names cycle through the list based on whisper's speaker turn hints. Without `--speakers`, output stays plain timestamped text.

**AMR support:** Added ffmpeg fallback. Symphonia handles WAV/MP3/FLAC/OGG/M4A/AAC natively. For formats it doesn't support (AMR, WMA, etc.), corky shells out to `ffmpeg` to convert to 16kHz mono f32 PCM. ffmpeg is detected automatically — clear error message if not installed.

**Tests:** 390 total, clippy clean. Release binary rebuilding with CUDA.

72. `--speakers` flag + AMR/ffmpeg fallback. tdrz speaker turn detection, markdown transcript output, ffmpeg for unsupported formats. 390 tests.

## User

Sync all topics into .corky.toml

## Assistant

Done. 3 topics synced from `mail/topics/` directories into `.corky.toml`:

1. **brian-takita** — brand, identity, linkedin, resume, agent-loop, corky, agent-doc
2. **constraint-shaped-ai-dev** — IOD, shaped-generation, typed-holes, graph-salience, RLM, constraint, topos, homer, ananke
3. **manufacturing-china** — smart-contract, escrow, supply-chain, manufacturing, PO, dispute, insurance, otonomi, kleros, vechain

Keywords extracted from each topic's README.md. Descriptions summarize the topic scope. `corky topics list --verbose` confirms all three are configured.

73. Topics synced to `.corky.toml` — 3 topics (brian-takita, constraint-shaped-ai-dev, manufacturing-china) with keywords and descriptions.

## User

Is the transcription feature released?

plugin: The ide prompt take the focus of the cursor. Pressing Esc propagates back to claude. Is there a way to either keep focus in the editor, with a hotkey to select the editor pane with the 2nd hotkey to select an option?

I already have Alt+1 bound to toggle the file selector pane. Can we have a key to select the prompt pane...like Alt+Esc? Can the prompt have a highlight color when selected?

I guess holding down the Alt could also highlight the pane and override other keyboard bindings. So Alt+1 would select option 1 with the override.

## Assistant

**Transcription release:** Not yet. The transcribe module is on the `20260225/topic-sync` branch — it'll go out with the next PR + version bump. The feature is behind opt-in Cargo feature flags (`transcribe`/`transcribe-cuda`), so default builds are unaffected. Your local `.bin/corky` binary already has it built with CUDA.

**Plugin focus fix:** Implemented. Plugin rebuilt and signed.

**How it works now:**
1. Prompt appears → **editor keeps focus** (no steal). Panel is muted (dimmer background, gray border).
2. **Alt+Esc** → focuses the prompt panel. Panel highlights (brighter background, blue border).
3. **1-9** → selects an option (when panel is focused).
4. **Esc** → dismisses the panel (from any focus state).
5. **Click** → always works (buttons don't take focus).

Alt+Esc when the prompt is in focus should bring the focus back to the editor.

**Focus highlight:** Blue top border + brighter warm background when focused, muted when unfocused. Visual cue is immediate.

**Re: Alt-hold override (Alt+1 to select):** Good idea. I can register Alt+1..9 at the frame level as direct option selectors — they override IDE's Alt+N bindings only while the prompt panel is visible (bindings are cleaned up on dismiss). This gives you the fast path without needing to focus the panel first. Want me to add that?

Do this as well.

Reinstall from `agent-doc-jetbrains/build/distributions/` and restart the IDE.

74. Plugin focus fix — Alt+Esc to focus prompt panel, 1-9 to select (panel focused), Esc to dismiss. Focus highlight (blue border). No focus steal on show.

## User

Does the plugin install really need to restart Idea?

Can we also have the plugin auto-focus after 2 seconds of no key being pressed or mouse click?

## Assistant

**Re: inline edits (Alt+Esc toggle + Alt+1..9):** Both implemented.

- **Alt+Esc toggle:** If panel has focus → returns focus to the editor. If editor has focus → focuses the panel. Uses `FocusEvent.oppositeComponent` to remember the previous focus owner.
- **Alt+1..9 direct selectors:** Registered at frame level — they override IDE's Alt+N bindings only while the prompt panel is visible. All bindings cleaned up on dismiss. Works from any focus state (no need to focus panel first).

**Plugin restart:** Not strictly necessary. IntelliJ can hot-swap plugins: **Settings > Plugins > gear icon > Install Plugin from Disk**, select the zip. Hot-swap usually works; full restart is only needed if the old version had stuck state.

**Auto-focus after 2 seconds:** Implemented. When the panel appears, a 2-second idle timer starts. Any key press or mouse click resets the timer. After 2 seconds of inactivity, the panel auto-focuses (blue highlight appears). Timer stops once panel gains focus. If focus leaves the panel, timer restarts.

**Updated keybinding model:**
1. Prompt appears → editor keeps focus, 2-second auto-focus timer starts
2. **Alt+Esc** → toggle: focus panel / return to editor
3. **1-9** → select option (panel focused)
4. **Alt+1..9** → select option directly (any focus state)
5. **Esc** → dismiss (any focus state)
6. **Click** → select (always works)

Reinstall from `agent-doc-jetbrains/build/distributions/`.

75. Plugin update — Alt+Esc toggle, Alt+1..9 direct selectors, 2-second auto-focus timer. All bindings cleaned up on dismiss.

## User

Implement the plugin so it does not need to restart the ide on install or replacement

## Assistant

Done. Plugin is now a **dynamic plugin** — no IDE restart required on install, update, or unload.

**Changes:**

1. **`plugin.xml`** — added `require-restart="false"` and a `<projectListeners>` entry for `PluginLifecycleListener`
2. **`PluginLifecycleListener.kt`** (new) — implements `ProjectManagerListener.projectClosed()` to dispose `PromptPoller` and dismiss `PromptPanel` when a project closes or the plugin is unloaded
3. **`PromptPoller.kt`** — added `disposeAll()` companion method for bulk cleanup

**What makes it dynamic-safe:**
- All mutable state (`PromptPoller` instances, `PromptPanel` UI, AWT listeners, timers, frame key bindings) is cleaned up on project close via the listener
- No `ApplicationComponent` or `ProjectComponent` (deprecated, blocks dynamic loading)
- No static initializers or classloader-pinning patterns
- `PromptPanel.dismiss()` already cleans up all global resources (AWT event listener, Swing timer, frame input/action maps)

**Install flow:** Settings > Plugins > gear icon > Install Plugin from Disk > select the zip. No restart prompt.

76. Dynamic plugin — `require-restart="false"`, `PluginLifecycleListener` for project close cleanup, `disposeAll()` for bulk teardown. No IDE restart needed.

## User

Plugin: I did not see the prompt pop open.

## Assistant

**No active prompts.** `agent-doc prompt --all` returns 6 sessions, all `active:false`. The plugin is working correctly — it only shows the panel when a Claude session has a pending permission prompt.

The plugin can't show a panel if no prompt exists. To test:
1. Start a Claude session on one of your documents
2. Trigger an action that requires permission (e.g., a bash command)
3. The prompt panel should appear at the bottom of the IDE

The dynamic plugin changes didn't break anything — the polling, panel, and keybindings are all intact. You just need an active prompt to see it.

77. Plugin working — no active prompts to display. Panel appears only when Claude sessions have pending permission questions.

## User

agent-doc gh action failure

2s
Run cargo clippy -- -D warnings
Updating crates.io index
error: failed to get `instruction-files` as a dependency of package `agent-doc v0.4.4 (/home/runner/work/agent-doc/agent-doc)`

Caused by:
failed to load source for dependency `instruction-files`

Caused by:
Unable to update /home/runner/work/agent-doc/instruction-files

Caused by:
failed to read `/home/runner/work/agent-doc/instruction-files/Cargo.toml`

Caused by:
No such file or directory (os error 2)
Error: Process completed with exit code 101.

## Assistant

**Root cause:** `instruction-files` is a path dependency (`path = "../instruction-files"`) in agent-doc's `Cargo.toml`. CI only checks out the `agent-doc` repo, so the sibling directory doesn't exist.

**Fixed all 4 workflows:**
- `ci.yml` — added `actions/checkout@v4` step to clone `btakita/instruction-files` into `../instruction-files`
- `release.yml` — same fix (cargo build needs it)
- `pypi.yml` — same fix in both `build` and `sdist` jobs (maturin compiles the Rust binary)
- `mdbook.yml` — no fix needed (builds docs only, no cargo compile of agent-doc)

The changes are in the `agent-doc` submodule. To push:
```
cd agent-doc
git add .github/workflows/
git commit -m "fix: clone instruction-files path dep in all CI workflows"
git push
```

78. CI fix — clone `instruction-files` sibling repo in all cargo-using GitHub Actions workflows.

## User

push fix

## Assistant

Pushed `889e349` to `agent-doc` main. CI triggered — 3 workflows queued (CI, mdBook, Book). Waiting for the CI run to confirm the fix.

Run `gh run watch 22473177698` from the agent-doc directory to monitor, or check GitHub Actions.

## User

Does instruction-files and agent-doc have a pending release?

Plugin: Maybe we should reduce the inactivity timeout to 1 second. The reason to keep focus in the editor is to not interrupt typing.

## Assistant

**instruction-files v0.1.3** — fully released. crates.io published, tag matches HEAD. No unreleased commits.

**agent-doc v0.4.4** — fully released (crates.io, PyPI, GitHub Releases). 2 commits after the tag are CI/docs only — no version bump needed.

**CI fix:** `actions/checkout@v4` doesn't allow paths outside the workspace. Fixed with `git clone --depth 1` instead. Pushed `8123b58`. **CI now passes.**

**Plugin timeout:** Reduced to 1 second. The logic is right — auto-focus should be fast enough that it doesn't interrupt typing, but slow enough to not flash during initial panel appearance. 1 second feels natural. Plugin rebuilt and signed.

79. Plugin auto-focus timeout reduced from 2s to 1s. CI fix confirmed passing.

## User

Plugin: Would it be possible to sync the tmux panes with the visible editor panes?
Would it be possible to orient the panes the same way as the editor. So if I have the editor on top, can the tmux panes reconfigure with the claimed files in open in the ide?

If I move to a file that not a claimed file, we could leave the previous pane as is.

Can we write a spec so the VSCode plugin has the same featureset as the Jetbrains plugin?

## Assistant

**Tmux pane focus sync: yes.** When you switch to a claimed file's editor tab, the plugin can run `tmux select-pane -t <pane-id>` to focus the corresponding tmux pane. The mapping already exists: file → session UUID (from YAML frontmatter) → pane ID (from `sessions.json`). Implementation is ~100 lines — a `FileEditorManagerListener` that fires on tab switch.

**Layout rearrangement: not feasible.** Tmux doesn't support dynamic pane reordering at runtime. There's no command to rearrange panes to match a visual layout. `swap-pane` swaps content but not position, and it would break the user's tmux workflow. Focus sync is the practical 80% solution — the right pane activates when you switch tabs.

**Non-claimed files:** Correct — if you switch to a file with no session in `sessions.json`, the plugin does nothing (previous pane stays active).

**VSCode plugin spec:** The current VSCode extension is bare minimum (submit only via `agent-doc route`). Here's the feature gap:

| Feature | JetBrains | VSCode |
|---|---|---|
| Submit (Ctrl+Shift+Alt+A) | yes | yes |
| Right-click context menu | yes | no |
| Prompt polling (`prompt --all`) | yes | no |
| Prompt panel (bottom bar) | yes | no |
| Hotkey selection (Alt+1..9, 1-9) | yes | no |
| Auto-focus timer (1s) | yes | no |
| Alt+Esc focus toggle | yes | no |
| Auto-save tracked files | yes | no |
| 3-way merge on external edit | yes | no |
| Tab-to-pane focus sync | planned | no |
| Dynamic plugin (no restart) | yes | n/a |

I'll write the spec to `agent-doc-vscode/SPEC.md`. Want me to implement the tab-focus sync in the JetBrains plugin now, or spec-first for both?

80. Tab-to-pane sync feasible (focus only, not layout). VSCode feature gap documented.

## User

What if we create a new window with the pane layout. Detaching the window whenever the claimed file orientation changes? And adding a new window with the orientation.

## Assistant

That's a solid approach. Here's the design:

**Mirror window:** A tmux window whose pane layout matches the editor's visible splits for claimed files.

**How it works:**
1. Plugin detects visible editor splits and which claimed files are in them
2. Create a new tmux window: `tmux new-window`
3. Split it to match: `tmux split-window -h` (vertical) or `-v` (horizontal)
4. Use `tmux join-pane -s <source-pane> -t <target-window>` to pull each Claude session's pane into the mirror window at the right position
5. Update `sessions.json` with new pane IDs

**When orientation changes** (user rearranges editor splits):
1. Detect layout change via `FileEditorManagerListener`
2. Break the mirror window apart: `tmux break-pane` each pane back to its own window (preserves running processes)
3. Create a new mirror window with the updated layout
4. `join-pane` the sessions back in at their new positions

**Key tmux commands:**
- `tmux join-pane -s %5 -t :new-window -h` — move pane %5 into target window, horizontal split
- `tmux break-pane -s %5` — detach pane back to its own window (process unaffected)
- `tmux select-layout -t :window tiled` — auto-arrange panes

**What `join-pane` + `break-pane` gives us:**
- Processes stay alive — Claude sessions never restart
- Clean transitions — break apart old layout, rebuild new one
- No empty shell panes left behind

**Phase plan:**
- **Phase 1:** Tab-focus sync (`tmux select-pane` on tab switch) — simple, useful now
- **Phase 2:** Mirror window with `join-pane`/`break-pane` for layout sync

Want me to implement Phase 1 now and spec Phase 2?

81. Mirror window design — `join-pane` to assemble, `break-pane` to disassemble, layout-aware recreation on orientation change.

## User

The tmux pane implementation should live with the other tmux logic...in agent-doc?

This would allow for a more portable solution between the Jetbrains and VSCode plugins.

## Assistant

Exactly right. The tmux logic belongs in `agent-doc` — the plugins just call CLI commands.

**Proposed `agent-doc` subcommands:**

```
agent-doc focus <file>          # Phase 1: select-pane for the file's session
agent-doc layout <file>...      # Phase 2: create/update mirror window with listed files
agent-doc layout --orientation h|v  # specify split direction
```

**Phase 1: `agent-doc focus <file>`**
- Look up file → session UUID → pane ID from `sessions.json`
- Run `tmux select-pane -t <pane-id>`
- Plugins just call this on tab switch — zero tmux knowledge needed

**Phase 2: `agent-doc layout <file1> <file2> [--orientation h]`**
- Create or update a mirror window with panes matching the given files
- `join-pane` / `break-pane` logic lives in `sessions.rs` alongside existing `send_keys`, `find_pane`, etc.
- Plugins call this when editor split layout changes, passing the list of visible claimed files and their orientation

**Plugin responsibility (minimal):**
- Detect tab switch → `agent-doc focus <file>`
- Detect split layout change → `agent-doc layout file1.md file2.md --orientation h`
- No tmux commands, no session parsing, no pane management

**agent-doc responsibility:**
- All tmux interaction (focus, layout, join, break, cleanup)
- Session registry management
- Pane ID tracking through layout changes

This keeps both plugins thin and portable. The VSCode extension could match the JetBrains plugin with minimal effort since it's just `exec("agent-doc focus ...")` and `exec("agent-doc layout ...")`.

82. Tmux pane management in agent-doc CLI — `focus` and `layout` subcommands. Plugins stay thin (just detect tab/split changes and call CLI).

## User

This is an opinionated design. As it assumes that the editor is above or below the tmux windows.
If the editor had a horizontal split and the tmux also had a horizontal split...and the tmux windows were above or below the editor, the configuration would be accurate but less ideal. So the implementation would nudge preference for the vertical split.

## Assistant

Good catch. The ideal tmux split depends on how the editor and terminal are physically arranged:

**If terminal is below the editor (most common):**
- Editor horizontal split (side by side) → tmux horizontal split → columns align vertically. Each file lines up with its Claude session. **Ideal.**
- Editor vertical split (stacked) → tmux vertical split → rows align. **Also ideal.**

**The problem you're describing:** If the editor has horizontal splits and tmux also splits horizontally, but they're stacked vertically, the *columns* still align — that's actually correct. The less ideal case is when the tmux orientation *doesn't* match the editor, creating a mismatch.

**Simple rule:** Mirror the editor's split orientation in tmux. The `layout` command should default to matching whatever the plugin reports:

```
agent-doc layout plan.md ron-berkes.md --split h   # horizontal (side by side)
agent-doc layout plan.md ron-berkes.md --split v   # vertical (stacked)
```

The plugin passes the editor's actual split direction. No guessing, no opinion — just mirror what the user chose in the editor.

**Config override** for users who want to force a preference:
```toml
# ~/.config/agent-doc/config.toml
[layout]
split = "h"   # always horizontal, regardless of editor
```

This keeps the default behavior correct (mirror editor) while letting users override if their physical layout doesn't match the assumption.

83. Layout split mirrors editor orientation by default. Config override for forced preference.

## User

document and implement

## Assistant

Done. Both `focus` and `layout` subcommands implemented in agent-doc, plus tab-sync in the JetBrains plugin.

**agent-doc CLI — new subcommands:**

**`agent-doc focus <file>`** — Focus the tmux pane for a session document.
- Reads session UUID from file's YAML frontmatter
- Looks up pane ID in `sessions.json`
- Runs `tmux select-pane -t <pane-id>`
- Exits with error if pane is dead or no session registered

**`agent-doc layout <file1> <file2> [--split h|v]`** — Arrange tmux panes to mirror editor split layout.
- Resolves each file to its session pane via frontmatter → `sessions.json`
- Uses `tmux join-pane` to move panes into the first pane's window
- `--split h` (default): horizontal/side-by-side. `--split v`: vertical/stacked
- Skips dead panes and files without sessions (warnings, not errors)
- Deduplicates panes (multiple files might share one)
- Single file falls back to `focus`

**Files changed in agent-doc:**
- `src/focus.rs` (new) — `focus` subcommand
- `src/layout.rs` (new) — `layout` subcommand with `Split` enum
- `src/sessions.rs` — added `select_pane`, `pane_window`, `join_pane`, `break_pane` to `Tmux` struct
- `src/main.rs` — `Focus` and `Layout` variants in `Commands` enum + dispatch

**JetBrains plugin — tab-to-pane sync:**
- `EditorTabSyncListener.kt` (new) — `FileEditorManagerListener` that fires on tab switch. If the new tab is a `.md` file, runs `agent-doc focus <relative-path>` in a background thread. Silently ignores failures (file might not have a session).
- `plugin.xml` — registered `EditorTabSyncListener` as `<projectListener>` on `FileEditorManagerListener`
- Plugin rebuilt and signed at `agent-doc-jetbrains/build/distributions/`

**Tests:** 21 integration tests pass, clippy clean. Binary installed to `~/.local/bin/agent-doc`.

84. `agent-doc focus` + `agent-doc layout` subcommands. JetBrains plugin tab-to-pane sync via `EditorTabSyncListener`. Plugin rebuilt and signed.

## User

The focus works. But when I switch to judith-krug, the switch to the tmux pane that claimed judith-krug did not happen.

It works when the pane is in the same window. But the judith-krug pane is in a different window.

## Assistant

**Bug:** `tmux select-pane -t %26` selects the pane within its window but doesn't switch the active window. Pane `%26` (judith-krug) is in window `@13`, while `%4` (plan.md) is in window `@1`. The user stays on window `@1` and never sees the switch.

**Fix:** Added `tmux select-window -t <pane-id>` before `select-pane` in `Tmux::select_pane()` (`sessions.rs`). Tmux resolves the pane's containing window automatically, so the same pane ID works for both commands.

Binary rebuilt and installed. 21 tests pass, clippy clean. `agent-doc focus tasks/judith-krug.md` now correctly switches to window `@13` and focuses pane `%26`.

85. Fix cross-window focus — `select-window` before `select-pane` so focus works across tmux windows.

## User

The switch to the judith-krug window + pane happened...but I also want to see the plan pane when I switch the right side editor to judith-krug. You mentioned there was an issue with moving panes. But would creating a new "swap" window work?

## Assistant

**Yes — implemented.** The `agent-doc layout` command already does this with `tmux join-pane`. Updated the plugin to use it.

**What changed:**

`EditorTabSyncListener` now detects all visible `.md` files across editor splits:
- **1 visible .md file** → `agent-doc focus <file>` (as before)
- **2+ visible .md files** → `agent-doc layout <file1> <file2> --split h|v`

Split orientation is auto-detected by walking the IntelliJ Swing component tree to find the `Splitter` instance. If the editor splits side-by-side (`Splitter.isVertical == false`), it passes `--split h`. If stacked (`isVertical == true`), `--split v`.

**How the layout works:**
1. `join-pane` pulls judith-krug's pane into plan.md's window → both visible side by side
2. When you close a split or switch to single-file view → `focus` takes over, switches to just that pane's window
3. Claude sessions stay alive through all transitions (join/break don't restart processes)

**Tested:** `agent-doc layout tasks/plan.md tasks/judith-krug.md --split h` successfully joined pane `%26` (judith-krug) into window `@1` (plan) side by side.

Plugin rebuilt and signed. Reinstall from `agent-doc-jetbrains/build/distributions/`.

86. Plugin multi-pane layout sync — detects split editors, calls `agent-doc layout` with orientation. Panes join/separate as editor splits change.

## User

Not quite right

## Assistant

**Fixed.** The issue: `ron-berkes.md` (pane `%5`) was already in window `@1` alongside `plan.md` (`%4`), so when `layout` joined `judith-krug.md` (`%26`), you got 3 panes instead of 2.

**What changed in `layout.rs`:** Before joining new panes, the layout command now **breaks out** any panes in the target window that aren't in the requested file list:

1. List all panes in the target window (`list_window_panes`)
2. Break out any that aren't in the file list (`break_pane` → moves to own window)
3. Then join the requested panes as before

**Added to `sessions.rs`:** `list_window_panes(window_id)` — lists all pane IDs in a tmux window.

**Tested:** `agent-doc layout tasks/plan.md tasks/judith-krug.md --split h` now correctly breaks out `%5` (ron-berkes) and leaves only `%4` (plan) + `%26` (judith-krug) side by side.

Binary rebuilt and installed. 21 tests pass, clippy clean.

87. Layout break-out fix — panes not in the file list are broken out of the target window before joining new ones.

## User

Some issues still

## Assistant

**Two issues identified from the screenshot:**

1. **Plugin not reinstalled** — the multi-pane `layout` integration was just added to the plugin. The running plugin still only had `focus`. The `layout` command works correctly from CLI.

2. **Race condition** — rapid tab switch events (close judith-krug, open ron-berkes) each spawn an independent thread. A stale `focus` from an intermediate state could overwrite the `layout` from the final state.

**Fix: debouncing.** Added 100ms debounce using `AtomicLong` generation counter. Each `selectionChanged` event bumps the counter, sleeps 100ms, then checks if it's still the latest. Only the final event's thread runs the command. This eliminates race conditions from rapid tab switches.

**Added diagnostic logging:** `System.err.println("[agent-doc] tab-sync: ...")` prints the command being run to the IDE log. Check `Help > Show Log in Files` to verify the plugin is calling `layout` vs `focus`.

Plugin rebuilt and signed. **Reinstall from `agent-doc-jetbrains/build/distributions/`** to get the multi-pane layout sync.

88. Plugin debounce (100ms) for tab-sync events. Diagnostic logging. Reinstall required for layout integration.

## User

We have the preceeding issue when switching to judith-krug. The plan1 window is hidden.

## Assistant

**Root cause: file ordering.** `FileEditorManager.selectedFiles` returns the most recently selected file first. When you click judith-krug, the order is `[judith-krug, plan]`. The layout used the first file's window as the target — judith-krug's solo window. It moved plan.md OUT of the shared window and INTO judith-krug's window, leaving the old layout window with only ron-berkes.

**Fix: smart target window selection.** The layout command now picks the target window with the most wanted panes. Tiebreaker: prefer the window with the most total panes (the existing layout window). This means:

- @1 has [plan, ron-berkes] (2 panes, 1 wanted) → total=2
- @19 has [judith-krug] (1 pane, 1 wanted) → total=1
- Winner: @1 (larger total) → break out ron-berkes, join judith-krug

**Verified transitions:**
1. plan+ron-berkes → plan+judith-krug: breaks out ron-berkes, joins judith-krug. Layout window stays @21.
2. plan+judith-krug → plan+ron-berkes: breaks out judith-krug, joins ron-berkes. Same window.
3. Both directions work — the layout window is stable, only the swapped pane changes.

Binary rebuilt and installed. 21 tests pass, clippy clean.

89. Layout target window fix — pick window with most wanted panes (tiebreak: most total panes). Stable layout window across pane swaps.

## User
The judith-krug pane is still alone

## Assistant

The binary fix works (verified from CLI), but the plugin isn't calling `layout`. Confirmed: no `/tmp/agent-doc-plugin.log` file exists, which means the plugin with the layout code was never installed.

**Plugin rebuilt with file-based diagnostic logging.** Reinstall from:
```
agent-doc-jetbrains/build/distributions/agent-doc-jetbrains-0.1.0-signed.zip
```

After reinstalling, switch tabs between split .md files and check:
```
cat /tmp/agent-doc-plugin.log
```

This will show exactly what `selectedFiles` returns and whether `layout` or `focus` is being called.

**To install:** Settings > Plugins > gear icon > Install Plugin from Disk > select the zip above.

90. Plugin diagnostic logging to `/tmp/agent-doc-plugin.log`. Reinstall required — current running plugin predates layout feature.

## User

When switching to judith-krug, the panes switch. But the selected pane is always the first pane after the swap.

## Assistant

**Fixed.** The layout was focusing the anchor pane (plan.md) instead of the file you just switched to (judith-krug). Changed `layout.rs` to focus the first file's pane — which is the most recently selected file from the plugin's `selectedFiles` array.

Now when you switch to judith-krug in the editor, the judith-krug tmux pane gets focused in the layout.

Binary rebuilt and installed.

91. Layout focus fix — focus the newly selected file's pane, not the anchor pane.

## User

audit docs

## Assistant

**Audit results:**

| Project | Issues | Status |
|---|---|---|
| agent-loop | 0 | clean |
| agent-doc | 4 → 0 | fixed |
| corky | 6 → 0 | fixed |

**agent-doc fixes (content updates):**
- `CLAUDE.md` — added `focus.rs` and `layout.rs` to Module Layout
- `SPEC.md` — added §7.10 focus and §7.11 layout subcommand specs
- `README.md` — added `focus` and `layout` to quick reference

**corky fixes (timestamp only):** All 6 docs were stale relative to `src/watch.rs` (v0.11.3 update) but content was already up to date. Touched to clear staleness.

92. Audit docs — agent-doc docs updated for focus/layout subcommands. Corky docs touched (content already current).

## User

PR. bump versions for agent-doc and corky. squash merge. release

## Assistant

**Released both projects.**

### agent-doc v0.5.1

| Channel | Status |
|---|---|
| crates.io | published |
| GitHub Release | [v0.5.1](https://github.com/btakita/agent-doc/releases/tag/v0.5.1) — 5 platform binaries |
| PyPI | workflow running |

- PR [#11](https://github.com/btakita/agent-doc/pull/11) — squash merged to main
- v0.4.4 → v0.5.0 (focus + layout subcommands)
- v0.5.0 → v0.5.1 hotfix: Windows build failed (`std::os::unix` in `start.rs`). Added `#[cfg(unix)]` gate with `Command::status()` fallback for non-unix.

### corky v0.12.0

| Channel | Status |
|---|---|
| crates.io | published |
| GitHub Release | [v0.12.0](https://github.com/btakita/corky/releases/tag/v0.12.0) — 5 platform binaries |
| PyPI | workflow running |

- PR [#16](https://github.com/btakita/corky/pull/16) — squash merged to main
- v0.11.5 → v0.12.0
- Fixed CI workflows (ci.yml, release.yml, pypi.yml) — instruction-files path dependency needs `git clone` step

### CI fixes applied to both repos

1. **instruction-files clone**: Added `git clone --depth 1` step to all workflows that need it
2. **PyPI manylinux Docker**: Added `before-script-linux` to maturin-action (host `git clone` doesn't map into Docker container)
3. **workflow_dispatch**: Added to pypi.yml so it can be manually triggered (releases created by GITHUB_TOKEN don't trigger `on: release` workflows)

93. Release — agent-doc v0.5.1, corky v0.12.0. crates.io + GitHub Releases complete. PyPI workflows in progress.

## User

We seem to be having issues with the submodules. The plugin source files are being added to the base repo. Do we need to add a skill for git submodules or just add the note to AGENTS.md?

❯ gs
On branch 20260225/topic-sync
Your branch is ahead of 'origin/20260225/topic-sync' by 130 commits.
(use "git push" to publish your local commits)

Changes not staged for commit:
(use "git add <file>..." to update what will be committed)
(use "git restore <file>..." to discard changes in working directory)
modified:   .agent-doc/sessions.json
modified:   .agent-doc/snapshots/2ca8a80b31ae5e069e73ca4cdd5fb5eca81f5492c015a247a640f00f50e46c5f.md
modified:   .agent-doc/snapshots/437a3262d8ad7f32cca4adf2c47f9b20144d72ef71309e29df41a8fa99a09dda.md
modified:   agent-doc (new commits)
modified:   agent-doc-jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPanel.kt
modified:   agent-doc-jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt
modified:   agent-doc-jetbrains/src/main/resources/META-INF/plugin.xml
modified:   corky (new commits)
modified:   tasks/plan.md
modified:   tasks/plan2.md
modified:   tasks/qwire-holdings.md
modified:   tasks/ron-berkes.md
modified:   tasks/toby-hill.md

Untracked files:
(use "git add <file>..." to include in what will be committed)
.agent-doc/snapshots/2436c93fa1a5a356e36d3bdf401c0fe151f7e68bc919070a639264bc48de8784.md
.agent-doc/snapshots/7172f8231f9e83d292555de24424ed994640e7d1f6dcfcf15027f317b69e2ce5.md
.agent-doc/snapshots/a10379bd365e0b0d581542dc3de458291873ab82c4b052cb6bfac20a364d1450.md
.agent-doc/snapshots/a4ce073e9643e347709808ca6606ddb70aa114a08f9a14d0050188aa3494baa7.md
agent-doc-jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt
agent-doc-jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt
bin/build-mail-filters

I also added tasks/plugins.md. It is not claimed by a claude session yet. When I switch the editor over to plugins.md, the tmux selection goes to the opposite toby-hill tmux pane. Even though the editor is on plugins.md.

## Assistant

Two separate issues, both fixed:

### 1. Submodule files in parent repo — note added to CLAUDE.md

`agent-doc-jetbrains/` is NOT a submodule — it's a regular directory in the parent repo. So those files showing up in `git status` is expected behavior. The actual submodules (`agent-doc`, `corky`) show as `(new commits)` because we pushed changes to them.

Added a section to `CLAUDE.md` clarifying which directories are submodules vs parent-repo directories, with git rules:
- Commit submodule changes from within the submodule
- Never stage files across repo boundaries in a single commit

No skill needed — the CLAUDE.md note is sufficient.

### 2. Unclaimed file focus — layout.rs fixed

**Root cause:** When plugins.md (no session) + toby-hill.md (has session) are sent to `agent-doc layout`, the code fell back to focusing the only resolved pane (toby-hill). But the user selected plugins.md — switching to toby-hill is wrong.

**Fix:** When < 2 panes resolve, only focus the first file's pane (the most recently selected file). If that file has no registered pane, don't change tmux focus at all. Return silently instead of erroring.

Binary rebuilt and installed.

94. Submodule note in CLAUDE.md + layout fix for unclaimed file focus.

## User

We should make agent-doc-jetbrains and agent-doc-vscode their own repos. They can also live in the agent-doc repo if the layout is clean.

## Assistant

**Recommendation: move into agent-doc repo under `plugins/`.**

```
agent-doc/
  src/                    # Rust CLI
  plugins/
    jetbrains/            # IntelliJ plugin (Kotlin/Gradle)
    vscode/               # VS Code extension (TypeScript)
  tests/
  docs/
  ...
```

**Why in-repo over separate repos:**
- Single place for all agent-doc tooling — CLI, plugins, skill
- Shared SPEC.md and VERSIONS.md — plugin changes documented alongside CLI changes
- Natural version coupling (plugins call CLI)
- Avoids small repos with separate release workflows

**Migration steps:**
1. Copy `agent-doc-jetbrains/` → `agent-doc/plugins/jetbrains/` (commit in submodule)
2. Copy `agent-doc-vscode/` → `agent-doc/plugins/vscode/` (commit in submodule)
3. Update Gradle/npm paths (`settings.gradle.kts`, `build.gradle.kts`, `package.json`)
4. Remove originals from parent repo
5. Update CLAUDE.md (both parent and agent-doc)
6. Add `plugins/jetbrains/` and `plugins/vscode/` to agent-doc's module layout in CLAUDE.md

Want me to proceed with the migration?

95. Proposed moving plugins into agent-doc repo under `plugins/` directory.

## User

Could this be confused with plugins to the agent-doc core library itself?

## Assistant

Yes — `plugins/` could imply extensibility hooks for agent-doc's engine. Better name: **`editors/`**.

```
agent-doc/
  src/                    # Rust CLI
  editors/
    jetbrains/            # IntelliJ plugin
    vscode/               # VS Code extension
```

"Editors" clearly means IDE integrations, not core extensibility. Alternative: `ide/` (shorter, same meaning). I'd go with `editors/` for clarity.

Ready to proceed with the migration using `editors/`?

## User

Yes.

## Assistant

**Done.** Migrated both editor plugins into the agent-doc repo.

**Changes in agent-doc submodule (pushed to main):**
- `editors/jetbrains/` — JetBrains plugin (Kotlin/Gradle), all source + gradle wrapper
- `editors/vscode/` — VS Code extension (TypeScript), source + package.json
- CLAUDE.md updated with `editors/` in module layout
- Certificate/build artifacts excluded via existing .gitignore

**Changes in parent repo (committed):**
- Removed `agent-doc-jetbrains/` and `agent-doc-vscode/` from git tracking
- Added both to `.gitignore` (old directories may still exist locally)
- Updated CLAUDE.md to document the move
- Updated agent-doc submodule pointer

The local `agent-doc-jetbrains/` and `agent-doc-vscode/` directories still exist on disk but are now gitignored. You can delete them when ready — the canonical source is now `agent-doc/editors/`.

96. Migrated editor plugins to `agent-doc/editors/jetbrains/` and `agent-doc/editors/vscode/`.

## User

Build both editor plugins.

## Assistant

**Both built successfully from `agent-doc/editors/`.**

| Plugin | Command | Output |
|---|---|---|
| JetBrains | `./gradlew buildPlugin` | `agent-doc-jetbrains-0.1.0.zip` (56KB) |
| VS Code | `npm install && npm run compile` | `out/extension.js` |

No path changes needed — Gradle and npm resolved everything correctly from the new location.

97. Both editor plugins build from `agent-doc/editors/`.

## User

clean me...I'm going to move editor talk to editor.md. Will be claimed by this caude session.

claim tasks/editors.md

## Assistant

Claimed `tasks/editors.md` for this pane (session `4aee457b`, pane `%4`). Ready to respond to editor topics there.

## User
clean me

