---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
links:
- ./judith-krug.md
- ./robert-waldman.md
- ../personal/robert-ward.md
- ./bob-remeika.md
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: start
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchahnge

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260617-204552.md`*

Compacted content:
- Archived 10 response topic(s): Ensure rwaldman100+admin@gmail.com has an admin account; Ensure all added admins are sent an email about their account; Send the account email to the operator; 7 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Catalog all portal messaging components. Make sure all portal ...

### Re: Video-corpus task + deploy do not apply to EFS this cycle — glm-5.2

> **Queue prompt:**
>
> Go through the video corpus and add relevant videos with high-confidence matches to the product pages, product category pages, and learn pages.


Blocked on the video-corpus item; nothing to ship for deploy. I walked the full EFS codebase (`src/equityfundingsource-dev/`) and the prerequisites are absent:

- **No video corpus.** Zero video files (`.mp4`/`.webm`/`.mov`), no YouTube/Vimeo embeds, no video manifest anywhere. The only `video` string is a doc-preview comment at `lib/borrower/loans/index.ts:588` and demo-call small talk.
- **No product / product-category / learn pages.** Live EFS routes are only `/`, `/about`, `/apply`, `/blog`, `/contact`, `/funded-deals`. No product taxonomy, no learn center. The loan "programs" (Purchase / Bridge / Construction / Commercial) are inline home-page labels in `lib/public_content/repo.ts`, not standalone pages; the blog is the only educational surface.

This task reads like it belongs to a different property (a site with a video library + product/learn structure). Striking it rather than fabricating a corpus or inventing pages. **If an external video corpus exists (Drive/Dropbox/etc.), point me at it and confirm product/learn pages are actually planned for EFS** — otherwise it's out of scope for a private-lending portal. On deploy: no code/spec/test changes were made (the video task was blocked, not implemented), so `make deploy` would only re-ship current state; re-queue deploy after real changes land.

### Re: deploy — shipped to production — glm-5.2

> **Queue prompt:**
>
> deploy


Deployed the EFS Worker. Pre-deploy checks all green: on `main`, clean tree, **812 tests pass / 0 fail**, build + `build:worker` OK, no pending D1 migrations (production current through 0084).

`make deploy` → build → no migrations to apply → Worker uploaded + activated.

- **Version ID:** `7a9b9ada-e110-4c6d-aa3e-2d8d9243e194`
- **URL:** https://equityfundingsource.brian-takita.workers.dev (cron `0 * * * *`)
- Ships the committed-but-not-yet-deployed features: admin provisioning + `POST /api/admin/bootstrap-admins`, Ctrl/Cmd+Enter message-send across portal message boxes, contact-form update, auth CLI fixes.
- Smoke tests: `/` 200, `/funded-deals` 200, `/login` 200.

Queue drained (video-corpus struck as out-of-scope last cycle; deploy struck this cycle).

### Re: Remove "Urgent notifications" label — done, pushed — glm-5.2

> **Queue prompt:**
>
> Remove the "Urgent notifications" label in Available Deals dialog/page. Keep the $... remaining to fund.


Removed the "Urgent notifications" heading from `DealUrgentNotifications` (admin deal detail / Available Deals dialog) in `lib/ui--browser--efs/portal/admin/deal_detail.tsx`. The notification chips — package follow-up, pending commitments, open doc requests, and **"$... remaining to fund"** — all remain; the "Show all" toggle now renders its own row only when there are 4+ items.

Verification: `make check` green — **812 tests pass / 0 fail**, build + `build:worker` OK.

Commits + push (preset `#spec-test-commit-push`):
- EFS `81e5fe4` `fix(admin): drop "Urgent notifications" label from deal detail` → pushed `b1f2f89..81e5fe4`
- root `5184e58ef` submodule pointer bump (root push follows this closeout)

Deploy remains queued as the next item.

### Re: deploy — label fix shipped — glm-5.2

Deployed the EFS Worker with the "Urgent notifications" label removal. Pre-deploy green: clean tree, no pending D1 migrations, `make check` already passed (**812 tests**). `make deploy` → build → no migrations → Worker activated.

- **Version ID:** `fc6e89db-b8be-492f-9ce6-912b0aadee68`
- Ships EFS `81e5fe4` (drop "Urgent notifications" label; "$... remaining to fund" kept)
- Smoke: `/` `/funded-deals` `/login` all 200

Queue drained.

### Re: Lender-select error is a missing TMO account, not the email rename — glm-5.2

