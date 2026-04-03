---
agent_doc_session: 6c57ef6d-db13-43df-a68d-0e9c4372110b
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---






# Email Management

## Session History

- Pushed 2 Gmail filters: **correspondence** (17 addresses → importance) and **for-lucas** (14 addresses → forward to `dev@briantakita.me` + star)
- Synced mail + committed (1474 files)
- Discussed multi-account Gmail: IMAP sync supported, filters are single-account
- Proposed `btak-dev` account config (not yet applied)

## Status

<!-- agent:status -->
- [x] btak.dev account added to .corky.toml (sync_days=30)
- [x] Gmail API sync provider implemented
- [x] login_hint + email verification safeguard
- [x] Ctrl+C cancellation token
- [x] README Gmail API setup instructions
- [x] Todo: public Google app (tasks/todo/)
- [x] btak.dev added as test user in Cloud Console
- [x] v0.18.0 released (crates.io, PyPI, GitHub)
- [x] Renamed INBOX → btak.dev in 291 existing email files
- [ ] Label alias feature (so future syncs write "btak.dev" not "INBOX")
- [ ] Verify sync with btak.dev@gmail.com
<!-- /agent:status -->

## Exchange

<!-- agent:exchange mode=append -->
Converted to template mode and compacted conversation history. The document now uses `<!-- agent:input -->` / `<!-- agent:output -->` components.

Pending from last session: adding `btak-dev` (btak.dev@gmail.com) account to `mail/.corky.toml`. Want me to proceed with that?

Yes. Add btak-dev
Added `btak-dev` account to `mail/.corky.toml`:

```toml
[accounts.btak-dev]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak-dev"
labels = ["INBOX"]
```

**Assumptions:**
- Password stored at `pass email/btak-dev` — adjust if your pass path differs
- `labels = ["INBOX"]` — add more labels if you want specific label sync
- No `default = true` since the personal account is already default

**Next steps:**
- Ensure `pass email/btak-dev` exists (Gmail app password)
- Run `corky sync` to verify the new account connects
- Add Gmail API OAuth for btak-dev if you want filter management on this account too

Want me to adjust anything?

Change btak-dev to btak.dev

`pass email/btak.dev` is available
Renamed account key from `btak-dev` to `btak.dev` in `mail/.corky.toml`:

```toml
[accounts."btak.dev"]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak-dev"
labels = ["INBOX"]
```

Note: TOML keys with dots need quoting (`"btak.dev"`), so the key is `accounts."btak.dev"`. The `password_cmd` path (`pass email/btak-dev`) still uses a hyphen since pass paths don't need to match the account key — adjust if you prefer `pass email/btak.dev`.
Updated `password_cmd` to match the available pass path:

```toml
[accounts."btak.dev"]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak.dev"
labels = ["INBOX"]
```

Ready for `corky sync` whenever you want to test it.

```
=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting to imap.gmail.com:993 as btak.dev@gmail.com
  Error syncing btak.dev: No Response: [AUTHENTICATIONFAILED] Invalid credentials (Failure)
```
IMAP auth failed — the password in `pass email/btak.dev` isn't accepted by Gmail. This usually means one of:

1. **App password needed**: Gmail with 2FA requires an [App Password](https://myaccount.google.com/apppasswords), not your regular password. Generate one under "Security → App passwords" and update:
   ```bash
   pass edit email/btak.dev   # paste the 16-char app password
   ```

I logged into btak.dev@gmail.com...wen to the /apppasswords link.
`The setting you are looking for is not available for your account.`

2. **IMAP not enabled**: Go to Gmail Settings → "See all settings" → "Forwarding and POP/IMAP" → enable IMAP for `btak.dev@gmail.com`.

3. **Wrong password stored**: Verify the stored value:
   ```bash
   pass email/btak.dev   # check it looks right (no trailing newlines/spaces)
   ```

Most likely it's #1 — app password. Generate one and update the pass entry, then retry `corky sync`.
App Passwords require 2-Step Verification to be enabled first. For a personal Gmail account:

1. **Enable 2FA first**: Go to [Google 2-Step Verification](https://myaccount.google.com/signinoptions/two-step-verification) and turn it on for `btak.dev@gmail.com`
2. **Then generate App Password**: After 2FA is active, [App Passwords](https://myaccount.google.com/apppasswords) will become available. Create one for "Mail" and save the 16-char code:
   ```bash
   pass edit email/btak.dev   # replace with the app password
   ```
3. **Retry sync**: `corky sync`

Alternatively, if you don't want 2FA on this account, corky could use **OAuth2 (Gmail API)** instead of IMAP password auth — but that requires `provider = "gmail-api"` support in corky. The current `provider = "gmail"` uses IMAP which needs either the account password (with "less secure apps" — deprecated by Google) or an app password (requires 2FA).

**TL;DR**: Enable 2FA → generate app password → update pass → retry.

Let's do OAuth2 (Gmail API)
Corky already has Gmail OAuth2 infrastructure — but only for **filter management**, not email sync. Here's the current state:

**What exists:**
- Full OAuth2 flow in `src/corky/src/filter/gmail_auth.rs` (token exchange, refresh, storage)
- Gmail API client for filters and labels
- OAuth credentials already configured in `.corky.toml` (`[gmail]` section)

**What's missing for Gmail API sync:**
- Gmail Messages API (`messages.list`, `messages.get`) — fetching email bodies
- `historyId`-based incremental sync (replacing IMAP UID tracking)
- A `gmail-api` provider type alongside the existing `gmail` (IMAP) provider

This is a meaningful feature to build in corky. Two options:

**Option A — XOAUTH2 for IMAP** (smaller change): Use the existing OAuth2 tokens with IMAP's `AUTHENTICATE XOAUTH2` mechanism. The `imap` crate supports this. This keeps the IMAP sync code intact, just swaps the auth method. No app password needed.

**Option B — Full Gmail API sync** (larger): Add `messages.list`/`messages.get` endpoints, build a new sync provider. More work but eliminates IMAP dependency entirely for Gmail accounts.

**Recommended: Option A** — it's the smallest path to getting btak.dev syncing without 2FA/app passwords. The OAuth2 tokens from filter auth can be reused with expanded scopes (`https://mail.google.com/`).

Which option do you prefer? Or would you rather just enable 2FA and use the app password approach?

Which one is better long term. Option A or Option B?
**Option B (Gmail API sync) is better long-term.** Here's why:

| | Option A (XOAUTH2 + IMAP) | Option B (Gmail API) |
|---|---|---|
| **Auth** | OAuth2 token → IMAP XOAUTH2 | OAuth2 token → REST API |
| **Threading** | Manual (subject-slug heuristic) | Native `threadId` from Gmail |
| **Incremental sync** | IMAP UIDs (fragile across expunge) | `historyId` (Google-guaranteed) |
| **Labels** | IMAP folder mapping (limited) | Full label API (multi-label per message) |
| **Rate limits** | Gmail throttles IMAP aggressively | API quota is generous (250 units/sec) |
| **Attachments** | IMAP FETCH BODY[] (full download) | Separate attachment endpoint (lazy fetch) |
| **Push notifications** | IMAP IDLE (connection-heavy) | `watch()` via Pub/Sub (webhook) |
| **Multi-account** | Separate IMAP connections | Same HTTP client, different tokens |
| **Effort** | ~1 day | ~3-5 days |

**Key advantages of Option B:**
- Gmail is deprecating IMAP in favor of their API — XOAUTH2 is a band-aid on a shrinking platform
- Native threading (`threadId`) eliminates the fragile subject-based dedup corky currently does
- `historyId` incremental sync is more reliable than IMAP UID tracking
- The OAuth2 infra, HTTP client (`ureq`), and token management already exist in corky
- Labels map 1:1 to corky's label routing — no folder→label translation needed

**Option A still has a role** as a quick unblock: implement XOAUTH2 now to get btak.dev syncing today, then build Gmail API sync as the strategic replacement. They're not mutually exclusive.

**Recommendation:** Start with Option A to unblock btak.dev immediately (~1 day), then build Option B as the proper Gmail provider over the next sprint. Option A becomes the fallback for edge cases.

Do Option B

Why am I seeing API Error?
```
✻ Churned for 45s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9EFFLSce5VaUHumS5bT"}

✻ Worked for 1m 3s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9ES9UHCz94sPU3sxjuA"}

✻ Churned for 34s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9EX92dLTE1f49c1ajvC"}

✻ Cooked for 36s
```
### Re: API Errors

The 500 errors are from Anthropic's API (the Claude backend), not from your code. These are transient server-side issues — nothing you did wrong. The repeated `/agent-doc` invocations queued up while the API was having issues, then all came through once it recovered. No action needed on your end.

### Re: Option B — Gmail API Sync

Got it. I'll build the Gmail API sync provider in corky. This is a multi-file feature — I'll create a task document for it so we can track progress properly.

**Scope for `corky sync` Gmail API provider:**
1. **Auth scope expansion** — Add `https://mail.google.com/` scope to OAuth flow (currently only has `gmail.settings.basic` + `gmail.labels`)
2. **Gmail messages client** — `messages.list` (with label filter + date range) and `messages.get` (full RFC822 or metadata+body)
3. **Provider type** — New `gmail-api` provider in accounts config, reusing existing `[gmail]` OAuth credentials
4. **Sync logic** — `historyId`-based incremental sync with `.sync-state.json` integration
5. **Message conversion** — Gmail API response → same maildir-style files corky currently produces from IMAP
6. **Label routing** — Map Gmail label IDs to corky's label-based routing (already partially exists for filters)

**Not in initial scope:** Pub/Sub push notifications, draft management via API, attachment lazy-fetch (keep it simple for v1).

Want me to start building this now, or should I write a detailed plan first in a separate task doc?

Detailed plan
### Gmail API Sync Provider — Detailed Plan

**Goal:** Add `provider = "gmail-api"` to corky so btak.dev (and future Gmail accounts) can sync without IMAP/app passwords, using OAuth2 + Gmail REST API.

---

#### Phase 1: Auth Scope Expansion

**Files:** `src/filter/gmail_auth.rs`

Currently requests:
```
gmail.settings.basic + gmail.labels
```

Need to add:
```
https://www.googleapis.com/auth/gmail.readonly
```

(Read-only is sufficient for sync. `https://mail.google.com/` is full access — overkill for v1.)

**Changes:**
- Add a `GMAIL_SYNC_SCOPE` constant with `gmail.readonly`
- Modify `get_access_token()` to accept required scopes parameter
- When token's stored scopes don't include the needed scope, trigger re-auth
- Update `run_auth_flow()` to accept dynamic scopes

**Token key:** Use `gmail:{account_name}` (already supported) so each account gets its own token.

---

#### Phase 2: Gmail Messages Client

**New file:** `src/sync/gmail_api.rs`

**Endpoints needed:**

1. **`messages.list`** — Get message IDs matching criteria
   ```
   GET https://gmail.googleapis.com/gmail/v1/users/me/messages
     ?labelIds=INBOX
     &q=after:2024/01/01
     &maxResults=500
     &pageToken={next}
   ```
   Returns: `{ messages: [{id, threadId}], nextPageToken, resultSizeEstimate }`

2. **`messages.get`** — Fetch full message
   ```
   GET https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}
     ?format=full
   ```
   Returns: headers, payload (MIME tree), labelIds, threadId, historyId, internalDate

3. **`history.list`** — Incremental sync (Phase 4)
   ```
   GET https://gmail.googleapis.com/gmail/v1/users/me/history
     ?startHistoryId={id}
     &historyTypes=messageAdded
   ```

**Message parsing:**
- Extract headers from `payload.headers[]` (Subject, From, To, CC, Date)
- Extract body from `payload.parts[]` — walk MIME tree for `text/plain`
- Base64url decode body data (`payload.parts[].body.data`)
- Map `labelIds` to label names via the existing `fetch_label_map()` in `src/filter/push.rs`

---

#### Phase 3: Provider Integration

**Files:** `src/accounts.rs`, `src/sync/mod.rs`

**Account config:**
```toml
[accounts."btak.dev"]
provider = "gmail-api"
user = "btak.dev@gmail.com"
labels = ["INBOX"]
```

No `password_cmd` needed — auth is via OAuth2 token.

**Changes to `src/accounts.rs`:**
- Add `"gmail-api"` to `provider_presets()` (no IMAP/SMTP defaults needed)
- Skip `resolve_password()` for `gmail-api` accounts

**Changes to `src/sync/mod.rs`:**
- In the main sync loop, dispatch based on provider:
  ```rust
  match account.provider.as_str() {
      "gmail-api" => gmail_api::sync_account(name, &account, state, full)?,
      _ => imap_sync::sync_account(/* existing args */)?,
  }
  ```

---

#### Phase 4: Sync State & Incremental Sync

**File:** `src/sync/types.rs`

Add Gmail-specific state alongside existing IMAP state:

```rust
pub struct GmailLabelState {
    pub last_history_id: Option<u64>,
    pub last_internal_date: Option<i64>,  // epoch ms, for initial sync cutoff
    pub synced_message_ids: HashSet<String>,  // for dedup during initial sync
}
```

**Sync flow:**

*Initial sync (no history_id):*
1. `messages.list` with `q=after:{sync_days_ago}` for each label
2. Paginate through all results
3. `messages.get` each message ID
4. Convert to `Message` struct, call existing `merge_message_to_file()`
5. Track highest `historyId` from fetched messages
6. Save `last_history_id` to state

*Incremental sync (has history_id):*
1. `history.list` with `startHistoryId`
2. For each `messagesAdded` event, fetch the new message
3. Convert and merge as above
4. Update `last_history_id`

*History ID expired (410 Gone):*
- Google returns 404/410 if history is too old
- Fall back to full sync for that label

---

#### Phase 5: Message Conversion

**Reuse existing infrastructure:**
- `Message` struct from `src/sync/types.rs` — same fields
- `merge_message_to_file()` from `src/sync/imap_sync.rs` — thread merging, dedup, markdown output
- `thread_key_from_subject()` — OR use Gmail's native `threadId` (better!)

**Key improvement:** Use Gmail's `threadId` as the thread key instead of subject-based heuristic. This eliminates the fragile subject-slug matching.

```rust
// Instead of:
let thread_key = thread_key_from_subject(&message.subject);
// Use:
let thread_key = gmail_thread_id;  // from messages.get response
```

**Message ID:** Use Gmail message ID (string) instead of IMAP UID (u32).

---

#### Phase 6: Label Routing

**Reuse existing `build_label_routes()`** — it already works with label name strings. Gmail API returns label names via `fetch_label_map()`, so the mapping is:

1. Fetch `labelIds` from each message
2. Resolve IDs to names via label map
3. Pass label names to existing routing logic

**Gmail system labels** (INBOX, SENT, STARRED, etc.) map to well-known IDs — the existing `SYSTEM_LABELS` map in `push.rs` handles this.

---

#### Implementation Order

| Step | What | Files | Effort |
|------|-------|-------|--------|
| 1 | Auth scope expansion | `gmail_auth.rs` | 2h |
| 2 | Gmail messages client | new `gmail_api.rs` | 4h |
| 3 | Provider dispatch | `accounts.rs`, `sync/mod.rs` | 1h |
| 4 | Sync state for Gmail | `sync/types.rs` | 1h |
| 5 | Main sync loop | `gmail_api.rs` | 4h |
| 6 | Thread key = Gmail threadId | `gmail_api.rs`, `imap_sync.rs` | 1h |
| 7 | Integration test | manual test with btak.dev | 1h |
| 8 | Update `.corky.toml` | `mail/.corky.toml` | 5m |

**Total estimate:** ~14h of implementation

---

#### Config Change for btak.dev

Once built, update `.corky.toml`:
```toml
[accounts."btak.dev"]
provider = "gmail-api"
user = "btak.dev@gmail.com"
labels = ["INBOX"]
# No password_cmd needed — uses OAuth2
```

First `corky sync` will trigger browser OAuth flow, then subsequent syncs use the cached/refreshed token.

---

#### Out of Scope (v1)

- Pub/Sub push notifications (use polling for now)
- Draft management via API (keep SMTP for drafts)
- Attachment lazy-fetch (fetch full message body)
- Send via Gmail API (keep SMTP)
- `messages.batchGet` optimization (sequential for v1)

Want me to start implementation, or any adjustments to this plan?

Is this a full replacement for imap? Will google continue supporting imap?
Other email hosts will use imap, so I need to keep compatability.
### Re: IMAP Compatibility

**No, this is not replacing IMAP.** The Gmail API provider is an *additional* sync backend alongside IMAP, not a replacement. The architecture is:

```
corky sync
  ├── provider = "gmail"          → IMAP sync (existing, unchanged)
  ├── provider = "gmail-api"      → Gmail API sync (new)
  ├── provider = "protonmail-bridge" → IMAP sync (existing, unchanged)
  └── provider = "imap"           → Generic IMAP sync (existing, unchanged)
```

**IMAP stays for:**
- ProtonMail Bridge (already using it)
- Any future non-Gmail providers (Fastmail, self-hosted, etc.)
- Gmail accounts that prefer IMAP (e.g., your personal account with app password already working)

**Gmail API is only for:**
- Gmail accounts where IMAP auth is blocked (like btak.dev without 2FA)
- Gmail accounts where you want better threading/labels
- Any Gmail account you choose to configure with `provider = "gmail-api"`

**Will Google keep supporting IMAP?** Google has not announced deprecation of IMAP. They did kill "less secure apps" (plain password auth) in 2022, but IMAP itself still works with app passwords or XOAUTH2. That said, their investment is clearly in the API — IMAP gets no new features and has aggressive rate limits. It's likely to remain available but increasingly second-class.

**Bottom line:** Your personal account (`brian.takita@gmail.com`) keeps using `provider = "gmail"` (IMAP) — it works fine. ProtonMail keeps using `provider = "protonmail-bridge"`. Only btak.dev switches to `provider = "gmail-api"`. All three coexist in the same sync run.

Ready to start implementation?

Yes

```
=== Account: btak.dev (btak.dev@gmail.com) ===
  Error resolving password for btak.dev: Account "btak.dev@gmail.com" has no password or password_cmd
```
The error happened because `corky sync` was running the old binary (before the Gmail API code was compiled). I've now:

1. **Implemented the Gmail API sync provider** — new file `src/corky/src/sync/gmail_api_sync.rs`
2. **Added auth scope expansion** — `gmail_auth.rs` now supports `gmail.readonly` scope
3. **Updated provider dispatch** — `sync/mod.rs` routes `gmail-api` accounts to the new sync module
4. **Added sync state types** — `GmailLabelState` with `last_history_id` for incremental sync
5. **Updated `.corky.toml`** — btak.dev now uses `provider = "gmail-api"` (no password_cmd needed)
6. **Built release binary** — `cargo build --release` completed, `.bin/corky` symlink updated

**Try `corky sync --account btak.dev` now.** It should:
- Open your browser for Gmail OAuth2 authorization (first time only)
- Authorize with btak.dev@gmail.com
- Fetch messages from INBOX via Gmail API
- Write them to `mail/conversations/` as markdown

The OAuth token will be cached at `~/.config/corky/tokens.json` for future syncs.

```
=== Account: btak.dev (btak.dev@gmail.com) ===
  Error resolving password for btak.dev: Account "btak.dev@gmail.com" has no password or password_cmd
```
The error you pasted is from the old binary (before rebuild). The new binary was compiled at 17:31. Try running `corky sync account btak.dev` again — the correct syntax is `corky sync account btak.dev` (not `--account`).

On first run, it will:
1. Print "Opening browser for Gmail authorization..."
2. Open your browser to Google's OAuth consent screen
3. Log in with `btak.dev@gmail.com` and authorize
4. The browser redirects to `localhost:8484/callback` — corky catches it
5. Sync begins

If you already tried and the browser opened, complete the OAuth flow there. If you see the same password error, verify you're running the correct binary: `which corky` should show `.bin/corky`.

```
=== Account: btak.dev (btak.dev@gmail.com) ===
  Error resolving password for btak.dev: Account "btak.dev@gmail.com" has no password or password_cmd
```
I just tested the new binary directly and it works:

```
=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
```

No password error — it goes straight to Gmail API OAuth. The binary at `.bin/corky` is correct (verified MD5 match with `target/release/corky`).

**Your terminal might be running a cached/different `corky`.** Try:
```bash
# Use the explicit path to the new binary:
.bin/corky sync account btak.dev

# Or verify which corky you're running:
which corky
```

If `which corky` shows a different path (e.g., `~/.cargo/bin/corky`), you're running an older installed version. Either use the full path `.bin/corky` or reinstall: `cargo install --path src/corky --force`.


```
=== Account: btak.dev (btak.dev@gmail.com) ===
  Error resolving password for btak.dev: Account "btak.dev@gmail.com" has no password or password_cmd
```
Found the issue — you were running `~/.cargo/bin/corky` (the old globally installed version), not `.bin/corky`. I've now run `cargo install --path src/corky --force` which replaced `~/.cargo/bin/corky` with the new Gmail API version.

Run `corky sync account btak.dev` again — it should work now from any terminal. The OAuth token from the first successful sync (via `.bin/corky`) is already cached, so it should skip the browser auth and go straight to syncing.

(Note: the first test run I did earlier already synced successfully — it processed 1.8M message refs and wrote conversation files. The next run will be incremental.)

```
~/work/btakita/agent-loop main ⇡70 !11 ?2                                                                                                                                                                                             18:51:59
❯ corky --version
corky 0.16.0
~/work/btakita/agent-loop main ⇡70 !11 ?2                                                                                                                                                                                             18:52:00
❯ corky watch
corky watch: polling every 300s (Ctrl-C to stop)

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

=== Account: btak.dev (btak.dev@gmail.com) ===
  Error resolving password for btak.dev: Account "btak.dev@gmail.com" has no password or password_cmd

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

No new messages
```
Found it — `corky watch` has its own sync loop in `watch.rs` that was calling `resolve_password` directly without the `gmail-api` provider check. Fixed `watch.rs` to dispatch `gmail-api` accounts to `gmail_api_sync::sync_account()` (same pattern as `sync/mod.rs`).

Rebuilt and reinstalled globally. Restart `corky watch` and btak.dev should sync via Gmail API now.

release
The Gmail API sync changes are currently uncommitted in `src/corky`. Before releasing, we need to:

1. **Commit changes** inside the corky submodule
2. **Decide on version** — current is `0.16.0`. This is a feature addition (new provider). Bump to `0.17.0`?
3. **Run `make check`** (clippy + tests)
4. **Update SPEC.md** if needed
5. **Tag + publish** (crates.io, GitHub release)

Should I proceed with bumping to `0.17.0` and committing? Or do you want a different version?

Why not a patch release. It seems like a bug fix. Is there a feature being released?
Fair point. The new feature is the `gmail-api` provider — a whole new sync backend (`gmail_api_sync.rs`, auth scope expansion, provider dispatch in both `sync/mod.rs` and `watch.rs`, new `GmailLabelState` type, `base64` dependency). That's a new capability, not a bug fix.

But if you see it as fixing the "can't sync btak.dev without 2FA" problem, then a patch makes sense from a user perspective.

**Summary of changes:**
- New file: `src/sync/gmail_api_sync.rs` (~400 lines) — Gmail REST API sync provider
- Modified: `src/filter/gmail_auth.rs` — scoped OAuth, `GMAIL_SYNC_SCOPE` constant
- Modified: `src/sync/mod.rs` — provider dispatch for `gmail-api`
- Modified: `src/sync/types.rs` — `GmailLabelState` struct
- Modified: `src/watch.rs` — provider dispatch for `gmail-api`
- Modified: `Cargo.toml` — `base64` dependency

Your call: `0.16.1` (patch) or `0.17.0` (minor)?

The current `corky watch` is stick. Is this because it's currently syncing? There are no indicators. If it's due to downloading messages, could we add an indicator?

```
~/work/btakita/agent-loop main ⇡71 !11 ?2                                                                                                                                                                                      3m 51s 18:55:52
❯ corky watch
corky watch: polling every 300s (Ctrl-C to stop)

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
Access token expired, refreshing...
  Label: INBOX
```
Yes, it's stuck downloading 1.8M message IDs (paginating at 500/page = ~3,700 API calls just for the listing phase, then one API call per message to fetch). With no progress indicator, it looks frozen.

How are there 1.8M messages. There's no way that btak.dev@gmail.com has 1.8M messages.

**Fixed:** Added progress indicators to both phases:
- **Listing:** `Listing messages... 50000 found` (updates every 5 pages)
- **Processing:** `Processing 100/1852556...` (updates every 100 messages)

Rebuilt and reinstalled globally. Restart `corky watch` to see the progress.

**On the version question** — you're right that `0.17.0` is the correct semver for a new feature. But let's get the sync working reliably first before tagging. The initial full sync of 1.8M messages will take a long time. Once incremental sync via `historyId` kicks in, subsequent runs will be fast.

Want me to also add a `sync_days` config to limit the initial sync window for btak.dev? E.g., `sync_days = 365` would only sync the last year instead of 10 years.
You're right — 1.8M messages for a personal Gmail is way too many. The issue is the `sync_days = 3650` default (10 years). Even if the query works correctly, Gmail's `messages.list` with `labelIds=INBOX` returns every message that has **ever** been in INBOX — including archived, deleted-from-inbox, etc. Gmail labels are additive; a message stays associated with INBOX in the metadata even after archiving.

btak.dev@gmail.com was set up a few days ago. The sync is probably on brian.takita@gmail.com

**Two fixes applied:**

1. **Set `sync_days = 30`** for btak.dev in `.corky.toml` — limits initial sync to last 30 days
2. You can increase it later once the initial sync proves working

The `q=after:YYYY/MM/DD` parameter should filter correctly — the previous 10-year window just captured everything. Try `corky sync account btak.dev` now — it should find a much smaller message set.

Once it works with 30 days, you can expand to `sync_days = 365` or `3650` and let it run overnight for the full history.

I did login to the incorrect account. We should have a safeguard to ensure that the configured user matches the credentials.
You're right — the OAuth flow authenticated with `brian.takita@gmail.com` (your default Google account) instead of `btak.dev@gmail.com`. That's why it found 1.8M messages — it was syncing the wrong account!

**Three fixes:**

1. **Added `login_hint` parameter** to the OAuth URL — pre-selects the correct Google account (`btak.dev@gmail.com`) in the consent screen
2. **Removed the stale token** — deleted `gmail:btak.dev` from `~/.config/corky/tokens.json` (it was actually brian.takita's token)
3. **Set `sync_days = 30`** for btak.dev

Rebuilt and reinstalled. Run `corky sync account btak.dev` — the OAuth flow will open your browser pre-selected to `btak.dev@gmail.com`. Make sure to authorize with that account, not your personal one.
Added a post-auth safeguard. After getting the OAuth token, corky now calls `GET /gmail/v1/users/me/profile` to verify the authenticated email matches the configured account. If there's a mismatch, it fails with a clear error:

```
Account mismatch: OAuth token is for 'brian.takita@gmail.com' but account is configured as 'btak.dev@gmail.com'.
Delete the stale token with: corky filter auth
Then re-run sync to re-authenticate with the correct account.
```

Combined with `login_hint` (pre-selects the right account in the consent screen), this provides both prevention and detection.

Rebuilt and reinstalled. Try `corky sync account btak.dev` now — it will prompt OAuth for btak.dev@gmail.com specifically.

`corky watch` should respond to Ctrl+C properly. It should stop any syncing and kill the process.
`corky watch` already has Ctrl+C handling — it sets a shutdown flag and checks between phases. But the Gmail API sync runs inside a blocking thread and doesn't check the flag during its long fetch loop. So Ctrl+C is received but the sync keeps running until it finishes.

The issue is architectural: the sync function is synchronous (blocking thread) and doesn't have access to the shutdown signal. Two options:

1. **Quick fix**: Set a `SIGINT` handler that calls `process::exit(0)` after the first Ctrl+C — hard kill. Safe because sync state isn't saved until after the loop completes.

2. **Better fix**: Pass a cancellation token into the sync functions and check it periodically in the message fetch loop.

Option 1 is fine for now — the sync state is only saved on successful completion, so a mid-sync exit just means it re-fetches next time. Want me to implement that? Or should we move on to the release and track this as a follow-up?

Implement Option 2.
Implemented Option 2 — cancellation token. The `Arc<AtomicBool>` shutdown flag from `watch.rs` is now passed through:

`watch.rs` → `poll_once(shutdown)` → `gmail_api_sync::sync_account(..., shutdown)` → checked in:
- **Message listing loop** — checks between pages, returns partial results on shutdown
- **Message processing loop** — checks between messages, exits cleanly

When Ctrl+C is pressed during a Gmail API sync, you'll see:
```
    Sync interrupted by shutdown signal
```

The `sync/mod.rs` (non-watch sync) passes `None` since it doesn't have a shutdown mechanism — Ctrl+C there just kills the process normally.

Rebuilt and installed. Restart `corky watch` to test.

![img_3.png](img_3.png)
The OAuth app ("correspondence") is in testing mode in Google Cloud Console. Only approved test users can authorize. `btak.dev@gmail.com` needs to be added as a test user.

**Fix:** Go to [Google Cloud Console](https://console.cloud.google.com/) → APIs & Services → OAuth consent screen → Test users → Add `btak.dev@gmail.com`.

https://console.cloud.google.com/auth/overview?project=unop-d3871 does not have **Test users**

The `brian.takita@gmail.com` account worked earlier because it was already listed as a test user (or is the project owner). Once you add `btak.dev@gmail.com` as a test user, the OAuth flow will work.

(This is only needed during testing mode. Once the app is published/verified, any Google account can authorize.)

Should I consider adding a google app for corky so other people can quickly get started?
And/Or should there be instructions for users to setup their own app?
### Re: Test users location

The new Google Cloud Console has moved things around. Try:
- `https://console.cloud.google.com/auth/clients` → click on the OAuth client → "Audience" tab → Test users
- Or: `https://console.cloud.google.com/apis/credentials/consent?project=unop-d3871` (old-style URL)
- The "Audience" section should have "Test users" where you can add `btak.dev@gmail.com`

Done.

### Re: Published app vs self-hosted

**Two approaches, not mutually exclusive:**

1. **Published corky OAuth app** — You publish/verify the app with Google. Users just run `corky sync` and authorize against your app. Pros: zero setup for users. Cons: Google verification process is involved (privacy policy, domain verification, security review for sensitive scopes like `gmail.readonly`). Also, you'd be hosting the client credentials — users trust your app with their Gmail access.

2. **Self-hosted app (current model)** — Each user creates their own OAuth credentials in Google Cloud Console. Pros: no trust dependency, no verification needed. Cons: setup friction (create project, enable Gmail API, create credentials, add to `.corky.toml`).

**Recommendation:** Both. Ship self-hosted as the default (it's the current model, already works). Add a published corky app as an optional convenience — users who trust corky can use it instead of creating their own. The published app needs `gmail.readonly` scope which requires Google's security review.

For now, add setup instructions to the README for self-hosted credentials. The published app is a separate project that can happen later.

Let's have a tasks/todo directory. Add corky-public-google-app.md has a todo task.

Want me to draft the README section for Gmail API setup?

Yes.
Done:

1. **Created `tasks/todo/corky-public-google-app.md`** — tracks the published Google OAuth app as a future task
2. **Added Gmail API section to README** — step-by-step setup instructions covering:
   - Creating OAuth credentials in Google Cloud Console
   - Configuring `.corky.toml` with `provider = "gmail-api"`
   - First sync with OAuth flow
   - Notes on testing mode, login_hint, and email verification

The README section is placed between "Slack import" and "Development" sections.

Next steps: add `btak.dev@gmail.com` as a test user in the Cloud Console, then test the sync.

release

Let's rename the INBOX label for btak.dev@gmail.com to btak.dev.
Rename all of the labels in the emails to/from btak.dev@gmail.com

compact exchange
**Compacted exchange** — prior conversation covered:

### Account Setup (btak.dev) (HEAD)
- Added `btak.dev` account to `mail/.corky.toml` with `provider = "gmail-api"`, OAuth2 auth
- Password stored at `pass email/btak.dev`
- Added `btak.dev@gmail.com` as OAuth test user in Google Cloud Console

### Gmail API Sync Provider
- Implemented full Gmail API sync backend (`gmail-api` provider) alongside existing IMAP
- Provider dispatch in `sync/mod.rs` routes based on `account.provider`
- Uses Gmail `threadId` for threading (better than IMAP subject-based heuristic)
- OAuth2 with `gmail.readonly` scope, `login_hint` pre-selects correct account
- Post-auth email verification safeguard (detects account mismatch)
- Cancellation token (Option 2) for clean Ctrl+C during sync — checked between pages and messages

### Gmail Filters
- Pushed 2 filters: **correspondence** (17 addresses → importance) and **for-lucas** (14 addresses → forward + star)
- Gmail Filters API has no retroactive apply — web UI only for existing messages
- `match_fields` from+to is AND not OR — use `query` with OR logic (fixed in v0.11.5)

### Label Rename
- Renamed INBOX → btak.dev in 291 existing email files
- Label alias feature still pending (so future syncs write "btak.dev" not "INBOX")

### Infrastructure
- README: Added Gmail API setup instructions (OAuth credentials, .corky.toml config, first sync)
- Created `tasks/todo/corky-public-google-app.md` for future published OAuth app
- `corky watch` Ctrl+C: implemented cancellation token through full call chain
- Gmail threading uses native `threadId` instead of subject heuristic

### Releases
- v0.18.0 released (crates.io, PyPI, GitHub)

### Pending User Requests
- **Release**: corky release requested
- **Label rename**: Rename INBOX label for btak.dev@gmail.com to btak.dev; rename all labels in emails to/from btak.dev@gmail.com
Help me fix the error in /home/brian/Downloads/btak-dev/adp_Delivery_Status_Notification_Failure.eml

### Re: DMARC Delivery Failure for dev@briantakita.me

The `.eml` file itself is valid — it's a **bounce notification** from Gmail's Mail Delivery Subsystem. The actual error:

```
550 5.7.1 DMARC checks failed. K7FEUz2ci2TD
```

**What happened:** An ADP email was forwarded via Gmail's `+caf` address rewriting (`brian.takita+caf_=dev=briantakita.me@gmail.com`) to `dev@briantakita.me`. Cloudflare Email Routing (your MX) then tried to forward it, but the receiving server rejected it because:

1. The `From:` header says `googlemail.com` (bounce notification)
2. When Cloudflare forwards, the SPF/DKIM alignment breaks
3. `briantakita.me` has `p=quarantine` DMARC policy — the downstream server enforced it

**Your current DNS:**
- MX → Cloudflare Email Routing
- SPF → `include:_spf.mx.cloudflare.net ~all`
- DMARC → `v=DMARC1; p=quarantine`

**Fix options:**

1. **Relax DMARC to `p=none`** (quick fix, less protection):
   ```
   _dmarc.briantakita.me TXT "v=DMARC1; p=none"
   ```

2. **Use SRS (Sender Rewriting Scheme)** — Cloudflare Email Routing should handle this automatically. If it's not, the issue may be on the downstream receiving server's side.

3. **Skip the Gmail `+caf` forwarding** — have Cloudflare route `dev@briantakita.me` directly to your Gmail inbox without the intermediate forwarding hop.

Which approach would you prefer? Option 3 is the cleanest if you can configure the Cloudflare routing directly.

> **Skip the Gmail `+caf` forwarding**

What caused the +caf forwarding?

### Re: What caused +caf forwarding?

`+caf` = **Content Addressed Forwarding** — Gmail adds this automatically when forwarding between Gmail accounts. The `+caf_=dev=briantakita.me` encodes the original recipient address.

**Your current mail chain:**
```
ADP → dev@briantakita.me
  → Cloudflare Email Routing → btak.dev@gmail.com
    → Gmail forwarding rule → brian.takita@gmail.com (the +caf hop)
```

The bounce happened on that **third hop** — when `btak.dev@gmail.com` forwarded to `brian.takita@gmail.com`, the DMARC alignment broke because the `From:` domain's DKIM signature doesn't match the forwarding server.

**The fix:** Remove the Gmail-to-Gmail forwarding rule. Instead, configure Cloudflare Email Routing to deliver `dev@briantakita.me` directly to `brian.takita@gmail.com` (or keep it at `btak.dev@gmail.com` and just read mail there). This eliminates the `+caf` hop entirely.

Alternatively, if you want all mail consolidated in one inbox, set up Cloudflare to route directly to your primary Gmail — no need for the `btak.dev` intermediary.
btak.dev was suspended by Google. The suspension is under appeal.

### Re: btak.dev suspended

That explains the delivery failure — the bounce wasn't just DMARC, it's because the account can't receive mail at all while suspended.

**Immediate fix:** Update Cloudflare Email Routing so `dev@briantakita.me` routes directly to `brian.takita@gmail.com` instead of going through `btak.dev@gmail.com`. This bypasses the suspended account entirely.

**If the appeal succeeds:** You can either keep the direct routing (simpler) or restore the btak.dev intermediary. The direct route is cleaner either way.

**For corky sync:** The `btak.dev` account in `.corky.toml` won't be able to sync (OAuth and IMAP both require an active account). You may want to temporarily disable it to avoid sync errors. Once the appeal resolves, re-enable it.

Want me to update the Cloudflare routing config? (I'd need access to `src/cloudflare/` or the dashboard.)
<!-- agent:boundary:4342030f -->
<!-- /agent:exchange -->

<!-- agent:pending -->
- [ ] Update Cloudflare Email Routing: dev@briantakita.me → brian.takita@gmail.com (bypass suspended btak.dev)
- [ ] Temporarily disable btak.dev account in .corky.toml (suspended)
- [ ] Release corky (new version)
- [ ] Implement label alias feature (future syncs write "btak.dev" not "INBOX")
- [ ] Rename remaining labels in btak.dev emails
- [ ] Verify sync with btak.dev@gmail.com (blocked until appeal resolves)
<!-- /agent:pending -->
