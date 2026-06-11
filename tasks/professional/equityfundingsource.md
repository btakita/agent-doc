---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: codex
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

EFS repo main: EFS `e6310d1` adds priority-sorted admin dashboard `attention_items` for nonzero operational work: pending funding commits, open applications, new leads, open document requests, pending lender requests, then recent uploads. This queue item used `#spec-test-commit-push`, so it was committed and pushed but not deployed. EFS workers.dev production remains Worker version `c2d7b525-8682-4e51-aa5e-05a7d13eb4ac` (deployed 2026-06-10): lender document **View** opens a stacked preview modal above the Lender Pipeline lender/card dialog; Esc closes only the document layer and leaves the lender dialog open (#doc-stack-esc, EFS `5c655ba`). The **Lender Pipeline** sidebar nav badge counts active board investor cards — one per `users.role = 'investor'` row shown in `/portal-admin/lender-pipeline` — instead of only pending lender requests + pending funds (#investor-pipeline, EFS `5af3c7d`). Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260610-161847.md`*

Compacted content:
- Archived 3 response topic(s): Boolean form fields as checkboxes (read-only → disabled); Human-readable lender field labels + lender-request documents link; Table row name opens the quick-view dialog (not a page jump)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Tabbed lender dialog — Lender Info + User tabs — opus-4-8

Done, tests updated, committed + pushed + deployed (EFS `c11e58c`; live 200).

The Lenders-table dialog is now **tabbed**, embedding the two existing pages instead of duplicating their content:

- **Lender Info tab** → the dedicated lender page (`/portal-admin/lenders/:id`) rendered inline: request status/account, submitted lender data (with the human-readable labels), lender documents, and the **accept (assign account & complete) + deny request** controls.
- **User tab** → the investor profile editor (`/portal-admin/users/:id`) rendered inline via `UserProfileEditor`.

**How it's wired:**
- Added an `embedded` prop to `AdminLenderDetail` that hides the page chrome (topbar / "Back to lenders") so the same component renders cleanly in the dialog, plus an `on_request_change` callback so the table row + dialog header stay in sync after an accept/deny — no refetch.
- Retired the old single-view dialog (the `CopyRow` data grid + separate `DenyLenderRequestModal`); the embedded detail page owns that workflow now, so there's one source of truth. Net −92 lines.
- Moved `lender_field_label_` to `lender_detail` (re-exported from `lender_management` for compatibility) so the humanized labels show on both the page and the dialog.

**Scope note:** I verified the build + full suite structurally, but I did **not** do a live click-through of the dialog — that needs an authenticated admin session (Cloudflare Access) I can't drive headlessly. The embedded components are existing, working pages, so risk is integration-level (props/layout). Worth a 10-second eyeball on `/portal-admin/lender-pipeline?tab=lenders` → click a lender → toggle the two tabs.

**Verification:** updated `admin-dashboard`, `lender-request-labels-docs`, and `table-row-opens-dialog` tests for the new structure. Full suite **543 pass / 0 fail**, worker build clean, no D1 migrations.

### Re: Codex session handoff — gpt-5

Acknowledged. The session document now identifies `agent: codex`, so this cycle is only a harness/attribution handoff rather than an EFS implementation request.

No repo work was requested by the edit. I left the backlog and gated review items unchanged; preflight did surface two semantic-completion warnings for `#cm3e` and `#vd8s`, but those are advisory cleanup signals rather than directives for this turn.

Verification: `agent-doc --version` is `0.34.0`; `agent-doc plan` required only finalization for this prompt response.

### Re: Lender Pipeline tabbed lender dialog — gpt-5

> **Queue prompt:**
>
> We have /portal-admin/lenders/:lenderId and we have /portal-admin/users/:userId. The Lenders tab in the the Lender Pipeline should open the lender dialog. The lender dialog should a "Lender" tab and a "User" tab. The "Lender Info" tab should have the contents of /portal-admin/lenders/:lenderId..include the buttons to accept and deny the request. The "User" tab should have the contents of /portal-admin/users/:userId. deploy


The queued request is already complete in the current EFS repo state. `src/equityfundingsource-dev` is clean, `HEAD` and `origin/main` both point to `c11e58c921c9c8a229d9d0786859bdd133bf22a3` (`lender pipeline: tabbed lender dialog (Lender Info + User)`), and that commit is the implementation described by the queue item: Lenders-table rows open the lender dialog, the dialog has **Lender Info** and **User** tabs, the Lender Info tab embeds `/portal-admin/lenders/:lenderId` including accept/deny controls, and the User tab embeds `/portal-admin/users/:userId`.

Fresh verification on the current commit:
- `bun run check` passed: **543 pass / 0 fail**, app build clean, worker build clean.
- `git rev-parse HEAD origin/main` returned the same `c11e58c...` SHA, so the EFS commit is pushed.
- `curl -I https://equityfundingsource.brian-takita.workers.dev/` returned HTTP 200.
- `curl -I https://equityfundingsource.brian-takita.workers.dev/portal-admin/lender-pipeline` returned HTTP 303 to `/login?role=admin&next=%2Fportal-admin%2Flender-pipeline`, which is expected for the protected admin route.

I did not make source changes in this cycle. `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`, and `gh workflow list` returned no configured workflows. The EFS repo stayed clean after verification; unrelated dirty files/submodule pointers in the superproject were left untouched.

### Re: Lender Pipeline menu count — gpt-5

> **Queue prompt:**
>
> The admin menu "Lender Pipeline" counts are 0. They should be the number of active items in the pipeline. deploy

Done, committed, pushed, and deployed (EFS `5af3c7d`; Worker version `675e2475-7351-4632-b17d-d9ce405cd532`).

The admin Management sidebar badge for **Lender Pipeline** now counts active board items: one investor card per `users.role = 'investor'` row, matching what `/portal-admin/lender-pipeline` displays. It no longer shows `0` just because there are no pending lender requests or pending wires.

Verification:
- `bun test tests/admin-investor-pipeline.test.ts tests/admin-dashboard.test.ts` — 50 pass / 0 fail.
- `bun run check` after rebase — 545 pass / 0 fail, app build clean, worker build clean.
- `bun run deploy:worker` — no D1 migrations to apply; Worker deployed successfully.
- Live smoke: `/` returns HTTP 200; `/portal-admin/lender-pipeline` redirects to admin login with HTTP 303; `/api/admin/nav-counts` rejects unauthenticated access with HTTP 401.
- GitHub Actions note: this repo has no configured workflows; `gh run list --workflow CI --limit 1` reports no workflow named `CI`.

### Re: Lender Pipeline document modal stack — gpt-5

> **Queue prompt:**
>
> In /portal-admin/lender-pipeline, when I open the lender dialog then open Document then press the Esc key, the lender dialog should still be open. The Document dialog should layer on top of the lender dialog. deploy

Implemented and deployed.

The lender document `View` action now opens an in-app stacked document preview modal instead of a new tab. The preview modal uses `PortalModal` with `z="z-[1200]"`, so it layers above the existing lender/card dialog; the existing topmost-only Escape stack closes only the document modal and leaves the lender dialog open. The modal previews the document inline and still offers a direct `Download` link.

Backend support now lets the admin investor-document download route honor `?disposition=inline|attachment`, so preview and download behavior are explicit.

EFS commit: `5c655ba` (`admin docs: stack lender document preview`), pushed to `origin/main`.
Worker deploy: `c2d7b525-8682-4e51-aa5e-05a7d13eb4ac`.

Verification:
- `bun test tests/admin-investor-pipeline.test.ts tests/admin-investor-documents.test.ts tests/portal-table-dialog.test.ts` — 55 pass / 0 fail.
- `bun run check` — 547 pass / 0 fail, app build clean, worker build clean.
- `bun run deploy:worker` — deployed; no D1 migrations to apply.
- Worker smoke checks: `/` 200, `/portal-admin/lender-pipeline` 303 to admin login, `/api/admin/nav-counts` 401 unauthenticated.
- GitHub Actions note: this repo has no configured workflows; `gh run list --workflow CI --limit 1` reports no workflow named `CI`.

### Re: Admin dashboard attention items — gpt-5

> **Queue prompt:**
>
> Add items that need attentention to the admin dashboard. Sort by priority/urgency.

Implemented and pushed.

> **Queue prompt:**
>
> deploy


The admin dashboard now has a `Needs attention` panel above the metric cards. The backend returns `attention_items` for nonzero operational work, sorted by explicit priority/urgency:

1. Pending funding commits
2. Open applications
3. New application leads
4. Open document requests
5. Pending lender requests
6. Recent uploads

Each item carries `priority`, `urgency`, `count`, `detail`, and a filtered admin link. The UI renders the sorted list with urgency badges and count links.

EFS commit: `e6310d1` (`admin dashboard: surface priority attention items`), pushed to `origin/main`.

Verification:
- `bun test tests/admin-dashboard.test.ts` — 23 pass / 0 fail.
- `bun run check` — 549 pass / 0 fail, app build clean, worker build clean.
- GitHub Actions note: this repo has no configured workflows; `gh run list --workflow CI --limit 1` reports no workflow named `CI`.

This queue item used `#spec-test-commit-push`, so I did not deploy a new Worker.

No new actionable follow-up items were found for the backlog.
Implemented and pushed.

The admin dashboard now has a `Needs attention` panel above the metric cards. The backend returns `attention_items` for nonzero operational work, sorted by explicit priority/urgency:

1. Pending funding commits
2. Open applications
3. New application leads
4. Open document requests
5. Pending lender requests
6. Recent uploads

Each item carries `priority`, `urgency`, `count`, `detail`, and a filtered admin link. The UI renders the sorted list with urgency badges and count links.

EFS commit: `e6310d1` (`admin dashboard: surface priority attention items`), pushed to `origin/main`.

Verification:
- `bun test tests/admin-dashboard.test.ts` — 23 pass / 0 fail.
- `bun run check` — 549 pass / 0 fail, app build clean, worker build clean.
- GitHub Actions note: this repo has no configured workflows; `gh run list --workflow CI --limit 1` reports no workflow named `CI`.

This queue item used `#spec-test-commit-push`, so I did not deploy a new Worker.

No new actionable follow-up items were found for the backlog.
<!-- agent:boundary:c1b5486a -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#efspgo] [recommended] EFS online-payment go-live config (after Robert provides Stripe account): set Cloudflare secrets STRIPE_SECRET_KEY + STRIPE_WEBHOOK_SECRET + var STRIPE_PAYMENTS_ENABLED=true (wrangler secret put / wrangler.toml), point a Stripe webhook at /api/stripe/webhook, then in admin Platform Settings fill the borrower payment wire/ACH + lender ACH values and flip the per-method enable toggles. Deploy with bun run deploy:worker. Capability is BUILT + admin-configurable (EFS c790ed3); this is config + deploy only.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
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
intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#cm3e] [recommended] Borrower online payment collection (Stripe) + TMO sync: add ACH/card payment in the borrower portal make-payment section; on Stripe payment_intent.succeeded webhook, verify signature, record the receipt in a new EFS D1 loan_payment_transactions table, reconcile the matching loan_payments schedule row, and push the payment to TMO. GATE: confirm ABS/TMO exposes a borrower-payment/receipt WRITE endpoint (current docs/TMO cover loan/application writes, investor AddLoanFunding, and schedule/funding reads only) — ties into the TMO-writeback gate (dzmm/k2xg). Deferred non-MVP collection path from e245; handle KYC, partial payments, NSF reversals, fees.
- [/] [#xcvg] [recommended] Borrower message email round-trip: add an admin owner for borrower-portal secure messages, email notification when a borrower sends a message, inbound email reply that appends into the D1 chat, and borrower email when an admin replies. Builds on existing chat_sessions/chat_messages. Gated on Resend EFS sending domain (f4rq) + Google Workspace SMTP (m6ch/p40t).
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [/] [#1v5m] Resolved 2026-05-25: created an EFS-specific Cloudflare API token scoped to account 1e7e194ff8da4ad9e40ce041aea84f76 with D1, Workers Scripts, R2, observability, and read-only user/account metadata permissions; updated passage btak/equityfundingsource/cloudflare_api_token. D1 migration list/apply now succeeds and reports no pending migrations.
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
