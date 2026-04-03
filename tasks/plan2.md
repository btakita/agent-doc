---
agent_doc_session: 71d57261-c118-423b-82d3-89b4871157f6
tmux_session: '0'
---

Set the env variable into .corky.toml

GMAIL_SYNC_DAYS=3650

The Jetbrains Plugin is no longer showing the prompts from the claude session.

## Assistant

**1. `GMAIL_SYNC_DAYS=3650`** — Already set. `mail/.corky.toml` line 33 has `sync_days = 3650` under `[gmail]`. No change needed.

**2. JetBrains plugin — prompts not showing**

Found the likely cause: a race condition in `PromptPoller.kt`.

`autoSaveTrackedFiles()` was changed from **`invokeAndWait`** (blocking) to **`invokeLater`** (non-blocking). This means the poll cycle now runs like:

1. `autoSaveTrackedFiles()` — queues save, returns immediately
2. `refreshTrackedFiles()` — runs before save completes
3. `agent-doc prompt --all` — polls with stale file state
4. Claude doesn't see latest edits → prompts don't generate or appear inactive

**Fix**: Change `invokeLater` back to `invokeAndWait` in `autoSaveTrackedFiles()`, or restructure so the poll waits for the save to complete before checking for prompts.

Want me to fix this?

## User

What do you recommend? It seems that waiting for the save to complete seems more sound...why are we using invokeLater?

## Assistant

You're right — `invokeAndWait` is the correct choice here.

The comment says `invokeLater` was used to "avoid deadlocking the poller thread." But that concern is unfounded: deadlock requires a circular wait, and the poller thread is a standalone daemon that only runs CLI commands. The EDT has no dependency on it. No circular dependency = no deadlock risk.

**The fix:** Change `invokeLater` → `invokeAndWait` in `autoSaveTrackedFiles()` (line 91 of `PromptPoller.kt`). This ensures the poll cycle is:

1. Save unsaved documents (blocking — waits for completion)
2. Refresh tracked files (sees the saved state)
3. Poll `agent-doc prompt --all` (Claude sees the latest edits)
4. Show prompt if active

Applying the fix now.

## User

push labels to gmail

## Assistant

`corky filter push` exists but needs re-authentication — got `403 Insufficient Permissions`. The token is missing the required Gmail scopes.

Run:
```
corky filter auth
```

This will open a browser for Gmail OAuth. Once authenticated, you can push:
```
corky filter push --dry-run    # preview
corky filter push              # apply
```

Note: `filter push` **replaces all existing Gmail filters** — so the dry-run first is important to verify.

## User

corky filter push --dry-run

## Assistant

Both done.

**Auth:** Succeeded — token stored as `gmail:default`.

**Dry run result:**
- Would **delete all 26 existing filters** (many are legacy — old mailing lists like ruby-talk, rspec-devel, selenium, etc.)
- Would **create 2 new filters** matching `.corky.toml`:
  1. `correspondence` filter — matches the addresses list (tomtaira, martin-investments, judithkrug, etc.), adds labels + IMPORTANT
  2. `for-lucas` filter — matches upwork, wellfound, linkedin, etc., adds labels + STARRED + IMPORTANT, removes SPAM, forwards to dev@briantakita.me

**Warning:** This deletes 26 existing Gmail filters and replaces them with 2. Most of the old ones look obsolete (Ruby mailing lists, Grockit, Textdrive, etc.), but confirm before running `corky filter push` without `--dry-run`.

## User

corky filter push

## Assistant

Done. Deleted 26 legacy filters, created 2 new ones:

1. **`correspondence`** — tomtaira, martin-investments, judithkrug, etc. → labels + IMPORTANT
2. **`for-lucas`** — upwork, wellfound, linkedin, etc. → labels + STARRED + IMPORTANT, no spam, forwards to dev@briantakita.me

Gmail filters now match `.corky.toml` exactly.

## User

Should topics be stored in .corky.toml?

## Assistant

They already are. Corky has a `[topics.*]` config section in `.corky.toml`:

```toml
[topics.manufacturing-china]
keywords = ["manufacturing", "smart contract", "PO", "escrow"]
contacts = ["ron-berkes"]
description = "Smart contract supply chain for China manufacturing"
```

**Existing infrastructure:**
- `corky topics list` / `add` / `info` / `suggest`
- Bidirectional sync: `mail/topics/{name}/` ↔ `mail/mailboxes/{mailbox}/topics/{name}/`
- Keyword matching against conversation subjects for discovery
- Content files (README.md) live in `mail/topics/`, config in `.corky.toml`

The `manufacturing-china` topic directory was created at `mail/topics/manufacturing-china/README.md` but not yet registered in `.corky.toml`. Want me to run `corky topics add manufacturing-china` or add it manually?

## User

Can corky watch also detect upgrades to corky...and restart when corky is upgraded?

## Assistant

Yes — most of the infrastructure already exists.

**Already in corky:**
- `upgrade.rs` — checks crates.io for newer versions (24-hour cache), downloads prebuilt binary from GitHub Releases, atomic file replacement
- `warn_if_outdated()` — runs on every startup, prints warning to stderr
- `corky upgrade` — manual self-update command

**Missing:** wiring it into the `watch` loop + auto-restart.

**Implementation:**

1. **Periodic check** — once per hour (not every poll cycle), call `check_for_update()` in the watch loop
2. **Auto-upgrade** — if newer version found, download via `try_github_release_upgrade()` (atomic binary replace)
3. **Self-restart** — use Unix `exec()` to replace the current process with the new binary, preserving args and working directory

```rust
// In watch loop, once per hour:
if let Some(new_ver) = upgrade::check_for_update()? {
    eprintln!("Upgrading corky {} → {}...", env!("CARGO_PKG_VERSION"), new_ver);
    upgrade::run()?;
    // Re-exec self — replaces process image, no downtime
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .args(std::env::args().skip(1))
        .exec(); // never returns on success
}
```

Key property: `exec()` replaces the process in-place. No zombie, no missed polls, seamless upgrade. The watch daemon restarts under the new version with the same arguments.

**Config option:** `auto_upgrade = true` in `[watch]` section of `.corky.toml`, defaulting to off for safety.

Want me to implement this?

## User

y
