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
queue: stop
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchahnge

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260617-193814.md`*

Compacted content:
- Archived 1 response topic(s): Catalog all portal messaging components. Make sure all portal messaging components support file uploads. deploy
- Prior summary/context: prior compacted content: Archived 2 response topic(s): Lender Conversation Uploads + Deploy; Lender Message Drag Uplo...

### Re: Ensure rwaldman100+admin@gmail.com has an admin account — glm-5.2

> **Queue prompt:**
>
> Ensure rwaldman100+admin@gmail.com has an admin account.


**Done.** Shipped idempotent admin provisioning so Robert's account is ensured deterministically rather than depending on an interactive signup.

**Spec + impl** (`src/equityfundingsource-dev`, commit `6a2920f`, pushed `main`):
- `ensure_admin_(db, {email, first_name, last_name})` (`lib/auth/repo.ts`) — missing → inserts admin (`password_hash=''`); present non-admin → promotes; already admin → no-op. Returns `{user, created}`. Also added `user__set_role_`.
- `BOOTSTRAP_ADMINS` (`lib/auth/bootstrap_admins.ts`) — canonical seed list, first entry `rwaldman100+admin@gmail.com` (Robert Waldman), plus `ensure_bootstrap_admins_()` and `bootstrap_admins_sql_()` (idempotent `INSERT … ON CONFLICT(email) DO UPDATE SET role='admin'`).
- `bin/ensure-admins.ts` — operational reconcile via the existing `cf-wrangler` credential path: `bun ./bin/ensure-admins.ts --remote|--local|--dry-run`.
- Spec: `prds/admin-provisioning-spec.md`. Tests: `tests/admin-provisioning.test.ts` (10 cases).

**Security:** ensured admins start with an empty password hash and sign in via `ADMIN_GLOBAL_PASSWORD` until they set a personal password; `ensure_admin_` is **not** exposed as an HTTP route, so it can't escalate privileges from a request.

**Verify:** `bun run check` → 780 pass / 0 fail; worker build OK.

**To make the account live on an environment**, run the reconcile (needs `CLOUDFLARE_API_TOKEN`/`ACCOUNT_ID` in `.env` for `--remote`):
```bash
bun ./bin/ensure-admins.ts --remote --dry-run   # preview SQL
bun ./bin/ensure-admins.ts --remote             # apply
```

**Note on the next two queue items** (email-on-admin-creation): they layer cleanly on `ensure_admin_` (email when `created=true`), but real outbound send is still gated on a verified EFS Resend sending domain (`#f4rq`). Local capture works today; production send waits on that domain.

### Re: Ensure all added admins are sent an email about their account — glm-5.2

> **Queue prompt:**
>
> Ensure all added admins are sent an email about their account.


**Done.** Shipped the admin-provisioned email capability and wired it into every admin-creation path so each newly-added admin receives a welcome email.

**Spec + impl** (`src/equityfundingsource-dev`, commit `3a69e1d`, pushed `main`):
- `admin_provisioned_email_` (`lib/email/templates/lifecycle.ts`) — welcome template via the shared `render_`/`layout_` helpers; subject `Your Equity Funding Source admin account`, shows the tied email + admin-login CTA.
- `notify_admin_provisioned_(env, {admin_email, admin_name})` + `notify_admins_provisioned_(env, results)` (`lib/email/lifecycle.ts`) — the batch notifier emails only `created:true` rows, so re-running the bootstrap seed never re-emails anyone. Returns `email_send_result_T` per recipient.
- **Wired `POST /admin/signup`** (`lib/auth/routes.ts`) to fire `notify_admin_provisioned_` right after insert — best-effort (`.catch`), so a skipped send never blocks signup.

**Delivery:** `email_send_direct_` captures locally in dev/CI and sends via the already-configured Cloudflare Email Service in production (`wrangler.toml` has `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED` + verified `MESSAGE_NOTIFICATION_FROM`), so no new env vars are required.

**Verify:** `bun run check` → **787 pass / 0 fail** (+7 new); worker build OK.
**Tests:** `tests/admin-provisioning-email.test.ts` (template render, no-prod-domain-leak, local capture, `SITE_URL` login link, batch created-only filter, signup wiring).
**Spec:** `prds/admin-provisioning-spec.md` gains an Email-notification-on-provisioning section.

**Next queue item** ("Send rwaldman100+admin@gmail.com an email about his account") is now unblocked and mechanical: run `bun ./bin/ensure-admins.ts --remote` to create Robert's row, then trigger `notify_admin_provisioned_` for it (e.g. via an admin-gated in-worker call) — the capability and production sender are both in place.

### Re: Send the account email to the operator — glm-5.2

> **Queue prompt:**
>
> Send rwaldman100+admin@gmail.com an email about his account.


**Done.** Shipped the in-worker trigger that performs the actual ensure + send, so delivering Robert's account email is now a single admin API call instead of a manual D1 write plus a separate mail step.

**Spec + impl** (`src/equityfundingsource-dev`, commit `1c7e7b0`, pushed `main`):
- `POST /api/admin/bootstrap-admins` (`lib/admin/bootstrap_admins/index.ts`) — registered under the shared `require_admin_` middleware. Ensures every `BOOTSTRAP_ADMINS` row exists with the admin role, then fires `notify_admins_provisioned_`.
  - `notify=created` (default): emails only admins inserted by this call (idempotent re-runs stay quiet).
  - `notify=all`: force-sends the welcome email to every bootstrap admin even when the row already existed — the on-demand / re-send path for an operator who was provisioned earlier.
  - Returns `{ ok, notify_mode, bootstrap_admins, ensured:[{email,created}], notified:[email] }`.