> **Queue prompt:**
>
> Selecting the lender in …/dialog_deal_id=7a0c1463-…&deal_tab=interests had an error. Is this due to the email change o…
>
> ```
> Select lender failed
> selected lender needs a TMO lender account before TMO sync
> ```

**No — unrelated to the email rename.** The guard that fires is `admin_deal_interests_by_id_select_PUT_` at `lib/admin/investor_management/index.ts:1137-1138`:

```ts
const lender_account = existing.investor_lender_account?.trim() ?? ''
if (!lender_account) return c.json({ error: 'selected lender needs a TMO lender account before TMO sync' }, 409)
```

`investor_lender_account` comes from `users.lender_account` via the JOIN at line 351. The rename-accounts CLI (`lib/auth/rename_accounts.ts:63`) only writes `email` + `updated_at`:

```sql
UPDATE users SET email = ?, updated_at = unixepoch() WHERE email = ?;
```

It does **not** touch `lender_account`, so the rename preserved whatever was already there — null, in this case. The renamed test account (`test2@investor.com` → `btak.dev+lender2@gmail.com`) never had a TMO lender account number assigned.

**Root cause:** the selected lender's `users.lender_account` is null/empty. That field is set only when an admin completes a lender request via `PUT /api/admin/lender-requests/:id/done` (`admin_lender_requests_by_id_done_PUT_`, line 1628), which writes the TMO account number to both `lender_requests` and `users.lender_account`.

**Fix (operational — no code change needed):**
- Complete a lender request for this investor with the real TMO lender account number via the admin UI, **or**
- Set it directly in D1 if you know the account number:
  ```sql
  UPDATE users SET lender_account = '<TMO-LENDER-ACCT>', updated_at = unixepoch()
  WHERE email = 'btak.dev+lender2@gmail.com';
  ```

The guard is correct as designed — selecting a lender enqueues a gated TMO writeback (`tmo_selected_lender_outbox_enqueue_`) that needs a real account to sync to. No spec/test/code changes warranted, so `#spec-test-commit-push` doesn't apply this cycle. If you want a UI affordance to set `lender_account` on an existing investor without a lender-request round-trip, I can add that as a backlog item.

### Re: Login account vs TMO lender_account — glm-5.2

> **Queue prompt:**
>
> The lender does have an account. I was just logged in. Why did we get the error?

Those are two different fields. The lender has a **login account** (`users.email` + `users.password_hash`, so they can authenticate), but the guard checks `users.lender_account` — the **TMO lender account number** from The Mortgage Office. That's a separate column that is empty.

I confirmed there is exactly **one** code path that writes `users.lender_account`: `user__set_lender_account_` (`lib/auth/repo.ts:171`), called only from `admin_lender_requests_by_id_done_PUT_` (`lib/admin/investor_management/index.ts:1664`) — i.e. when an admin completes a lender request and types in the TMO account number. The investor signup/auth flow creates the login row but never sets `lender_account`. The same guard also fires on the investor side at `lib/investor/deal_marketplace/index.ts:435` ("A registered lender account is required before funding").

**Verify directly** — run this against production D1 for the deal you hit:

```sql
SELECT u.email, u.lender_account, u.role,
       (SELECT lr.status FROM lender_requests lr WHERE lr.user_id = u.id ORDER BY lr.created_at DESC LIMIT 1) AS latest_request_status
FROM investor_deal_interests i
JOIN users u ON u.id = i.user_id
JOIN investment_deals d ON d.id = i.deal_id
WHERE d.id = '7a0c1463-27bb-4d7c-82f7-2281b3c008d3';
```

You'll almost certainly see `lender_account = NULL` and `latest_request_status` either NULL (no lender request was ever submitted) or not `done`.

**Fix:** complete a lender request for this investor via `PUT /api/admin/lender-requests/:id/done` with their real TMO account number (or set `users.lender_account` directly in D1). The select-lender action will then pass.

No code change needed — the guard is correct; the `lender_account` column just isn't populated yet.

### Re: Login ≠ lender_account; accreditation is not checked here — glm-5.2

> **Queue prompt:**
>
> The lender logged into the lender portal so they should have a lender account. Is this due to the lender not being accredited?

No — neither logging in nor accreditation controls this error. The naming is confusing, so here's the exact distinction:

| What you're thinking | Actual code field | Set by |
|---|---|---|
| Lender can log into the portal | `users.email` + `users.password_hash` + `users.role='investor'` | signup/auth flow |
| Accreditation status | `users.accreditation_status` | admin accreditation review (`POST /api/admin/investor-accreditation/:id/decision`) |
| **TMO lender account number** | **`users.lender_account`** | **admin lender-request completion (`PUT /api/admin/lender-requests/:id/done`)** — the only write path |

