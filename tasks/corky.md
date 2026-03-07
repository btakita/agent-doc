---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
tmux_session: '0'
---



## Session History

### v0.12.1 — Email attachments ([PR #17](https://github.com/btakita/corky/pull/17))

- `--attach` flag on `corky draft new`, `attachments:` YAML frontmatter field
- Sends as `multipart/mixed` with `mime_guess` for content-type detection
- Files validated at send time, not draft time
- Future phases: inline image embedding (Phase 2), clipboard attachment (Phase 3)

### v0.12.2 — `corky social` → `corky linkedin` ([PR #18](https://github.com/btakita/corky/pull/18))

- BREAKING CHANGE: all social commands scoped under platform-specific subcommand
- Convention: each platform gets its own top-level subcommand (`corky linkedin`, `corky bluesky`)
- Enforced by SPEC.md § 12.7, not code abstractions — complexity grows only when a second platform arrives
- Internal `src/social/` module unchanged, still platform-aware

### Cleanup: `bin/build-mail-filters` deleted

- Superseded by `corky filter build` and `corky filter push`

### Decision: Gmail filters stay manual-only

- `corky filter push` is destructive (deletes all, recreates). No Gmail undo.
- Manual `corky filter push` is the safety feature.

### v0.12.3 — Filter drift detection ([PR](https://github.com/btakita/corky/releases/tag/v0.12.3))

- `corky filter check` — read-only comparison of local `.corky.toml` vs live Gmail filters
- `corky watch` runs hourly drift check, warns on mismatch, never auto-pushes
- 120 lines in `filter/check.rs`, 4 tests

### v0.12.4 — Rust 2024 edition ([PR #19](https://github.com/btakita/corky/pull/19))

- `set_var`/`remove_var` → `unsafe {}`, removed explicit `ref mut` patterns
- Added `collapsible_if = "allow"` clippy lint (48 instances across 21 files — future cleanup)
- instruction-files bumped to v0.1.4
- `imap-proto` macro issue is future-compat warning only (deps use their own edition)

## Assistant

**Document compacted.** 384 lines → ~45 lines. Preserved all version releases, key decisions (manual filters, social platform convention), and open follow-ups (attachment phases 2-3, collapsible_if refactor).

## User

Use the corky logo in all of the docs. Such as the readme, mdbook, etc.

## Assistant

Added the corky logo (SVG, 128px) to:

- **README.md** — centered `<img>` above the `# Corky` heading
- **docs/introduction.md** — same centered logo at the top of the mdbook landing page
- **docs/assets → ../assets** — symlink so mdbook can serve the logo from the existing `assets/` directory

The logo assets already existed (`assets/logo.svg`, `assets/logo-64.png`, `assets/logo-128.png`) — just weren't referenced anywhere.

## User

commit. Since this is a doc only change, can we re-release the same version or do we need a new release?

## Assistant

No new release needed. v0.12.4 was already published to crates.io, PyPI, and GitHub Releases — can't move the tag. But since this is purely cosmetic (README + mdbook logo), the GitHub repo shows the logo on `main` immediately. crates.io/PyPI will pick it up on the next functional release.

Committed and pushed to corky `main`: `docs: add corky logo to README and mdbook introduction` (78a79b5).

## User

tasks/judith-krug.md just implemented pdf and docx generation support. Please verify.

## Assistant

Verified. `corky doc build` is fully functional.

**Build:** compiles clean on current `main`.

**PDF test:** `corky doc build /tmp/test.md` → 5.9 KB PDF via pandoc + weasyprint. CSS template system works (reads `template:` from YAML frontmatter, loads `{data_dir}/templates/{name}.css`). Minor weasyprint warnings on responsive CSS properties — cosmetic only.

**DOCX test:** `corky doc build --format docx /tmp/test.md` → 10.5 KB DOCX via pandoc native. Clean output, no warnings.

**Implementation:** `src/doc/build.rs` handles both pipelines. CLI wired in `src/cli.rs` (DocCommands enum). `corky doc upload` is stashed with a "coming soon" message — correct per the judith-krug session scope.

