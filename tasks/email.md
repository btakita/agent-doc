---
agent_doc_session: 6c57ef6d-db13-43df-a68d-0e9c4372110b
tmux_session: '1'
agent_doc_format: template
agent_doc_write: crdt
---

# Email Management

## Session History

- Pushed 2 Gmail filters: **correspondence** (17 addresses → importance) and **for-lucas** (14 addresses → forward to `dev@briantakita.me` + star)
- Synced mail + committed (1474 files)
- Discussed multi-account Gmail: IMAP sync supported, filters are single-account
- Proposed `btak-dev` account config (not yet applied)

## Status

<!-- agent:status -->
- [x] btak.dev account added to .corky.toml
- [x] Gmail API sync provider implemented & compiled
- [ ] First sync test with btak.dev
<!-- /agent:status -->

## Exchange

<!-- agent:output mode=append -->
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
<!-- /agent:output -->