The guard that fires checks **only** `users.lender_account` — the TMO account number from The Mortgage Office:

```ts
// lib/admin/investor_management/index.ts:1137-1138
const lender_account = existing.investor_lender_account?.trim() ?? ''
if (!lender_account) return c.json({ error: 'selected lender needs a TMO lender account before TMO sync' }, 409)
```

It does **not** check accreditation. It does **not** check whether the lender can log in. It checks one thing: is there a TMO account number string in `users.lender_account` for this investor?

That number is **never** set by signup, login, or accreditation. It's an external identifier from The Mortgage Office system that an admin manually enters when completing a lender request. A lender can sign up, log in, be fully accredited, and express interest in deals — and still have `lender_account = NULL` if no admin completed their lender request with the TMO number.

**To confirm and fix in one step**, run this against production D1 (`npx wrangler d1 execute ... --command "..."`):

```sql
-- Check current state
SELECT email, lender_account, accreditation_status FROM users WHERE email = '<lender-email>';

-- If lender_account is NULL and you know the TMO number, set it:
-- UPDATE users SET lender_account = '<TMO-NUMBER>', updated_at = unixepoch() WHERE email = '<lender-email>';
```

This is a data gap, not a code bug. The one thing that would make this less confusing is renaming the error message or the column — say the word and I'll add a backlog item for clearer messaging (e.g., "lender has no TMO account number on file — complete their lender request first").

### Re: TMO lender-account warning surfaced + deployed — glm-5.2

> **Queue prompt:**
>
> In the InterestedLenderCard where the TMO account is not linked, there should be a button under Show/Hide workflow to link the account to TMO. deploy


> **Queue prompt:**
>
> The admin should see the current lender account as not being synced with TMO. Since there is no lender id on the account, the admin should be prompted to sync the lender with TMO in the admin Available deal dialog/page and the admin Lender dialog/page. The error message in the admin Available deal should be rendered above the "Select lender" button and be present in the toast message. deploy

Shipped proactive TMO-account-missing warnings across the admin UI so the admin sees the problem *before* clicking "Select lender", plus a clearer toast error. Deployed.

**Changes (EFS `13ae196`):**