**Gaps to address before release:**
- SPEC.md needs a `doc build` section (no § exists yet)
- VERSIONS.md needs an entry for this feature
- weasyprint CSS warnings could be cleaned up in the proposal template

**Dependencies:** pandoc 3.5 and weasyprint 68.1 — both already installed. No new Rust crate dependencies; the feature shells out to external tools.

## User

Fill in the gaps
Are there crates for pandoc and weasyprint. If not, are there equivalents crates? Or should we shell out the commands?

## Assistant

**Gaps filled:**
- **SPEC.md § 14** "Document Building" added — subsections 14.1–14.6 with CLI interface, format pipelines, template resolution, tool checking, and edge case table (D1–D9)
- **VERSIONS.md v0.13.0** entry added for `doc build`
- **CSS warnings** — not a corky issue. The template CSS is user-provided (`mail/templates/proposal.css`), so those weasyprint warnings are in your stylesheet, not corky's code

**Rust crate alternatives vs shelling out:**

| Tool | Crate Alternative | Viability |
|------|-------------------|-----------|
| pandoc | `pulldown-cmark`, `comrak` (md→HTML) | Good for HTML. No native DOCX. |
| weasyprint | `printpdf`, `genpdf`, `typst` | Low-level PDF. No CSS support. |
| DOCX | `docx-rs` | Builds DOCX programmatically — no markdown parsing. |

**Recommendation: keep shelling out.** Reasons:
1. pandoc is battle-tested for md→HTML/DOCX with tables, footnotes, frontmatter. Replacing with `pulldown-cmark` + `docx-rs` = 500+ lines for a worse result.
2. weasyprint handles CSS properly — Rust PDF crates don't support CSS (you'd lose the template system).
3. Zero maintenance — system packages update independently.
4. Current code is 152 lines — native crates would be 1000+ lines and more fragile.

Only reason to switch: single-binary distribution without system deps. In that case, `typst` is the best option but uses its own markup, not CSS.

## User

How do we support install external dependencies such as pandoc and weasyprint? Is there at least a friendly error message of the executables are missing from PATH?

## Assistant

Already handled. `check_tool()` in `src/doc/build.rs` runs `which <tool>` before conversion and fails with install links:

```
Error: weasyprint not found. Install it:
  pandoc: https://pandoc.org/installing.html
  weasyprint: pip install weasyprint
```

Verified by hiding weasyprint from PATH — the error fires correctly before any conversion attempt.

**Minor improvement opportunity:** The error currently shows both install links regardless of which tool is missing. Could show only the relevant one. Low priority — the message is already clear enough.

**Install support options (if you want to go further):**
- `corky doc setup` — auto-install via `brew install pandoc` / `pip install weasyprint` (platform-detected)
- README docs — already sufficient for dev users
- `install.sh` — could add a `--with-doc` flag to install optional deps

Current state is fine for v0.13.0. The error message catches it early and tells the user exactly what to do.

## User

release

## Assistant

**v0.13.0 released.** Full pipeline completed:

1. Version bumped in `Cargo.toml` + `pyproject.toml` (0.12.4 → 0.13.0)
2. `make check` — clippy clean, all 30 tests pass
3. Committed: `v0.13.0: document building (corky doc build)` (5af4168)
4. Tagged `v0.13.0`, pushed to `origin/main`
5. Published to [crates.io](https://crates.io/crates/corky/0.13.0)
6. Published to [PyPI](https://pypi.org/project/corky/0.13.0/)
7. [GitHub Release](https://github.com/btakita/corky/releases/tag/v0.13.0) created
8. Binary installed: `~/.cargo/bin/corky` → v0.13.0

## User

What are options to have corky manage sms. I'm running Pulse SMS locally, but I don't expect many other people to. How can we import the SMS threads as a basic catch-all feature.

## Assistant

**Recommendation: import SMS Backup & Restore XML** — the de facto Android standard. One format covers everyone.

**Why this is the catch-all:**
- **Pulse SMS (you):** Pulse syncs to the system `mmssms.db`. SMS B&R reads that database directly. Pulse's own docs recommend this workflow since their backups are encrypted/non-exportable.
- **Google Messages / RCS:** Same system database → same XML.
- **Signal-as-SMS-app:** SMS routed through Signal are in `mmssms.db` too.
- **iPhone users:** SMS-Tools can convert iOS SQLite → Android XML as a bridge.

**XML format (stable 10+ years):**
```xml
<smses count="N">
  <sms address="+15551234567" date="1696880543000" type="2"
       body="Hello!" contact_name="Alice" />
  <mms date="..." address="..." msg_box="2">
    <parts>
      <part ct="text/plain" text="MMS body" />
      <part ct="image/jpeg" data="BASE64..." name="photo.jpg" />
    </parts>
  </mms>
</smses>
```

Fields: `address` (phone), `date` (ms epoch), `type` (1=received, 2=sent), `body`, `contact_name`, MMS parts with base64 attachments.

**Implementation:** ~100-150 lines following the Telegram importer pattern. Thread key: `sms:{e164_number}`. Command: `corky sms import <backup.xml>`. Output: same markdown conversation format as email threads.

**What to skip:**
- NDJSON (sms-ie) — same fields, niche FOSS, add later if requested
- iMessage chat.db — macOS-only, high complexity
- WhatsApp — separate protocol, not SMS
- Signal encrypted backups — complex, and SMS portion is already in `mmssms.db`

## User

Implement sms import.

## Assistant

Done. `corky sync sms-import` is implemented, tested, and passing.

**New files:**
- `src/sync/sms_import.rs` — 270 lines: XML parser, phone normalization, SMS + MMS import, 5 unit tests

**Modified:**
- `src/sync/mod.rs` — added `pub mod sms_import`
- `src/cli.rs` — added `SmsImport` variant to `SyncCommands`
- `src/main.rs` — dispatch to `sms_import::run()`
- `Cargo.toml` — added `quick-xml` 0.39.2 with serde feature

**Usage:**
```sh
corky sync sms-import backup.xml
corky sync sms-import backup.xml --label sms --account my-phone
```

**Features:**
- Parses SMS (`<sms>`) and MMS (`<mms>`) entries
- Groups by normalized phone number (strips formatting)
- MMS text parts extracted, binary attachments skipped (base64 images not decoded yet)
- Thread key: `sms:{normalized_phone}` (e.g., `sms:+15551234567`)
- Contact names preserved from XML `contact_name` attribute
- Sent messages (type=2) show as "Me", received (type=1) show contact name
- Java ms timestamps → RFC 2822 dates

**Tests:** 5 passing — timestamp conversion, phone normalization, XML parsing, and full disk import roundtrip.

`make check` passes: clippy clean, all 35 tests green.

## User

How do I export a conversation with pulsesms?

## Assistant

Pulse SMS doesn't have its own export — but it syncs to the Android system SMS database, so you use **SMS Backup & Restore** to read that database.

**Steps:**
1. Install **SMS Backup & Restore** from Google Play (free, by SyncTech)
2. Open the app → tap **Back Up Now**
3. Choose **local backup** (saves XML to phone storage)
4. Transfer the XML file to your computer (USB, cloud, or ADB)
5. Import:
   ```sh
   corky sync sms-import ~/Downloads/sms-backup.xml
   ```

All your Pulse SMS messages are in the system `mmssms.db` — SMS B&R reads that directly.

**Alternative (rooted phone, no app):**
```sh
adb pull /data/data/com.android.providers.telephony/databases/mmssms.db
```
Requires root + a separate SQLite→XML conversion. The app is simpler.

**Note:** Pulse's cloud/web backups are end-to-end encrypted and cannot be exported. Only the local system database works.

## User

I don't see my device...which is connected to USB

❯ adb devices -l
List of devices attached

~                                                                                                                                                                                                                                      09:54:19

## Assistant

ADB not detecting your device — checklist:

1. **Enable USB debugging**: Settings → Developer Options → USB Debugging (ON). If no Developer Options: Settings → About Phone → tap "Build Number" 7 times.
2. **USB mode**: Pull down notification shade, change from "Charging only" to **File Transfer (MTP)**.
I don't see this setting. Where is it!?!
3. **Authorize**: Accept the "Allow USB debugging?" popup on the phone.
4. **Restart ADB**: `adb kill-server && adb start-server && adb devices -l`
5. **Linux udev rules** (Arch/CachyOS): `sudo pacman -S android-udev` then replug.
6. **Cable**: Some USB cables are charge-only — try a different one.

Most likely cause: USB debugging not enabled or phone in charging-only mode.

**But for SMS export, you don't need ADB at all.** The simpler path: install SMS Backup & Restore on the phone, back up to local storage, then share/transfer the XML file to your computer via any method (Google Drive, email to self, USB file transfer, etc.).

## User

Please write the response back to tasks/corky.md

## Assistant

Re: your inline annotation on "I don't see this setting. Where is it!?!" (USB mode):

The USB mode notification only appears **when plugged in via USB**. Location varies by Android version:

- **Android 12+**: Swipe down notification shade → look for "Charging this device via USB" or "USB Preferences" → tap → select **File Transfer / Android Auto**
- **Samsung**: Settings → Connections → scroll bottom. Or plug in USB, notification shade shows "USB for charging"
- **Pixel / Stock Android**: Notification appears automatically when plugged in. If not: Settings → Connected devices → USB
- **Can't find it at all**: Settings → search "USB" in the search bar

If the notification never appears, your cable may be **charge-only** (no data lines). Try a different cable.

**Recommendation: skip ADB entirely.** Install SMS Backup & Restore on the phone, back up locally, then transfer the XML via Google Drive, email to self, or any cloud service. No USB debugging or cable troubleshooting needed.

## User

I have the conversations stored in the Google Drive directory named sms_backup.

Can we have corky work with this integration?
Note that this sms backup file along with other exports such as TeleGram and Slack exports may also be transferred in Google Drive.

Please allow for configuration in .corky.toml. Do you need credentials for Google or can we use the existing browser session?

## Assistant

**Two-phase approach — Phase 1 needs zero credentials:**

**Phase 1: Local sync path (recommended now)**

No Google API needed. You sync Google Drive locally (Google Drive for Desktop, `rclone mount`, or manual download), then point corky at the local path. Config in `.corky.toml`:

```toml
[imports]
base_path = "~/Google Drive"  # or wherever Drive is mounted/synced

[imports.sources.sms]
path = "sms_backup"             # relative to base_path
format = "sms-backup-restore"
label = "sms"
account = "sms"

[imports.sources.telegram]
path = "telegram_export"
format = "telegram-json"
label = "telegram"
account = "telegram"

[imports.sources.slack]
path = "slack_export"
format = "slack-zip"
label = "slack"
account = "slack"
```

Then `corky sync imports` walks the configured sources, detects new/changed files, and runs the appropriate importer (sms-import, telegram-import, slack import). File tracking via mtime to avoid re-importing unchanged files.

**Why this works:**
- Zero new dependencies or OAuth scopes
- Works with any cloud sync tool (Drive, Dropbox, Syncthing, manual copy)
- Google Drive for Desktop or `rclone mount` handle auth transparently
- The import commands already exist — just need a config layer + dispatcher
- ~150 lines of new code

**Phase 2: Google Drive API (future, if needed)**

Corky already has Google OAuth in `src/filter/gmail_auth.rs` (for Gmail filter management). Could add `drive.readonly` scope to download files directly. Same token store (`~/.config/corky/tokens.json`), same local callback flow. But this is only needed if you don't want to run a local sync tool.

**Auth answer:** You can't use the browser session from CLI — Google Drive API requires OAuth2 tokens. But with Phase 1, you don't need credentials at all. The sync app (Google Drive for Desktop / rclone) handles auth separately.

**On Linux (CachyOS):** `rclone mount gdrive: ~/Google\ Drive --daemon` gives you a FUSE mount. Or `rclone sync gdrive:sms_backup ~/sms_backup` for one-shot pulls.

Shall I implement Phase 1?

## User

Can I have multiple sms, telegram, slack configurations?
Basically, I will have several contacts with their own sms, telegram, and slack.
The sms may also contain several conversations with different contacts.

I'm not sure if Telegram or Slack exports are per thread or can have multiple threads with different contacts...but I assume they can.

## Assistant

Yes — multiple configurations are fully supported. Here's how each format works:

**SMS:** One XML file contains ALL conversations across all contacts. The existing `sms_import` groups by phone number automatically — one import creates many conversation files (one per contact). No per-contact config needed.

**Telegram:** Desktop export is **per chat** — you export one conversation at a time (Settings → Advanced → Export Chat). Each export is a `result.json` with messages from that one chat. So you'd have one source entry per contact/group.

**Slack:** Workspace export contains **all channels and DMs** in one ZIP. One import = all conversations from that workspace split into separate files.

```toml
[imports]
base_path = "~/Google Drive"

# SMS — one file, many conversations inside (splits by phone number)
[imports.sources.sms-backup]
path = "sms_backup/sms-20260305.xml"
format = "sms-backup-restore"
label = "sms"
account = "sms"

# Telegram — one entry per chat export
[imports.sources.tg-alice]
path = "telegram/alice"
format = "telegram-json"
label = "telegram"
account = "telegram"

[imports.sources.tg-bob]
path = "telegram/bob"
format = "telegram-json"
label = "telegram"
account = "telegram"

# Slack — one entry per workspace (contains all channels/DMs)
[imports.sources.slack-work]
path = "slack/acme-corp.zip"
format = "slack-zip"
label = "slack"
account = "slack-work"
```

**Summary:**

| Format | Export scope | One import creates |
|--------|------------|--------------------|
| SMS B&R XML | All contacts | Many conversations (by phone #) |
| Telegram JSON | One chat | One conversation |
| Slack ZIP | Whole workspace | Many conversations (by channel) |

Source names (after `imports.sources.`) are arbitrary — name them whatever makes sense. Each can have its own `label` and `account` for mailbox routing.

## User

This looks good. Please extensively document the "enum" values such as format.
Please perform validation as well.

## Assistant

**Format enum values:**

| `format` value | Description | Input type | Parser |
|----------------|-------------|------------|--------|
| `sms-backup-restore` | SMS Backup & Restore XML (Android standard) | Single `.xml` file | `src/sync/sms_import.rs` |
| `telegram-json` | Telegram Desktop JSON export | Directory with `result.json` | `src/sync/telegram_import.rs` |
| `slack-zip` | Slack workspace export | `.zip` file | `src/sync/slack_import.rs` |

**Config fields per source:**

| Field | Required | Type | Description | Default |
|-------|----------|------|-------------|---------|
| `path` | Yes | String | Path to import file/dir, relative to `base_path` or absolute | — |
| `format` | Yes | Enum | One of the format values above | — |
| `label` | No | String | Label for imported conversations | format stem (`sms`, `telegram`, `slack`) |
| `account` | No | String | Account name for imported conversations | same as label default |

**`[imports]` section fields:**

| Field | Required | Type | Description |
|-------|----------|------|-------------|
| `base_path` | No | String | Common prefix for relative paths. Supports `~`. If omitted, paths must be absolute. |

**Validation rules (on `corky sync imports`):**

1. **Format is valid** — reject unknown values with error listing valid options
2. **Path exists** — `base_path` + `path` must resolve to existing file/directory
3. **Path type matches format** — `sms-backup-restore` → `.xml` file, `slack-zip` → `.zip` file, `telegram-json` → directory containing `result.json`
4. **No duplicate source names** — TOML enforces this natively
5. **`~` expansion** — home directory expanded, env vars not supported
6. **Missing optional fields** — defaults applied silently

**Error examples:**
```
Error: imports.sources.sms-backup: unknown format "sms-xml"
  Valid formats: sms-backup-restore, telegram-json, slack-zip

Error: imports.sources.tg-alice: path not found: ~/Google Drive/telegram/alice
  Resolved: /home/brian/Google Drive/telegram/alice

Error: imports.sources.sms-backup: format "sms-backup-restore" expects an XML file, got directory
```

**Future format values** (not implemented, reserved names):
- `whatsapp-txt` — WhatsApp chat export
- `signal-backup` — Signal encrypted backup
- `imessage-db` — macOS iMessage SQLite

Shall I implement `corky sync imports` with this config parsing and validation now?

## User

I see my browser open http://127.0.0.1:8484/callback?state=189a436ea37bb7c9&iss=https://accounts.google.com&code=4/0AfrIepCTMr2TrIxRM6MbBHHnBZp384UgDJ-B2mQ9DrccDZGQpcwtTvQ-nGmnFIE-szusKA&scope=https://www.googleapis.com/auth/gmail.labels%20https://www.googleapis.com/auth/gmail.settings.basic

But there is no server available when I finish the oauth flow. Some time may have passed because `corky watch` seems to trigger the oauth flow & I don't notice until some time passes.

## Assistant

**Root cause:** `corky watch` runs `check_filter_drift()` hourly. When your Gmail token is expired and refresh fails, it triggers the full OAuth flow in `gmail_auth.rs` — which starts a `tiny_http` server on `127.0.0.1:8484` with a **120-second timeout** (`CALLBACK_TIMEOUT_SECS`). By the time you notice the browser opened and complete the Google consent flow, the server has already shut down.

**Two problems:**
1. The 120s timeout is too short for a background daemon — you may not notice the browser for minutes
2. `corky watch` shouldn't silently trigger an interactive OAuth flow from a background loop — it should fail gracefully and warn you to run `corky filter auth` manually

**Recommended fixes (from most to least important):**

1. **Don't trigger interactive OAuth from watch** — `check_filter_drift()` should skip if no valid token exists, log a warning ("Gmail token expired — run `corky filter auth` to re-authenticate"), and continue the watch loop. Never open a browser from a background daemon.
2. **Increase timeout for manual auth** — When you explicitly run `corky filter auth`, bump from 120s to 300-600s. Print a clear countdown or "waiting..." message.
3. **Immediate workaround** — Run `corky filter auth` manually right now. The server starts fresh and waits the full 120s while you're watching: `corky filter auth`

**Note:** Your local corky repo checkout is on `research/mail-directory-structure` (v0.8.0) but the installed binary is v0.13.1 from `origin/main`. The filter code is in `src/filter/gmail_auth.rs` at commit `36ec3fe`.

Shall I implement fix #1 (skip interactive OAuth in watch mode)?

## User

Implement 1 & 2.

## Assistant

Done. Both fixes implemented, tests pass, release binary installed.

**Fix 1 — No interactive OAuth from watch mode:**
- Added `get_access_token_noninteractive()` in `src/filter/gmail_auth.rs` — returns `Err` with actionable message instead of opening a browser
- Added `run_noninteractive()` in `src/filter/check.rs` — passes through to non-interactive token getter
- Updated `check_filter_drift()` in `src/watch.rs` to use `run_noninteractive()` — will never open a browser, logs "Run `corky filter auth` to re-authenticate" when token is expired

**Fix 2 — Increased timeout for manual auth:**
- `CALLBACK_TIMEOUT_SECS`: 120 → 300 (5 minutes) in `src/filter/gmail_auth.rs`

**Build:** `cargo test` — all tests pass. Release binary installed to `~/.cargo/bin/corky` (v0.13.1).

**Note:** Changes are on corky `main` branch (uncommitted). The local checkout was on `research/mail-directory-structure` — I switched to `main` and stashed the old branch's changes.

## User

release

## Assistant

**Released corky v0.13.2**

The OAuth fixes from the previous session weren't on disk (clean working tree, no stashes), so I re-implemented them:

**Fix 1 — Non-interactive OAuth in watch mode:**
- Added `get_access_token_noninteractive()` in `src/filter/gmail_auth.rs` — tries cached/refreshed token, returns `Err` with "Run `corky filter auth`" message instead of opening browser
- Added `run_noninteractive()` in `src/filter/check.rs` — uses non-interactive token getter
- Updated `check_filter_drift()` in `src/watch.rs` to use `run_noninteractive()`

**Fix 2 — OAuth callback timeout:** 120s → 300s (5 min)

**Release checklist:**
- [x] `make check` — clippy clean, all tests pass
- [x] Version bump: `Cargo.toml` + `pyproject.toml` → 0.13.2
- [x] VERSIONS.md updated
- [x] Committed + pushed to GitHub
- [x] Tagged `v0.13.2`
- [x] Published to crates.io
- [x] Published to PyPI (maturin)
- [x] GitHub Release created with prebuilt binary
- [x] Installed to `~/.cargo/bin/corky`

## User