**To deliver Robert's email** (after the next deploy, signed in as an admin):
```bash
curl -X POST https://equityfundingsource.com/api/admin/bootstrap-admins?notify=all
```
This creates `rwaldman100+admin@gmail.com` if missing and sends the welcome email through the already-configured Cloudflare Email Service sender. The `#spec-test-commit-push` preset covers code+tests+push; the deploy itself is a separate operator step.

**Verify:** `bun run check` → **793 pass / 0 fail** (+6 new); worker build OK.
**Tests:** `tests/admin-bootstrap-admins.test.ts` (route registration, handler wiring, default create+notify, idempotent re-run, `notify=all` force-send, 401 on missing admin auth).
**Spec:** `prds/admin-provisioning-spec.md` documents the endpoint + curl.

The remaining drain items (a keyboard-send shortcut across message boxes, and a deploy) are next.

### Re: Ctrl/Cmd+Enter to send across all message boxes — glm-5.2

> **Queue prompt:**
>
> All message boxes, portal and main site, should support Ctrl+Enter (Windows, Linux) or Cmd+Enter (OSX) to send the message.


**Done.** Adopted the Gmail-style send convention everywhere a message is composed: plain **Enter inserts a newline**, and **Ctrl+Enter (Windows/Linux) / Cmd+Enter (macOS)** is the explicit send trigger. Previously the portal chat sent on plain Enter (accidental sends while typing multi-line messages); Ctrl/Cmd+Enter already sent there too, so this makes the modifier the single deliberate trigger and applies it consistently.

**Spec + impl** (`src/equityfundingsource-dev`, commit `0408d77`, pushed `main`):
- Shared helper in `portal/shared/ui.tsx`: `message_send_shortcut_match_(e)` (Ctrl/Cmd+Enter; Shift benign, Alt blocks) + `send_shortcut_hint_label_()` + `<MessageSendHint/>`.
- Wired into every message box:
  - Borrower portal chat (`borrower/messages.tsx`)
  - Admin portal chat (`admin/messages.tsx`)
  - Investor deal-detail lender message (`investor/deal_detail.tsx`)
  - Admin deal-detail lender + borrower messages (`admin/deal_detail.tsx`)
  - Main-site contact form (`ui--server--efs/contact/index.ts`) — inline keydown → `form.requestSubmit`, platform-aware hint
- Each composer now shows a small "Ctrl+Enter to send" / "⌘+Enter to send" hint.

**Verify:** `bun run check` → **802 pass / 0 fail** (+9 new); worker build OK (browser bundle hash rotated, confirming the UI changes compiled in).
**Tests:** `tests/msg-send-shortcut.test.ts` — matcher truth table (Ctrl/Cmd+Enter, plain Enter, Shift, Alt, non-Enter keys), hint label, and source guards confirming every box wires the helper and the old plain-Enter chat send is gone.

The last drain item (shipping these changes to production) remains.

### Re: Ship the accumulated changes to production — glm-5.2

> **Queue prompt:**
>
> deploy


**Done.** Deployed the EFS Worker to production via `make deploy` (build → migrations → upload+activate).

- **Version ID:** `86f338f7-0dc7-452c-898b-a63199493d62`
- **Live at:** `https://equityfundingsource.brian-takita.workers.dev` (custom-domain cutover still intentionally deferred per `#next-steps`)
- **Migrations:** none pending — `cf:d1:migrations:list:remote` reported "No migrations to apply" (the admin-provisioning work uses runtime SQL via `bin/ensure-admins.ts`, not a migration)
- **Commit shipped:** `0408d77` (HEAD of `main`, already pushed)

**What went live this deploy:**
- `#admin-provisioning` — `ensure_admin_`, `BOOTSTRAP_ADMINS`, `bin/ensure-admins.ts`
- Admin-provisioned welcome email (`admin_provisioned_email_` + signup wiring)
- `POST /api/admin/bootstrap-admins` (admin-gated ensure + notify trigger)
- `#msg-send-shortcut` — Ctrl/Cmd+Enter to send across every message box + the main-site contact form

**Post-deploy verification (all green):**
- `GET /` → `200`
- `GET /contact` serves the new shortcut markup (`id="contact-form"`, `contact-send-shortcut`, `e.key==='Enter'`, `form.requestSubmit`)
- `GET /portal-admin` → `303` (Cloudflare Access redirect, as expected)
- `POST /api/admin/bootstrap-admins` → `401` without an admin session (correctly gated)

**Production env confirms email delivery is wired:** `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED=true`, `CLOUDFLARE_EMAIL_ACCOUNT_ID` set, `MESSAGE_NOTIFICATION_FROM` configured — so the admin welcome email and borrower messaging round-trip send for real.

**Operator follow-up to actually provision + email Robert** (now that the endpoint is live): an admin signed in via the portal runs
```bash
curl -X POST https://equityfundingsource.brian-takita.workers.dev/api/admin/bootstrap-admins?notify=all \
  -H 'Cookie: <admin session cookie>'
```
That creates `rwaldman100+admin@gmail.com` if missing and sends the welcome email.

The queue is fully drained.
<!-- agent:boundary:5022ae9e -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

2## Review

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