1. **Available Deals dialog — deal detail** (`lib/ui--browser--efs/portal/admin/deal_detail.tsx`):
   - Amber warning banner above the "Select lender" button: "No TMO lender account on file — complete the lender request before selecting." (shown only when `investor.lender_account` is empty and the lender isn't already selected)
   - "TMO account not linked" badge on each `InterestedLenderCard` header
   - Lender account field reads "Not linked" (amber) instead of "—" when empty

2. **Investor pipeline modal** (`lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`):
   - Lender account `ModalRow` now shows "Not linked" with amber accent instead of "—" when no account and no pending request

3. **API error message** (`lib/admin/investor_management/index.ts:1138`):
   - Old: `"selected lender needs a TMO lender account before TMO sync"`
   - New: `"No TMO lender account on file for this lender. Complete the lender request first to assign a TMO account number."` — surfaces in the toast on 409

4. **Test** (`tests/admin-investor-pipeline.test.ts:531`): updated assertion to match new error message

**Verification:** `make check` — **812 tests pass / 0 fail**, build + build:worker OK. No pending D1 migrations.

**Deploy:**
- EFS `13ae196` pushed `81e5fe4..13ae196`
- Root `8b690b3ea` submodule pointer bump pushed
- Worker deployed: **Version `8c43e5b7-370b-4fe2-b6c8-5d0071dde8ac`** at https://equityfundingsource.brian-takita.workers.dev
- Smoke: `/`, `/funded-deals`, `/login` all 200
- Worker deployed: **Version `fb14e2e4-6561-4654-8375-a6b1ab41f8e3`**
- Root `2ec612576` submodule pointer bump pushed
- EFS `80a7144` pushed `13ae196..80a7144`
- On failure: toast shows the error
- On success: card updates in place (badge + field flip to the account number), toast confirms
- Enter submits, Escape cancels
- Clicking reveals an inline monospace input ("TMO lender account #") with Link / Cancel buttons
- "Link TMO account" button appears under the Workflow toggle when the lender has no TMO account
- Registered in routes.ts alongside the other user-management PUTs
- Admin-guarded, fires `notify_admin_event_`
- Accepts `{ lender_account }`, validates via `clean_account_`, calls `user__set_lender_account_`

Queue drained.

### Re: Inline Link TMO account button + deployed — glm-5.2

> **Queue prompt:**
>
> In the InterestedLenderCard where the TMO account is not linked, there should be a button under Show/Hide workflow to link the account to TMO. deploy

Shipped a "Link TMO account" button directly on each `InterestedLenderCard` (visible when `lender_account` is empty) with an inline input that calls a new dedicated endpoint. Deployed.

**New endpoint** — `PUT /api/admin/users/:id/lender-account` (`lib/admin/user_management/index.ts`):
- Accepts `{ lender_account }`, validates via `clean_account_`, calls `user__set_lender_account_`
- Admin-guarded, fires `notify_admin_event_`
- Registered in routes.ts alongside the other user-management PUTs

**UI** — `InterestedLenderCard` (`deal_detail.tsx`):
- "Link TMO account" button appears under the Workflow toggle when the lender has no TMO account
- Clicking reveals an inline monospace input ("TMO lender account #") with Link / Cancel buttons
- Enter submits, Escape cancels
- On success: card updates in place (badge + field flip to the account number), toast confirms
- On failure: toast shows the error

**Verification:** `make check` — **813 tests pass / 0 fail** (+1 new test for the endpoint), build + build:worker OK.

**Deploy:**
- EFS `80a7144` pushed `13ae196..80a7144`
- Root `2ec612576` submodule pointer bump pushed
- Worker deployed: **Version `fb14e2e4-6561-4654-8375-a6b1ab41f8e3`**
- Smoke: `/`, `/funded-deals`, `/login` all 200

Queue drained.
<!-- agent:boundary:e1987bd9 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- Can we have the "Link TMO Account" button perform a TMO sync of the lender account? 
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#efs-cutover-sso-plan] Workers.dev to custom-domain/SSO closeout: write and execute a cutover checklist for equityfundingsource.com/www routing, /login/access-callback, rollback, and Google Workspace SSO after #m6ch credentials arrive. Advances #next-steps.
- [/] [#zwws-vestings-local] [recommended] Investor dashboard phase 4 local half: capture/display vestings and special instructions in EFS D1/UI with tmo_cache_outbox enqueue; leave TMO push gated on #dzmm/#7a4d. Advances #zwws.
- [/] [#67v2-auto-advance] Admin pipeline auto-advance: define stage requirement checks from the TMO-aligned Kanban, run them against D1/tmo_cache, and enqueue audited status updates; keep replay gated by #dzmm. Advances #67v2.
- [/] [#jnzs-approval-followup] [recommended] Send/track funded-deals approval packet: attach current screenshots/copy/lien-position language plus the full_application ESIGN/UETA disclosure excerpt, record Judith/EFS reviewer decision, and convert requested edits into implementation backlog. Advances #jnzs.
- [/] [#xcvg-prod-email] [recommended] Borrower secure-message email production pass: after #f4rq/#m6ch/#p40t or #z3kx path-of-record is decided, enable borrower/admin outbound notifications, verify inbound reply threading, and add production env/runbook checks. Advances #xcvg.
- [/] [#dzmm-writeback-replay] EFS->TMO writeback replay: once #7a4d field map and #k2xg support answers arrive, replay tmo_cache_outbox with idempotency keys, conflict logging, retry/dead-letter state, and tests across broker/admin/investor writebacks. Advances #dzmm/#37ah/#67v2/#zwws.
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#ghl-stage-bridge] [recommended] Implement an event-driven GHL stage bridge from EFS Deal Pipeline state to HighLevel contacts/opportunities/workflows for Processing, Underwriting, Terms negotiation, and Closing while keeping EFS/D1 canonical for loan, lender-selection, borrower portal, document, audit, and funding state.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG -> transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6-12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#kxsx] Form submission pipeline
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#7fx4] OCR ingestion schema/state machine: add provider-neutral document_processing tables for documents, versions, OCR runs, pages/chunks, extracted fields, entity mentions/edges, review decisions, provider cost, and retry/error state.
- [ ] [#d1h2] OCR provider adapter: process private R2 documents through pluggable parsers starting with Ragie Parse or Workers AI/Claude where appropriate; keep original R2 object as source of truth and persist source page/bounding-box evidence.
- [ ] [#f5kq] OCR admin review workflow: show extracted fields with source evidence, confidence, and diffable approval/rejection before any TMO borrower, loan, lender, or loan-application writeback.
- [ ] [#ntcr] Search/index spike: keep D1/R2 canonical and evaluate Ragie index plus Cloudflare Vectorize/SQL edge tables before adopting Neo4j, FalkorDB, Convex, or another separate graph database.
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
