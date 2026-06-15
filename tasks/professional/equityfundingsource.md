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

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-084628.md`*

Compacted content:
- Archived 1 response topic(s): Advance review
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: EFS->TMO writeback replay — gpt-5

> **Queue prompt:**
>
> [#dzmm-writeback-replay]


I checked `#dzmm-writeback-replay` against the current session state. This remains externally blocked: TMO webhooks are unavailable, and the EFS-to-TMO replay cannot be implemented safely until both the `#7a4d` write field map and `#k2xg` support answers are available. I moved the queue item to gated review rather than leaving it as a drainable backlog item, so the queue can continue without repeatedly attempting blocked TMO writeback work.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
Verification: no code changes; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that implementation is blocked until the TMO write contract is known.
Escalation: none.

### Re: [#xcvg-prod-email] — gpt-5

> **Queue prompt:**
>
> [#xcvg-prod-email]


I can’t perform a harness `/clear` from inside this already-loaded model context. I’m going to stop here rather than draining `#xcvg-prod-email` in the stale context; re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` after `/clear` so the queue head runs cleanly.

### Re: Borrower secure-message email production pass — gpt-5

`#xcvg-prod-email` cannot be completed as implementation work yet because the production messaging path is still externally gated: either the EFS sending domain and Google Workspace SMTP path need to be verified (`#f4rq` / `#m6ch` / `#p40t`), or `#z3kx` needs to become the path of record through GoHighLevel. I gated the exact queue backlog item so the previously consumed queue head is no longer an open, silently dropped backlog item.

For the underlying borrower-portal questions: yes, the shipped scope already has an admin owner for borrower secure-message threads, and the intended production pass is to notify admins when borrowers send messages, support inbound email replies that append to the D1 chat thread, and notify borrowers by email when an admin replies. The remaining work is production enablement and verification, not deciding whether the feature belongs in scope.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#xcvg-prod-email` while preserving the external production-email gate.
Escalation: none.

### Re: [#jnzs-approval-followup] — gpt-5

> **Queue prompt:**
>
> [#jnzs-approval-followup]


I can’t run the harness `/clear` from inside this loaded model context, and I won’t process `#jnzs-approval-followup` here. Please clear the Codex context and re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so the next queue head runs with fresh context.

### Re: Funded-deals approval follow-up — gpt-5

I resolved the dropped-queue state for `#jnzs-approval-followup` and prepared the approval packet content that should be sent/tracked with Judith or the EFS reviewer. I did not send an external email from this pane, so I gated the item on the reviewer decision and any requested edits.

Approval packet draft:

- Screenshots to attach: public `/funded-deals`, homepage `Recent transactions`, and admin `Funded Deals` editor if showing how Judith/EFS can maintain records.
- Public funded-deals page copy: “A working record of private lending execution.” Supporting copy says these are real private real estate transactions EFS has funded and closed, organized for borrowers, brokers, and investors evaluating private real estate lending experience.
- Funded-deals section copy: “Funded real estate loans.” Supporting copy says it is a selected record of closed private real estate loans arranged by Equity Funding Source.
- Current seeded records include visible closed transactions in Flintridge, Pasadena, Redondo Beach, Malibu, Chula Vista, Montebello, Santa Barbara, Los Angeles, Santa Monica, Big Bear City, Oahu, South Gate, Moorpark, Santa Ana, Inglewood, El Dorado Hills, Indian Wells, and San Pedro, with amounts from `$50,000` to `$1,465,000`.
- Lien-position language to approve: homepage loan parameters list “Lien position: 1st, 2nd, other junior” and notes “LTV varies by lien position and total encumbrance”; footer disclaimer says loans are secured by real estate in first, second, and other junior lien positions and are subject to borrower/property qualification, not a commitment to lend.
- ESIGN/UETA disclosure excerpt for legal/reviewer visibility: borrower full-application review/sign step tells users they must have a current web browser, internet connection, PDF save/view ability, and a valid email address; they may request paper copies at no charge by contacting EFS; they may withdraw electronic-consent before or after signing, with withdrawal not affecting already signed records.

Decision to track: approve as-is, request copy/image/lien-position edits, or request legal edits to the borrower ESIGN/UETA disclosure. Any requested edits should become implementation backlog items under `#jnzs`.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg -n ...`, `sed -n ...` for funded-deals/home/full-application source inspection.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#jnzs-approval-followup` while preserving the real external reviewer gate.
Escalation: none.

### Re: [#67v2-auto-advance] — gpt-5

> **Queue prompt:**
>
> [#67v2-auto-advance]


I can’t execute `/clear` from inside this already-loaded model context, and I will not process `#67v2-auto-advance` here. Clear the Codex context first, then re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so the next queue head runs in fresh context.

### Re: Admin pipeline auto-advance — gpt-5

I resolved the dropped-queue state for `#67v2-auto-advance` and gated the exact backlog item rather than marking it shipped.

Current evidence: `#67v2a`, `#67v2-kanban`, `#67v2b`, and `#67v2d` are already archived as done: the admin Deal Pipeline exists, manual drag/status advance works, EFS-local loan-status changes enqueue `update_loan_status` rows into `tmo_cache_outbox`, and the dashboard summary mirrors the board. The remaining auto-advance layer is distinct: it needs requirement rules for each stage, evaluation against D1 plus `tmo_cache`, audited status changes, and no premature TMO replay.

The safe next implementation shape is:

- Define deterministic requirement predicates for `Application Received -> Processing -> Documents -> Underwriting -> Funded` from the TMO-aligned board, using D1 as local state and `tmo_cache_entities` as the read mirror.
- Run those predicates as an explicit admin/server job, not hidden UI behavior, so each suggested/automatic move has an audit trail and can be reviewed.
- Apply local status changes through the same `admin_loan_status_update_` / `tmo_loan_status_outbox_enqueue_` path already used by manual drag moves.
- Keep `update_loan_status` replay gated on `#dzmm` / `#7a4d`; the existing test suite explicitly asserts no replay worker drains those rows yet.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg -n ...`, `sed -n ...` for pipeline/TMO outbox source, tests, and done-archive inspection.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#67v2-auto-advance` while preserving the actual remaining gated implementation work.
Escalation: none.

### Re: [#zwws-vestings-local] — gpt-5

> **Queue prompt:**
>
> [#zwws-vestings-local]


I can’t execute the harness `/clear` from this already-loaded context, and I will not process `#zwws-vestings-local` here. Clear the Codex context first, then re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so the next queue head runs cleanly.

### Re: Investor vestings local half — gpt-5

I resolved the dropped-queue state for `#zwws-vestings-local` and gated the exact backlog item on the remaining TMO writeback contract.

Evidence from the current repo: the EFS-local half is already shipped under `#zwws-local-first`. The PRD marks Phase 4 as shipped for EFS-local vestings/special instructions, backed by migration `0041` tables `investor_vestings` and `investor_special_instructions`, routes `GET/POST/PATCH/DELETE /api/investor/vestings`, `PUT /api/investor/special-instructions`, and the investor portal `Vestings` section. Tests in `tests/investor-vestings.test.ts` cover the nav/route/UI wiring and explicitly guard that the copy does not imply active TMO sync.

The remaining part is not safe to mark done: writing vestings into TMO and any `tmo_cache_outbox` replay path still depends on the TMO writeback field map (`#7a4d`) plus sync-direction answers (`#dzmm` / `#k2xg`). Once those arrive, the follow-up should wire the TMO-facing optimistic-write/outbox path without changing the EFS-local source-of-truth behavior.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg -n ...`, `sed -n ...` for done archive, PRD, vestings repo/routes, and tests.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#zwws-vestings-local` while preserving the external TMO writeback gate.
Escalation: none.

### Re: [#zwws-new-deal-email] — gpt-5

> **Queue prompt:**
>
> [#zwws-new-deal-email]


I can’t execute `/clear` from inside this already-loaded model context, and I will not process `#zwws-new-deal-email` here. Clear the Codex context first, then re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so that queue head runs with fresh context.

### Re: Investor new-deal email — gpt-5

I resolved the dropped-queue state for `#zwws-new-deal-email` and marked the exact backlog item done based on the current repo evidence.

The workflow-priority dashboard half is already shipped: `investor_action_items_` and `investor_has_urgent_` drive the investor dashboard `Needs attention` panel, with available deals leading when nothing is urgent. The new-deal email path is also wired: publishing a deal through `admin_loans_by_id_deal_POST_` calls `notify_deal_published_for_loan_`, which sends `deal_published_investor_email_` to active investor users with a `/portal-investor/deals/:id` review link. Delivery uses the shared Cloudflare/local-capture email layer, so dev/test captures locally when delivery is not configured.

The remaining production-domain concern is not this queue item: `wrangler.toml` still uses `EFS_EMAIL_FROM = "Equity Funding Source <info@briantakita.me>"`, and switching to an EFS-verified sender stays tracked by `#f4rq`.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg -n ...`, `sed -n ...` for investor dashboard, deal publish, lifecycle email, local capture, tests, and env config.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#zwws-new-deal-email` without duplicating shipped work.
Escalation: none.

### Re: [#efs-cutover-sso-plan] — gpt-5

> **Queue prompt:**
>
> [#efs-cutover-sso-plan]


I can’t execute `/clear` from inside this already-loaded model context, and I will not process `#efs-cutover-sso-plan` here. Clear the Codex context first, then re-invoke `agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so that queue head runs with fresh context.

### Re: Workers.dev to custom-domain/SSO cutover — gpt-5

I resolved the dropped-queue state for `#efs-cutover-sso-plan` and gated the exact backlog item. I wrote the cutover checklist below, but did not execute it because the remaining steps require external Cloudflare/domain control and Google Workspace admin credentials (`#m6ch`).

Cutover checklist:

- Preflight: verify `bun run deploy:worker` is clean, remote D1 migrations are applied, and the Worker URL serves `/`, `/funded-deals`, `/login`, `/portal-admin`, and `/login/access-callback` as expected.
- Routing decision: point `equityfundingsource.com` and `www.equityfundingsource.com` to the Worker if portal/admin routes should live on the same hostname; otherwise keep public static routes on the current public host and route only portal/auth paths to the Worker.
- Cloudflare custom domain: add the Worker custom domains/routes in Cloudflare for the chosen hostnames and verify TLS is active before switching traffic.
- Access app: add both custom-domain callbacks to the Cloudflare Access self-hosted app, at minimum `equityfundingsource.com/login/access-callback` and `www.equityfundingsource.com/login/access-callback` if both hostnames serve login.
- SSO vars: confirm `ACCESS_TEAM_DOMAIN = "equityfundingsource"` and `ACCESS_AUD` still match the Access app that protects `/login/access-callback`; redeploy after any Access app/AUD change.
- Google Workspace SSO: after `#m6ch`, enable the Google identity provider in Cloudflare Access, restrict policy to explicit authorized admin emails first, and avoid a broad domain-wide allow rule until EFS approves it.
- Smoke tests: check public pages, login redirects, `/login/google?role=admin`, Access callback challenge, existing email/password login fallback, and admin/borrower/broker/investor portal entry points.
- Rollback: remove/disable Worker custom-domain routes or restore DNS to the prior host; leave workers.dev live as the fallback verification URL; if Access breaks, remove the custom-domain callback from the Access app and keep email/password login available.

Current repo evidence: `wrangler.toml` already has non-secret `ACCESS_TEAM_DOMAIN` and `ACCESS_AUD`, `/login/access-callback` verifies Cloudflare Access JWTs, and the pending-config runbook covers the workers.dev Access app. The production custom domain remains outside that runbook and is explicitly still deferred.

Changed paths: `tasks/professional/equityfundingsource.md` only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `rg -n ...`, `sed -n ...` for wrangler, auth callback, deploy docs, and pending-config runbook inspection.
Verification: no app code changed; no app tests run. Document closeout is through `agent-doc finalize` and post-finalize session check.
Confidence: high that this resolves the interrupted queue-removal guard for `#efs-cutover-sso-plan` while preserving the real external cutover/credential gate.
Escalation: none.
<!-- agent:boundary:e188c1e7 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~[#dzmm-writeback-replay]~~
- ~~[#xcvg-prod-email]~~
- ~~[#jnzs-approval-followup]~~
- ~~[#67v2-auto-advance]~~
- ~~[#zwws-vestings-local]~~
- ~~[#zwws-new-deal-email]~~
- ~~[#efs-cutover-sso-plan]~~
- [#37ah-review-dedupe]
- [#dropbox-sync-contract]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#37ah-review-dedupe] [recommended] Broker portal review closeout: verify the shipped #37ah-backend covers the broker portal backend scope, move any remaining TMO sync note under #dzmm-writeback-replay, then resolve #37ah from review.
- [ ] [#dropbox-sync-contract] Dropbox sync adapter prep: design the R2_DOCUMENTS<->Dropbox sync contract, D1 sync-log schema, tombstone/idempotency rules, and mocked adapter tests so real Dropbox credentials can be wired without reworking the flow. Advances #dropbox-sync-adapter.
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
- [/] [#37ah] [recommended] Broker portal backend — D1-backed submit-deal, pipeline, commissions, documents, and notifications shipped (#37ah-backend, 2026-06-13). Remaining: TMO writeback sync, gated on #dzmm (TMO write field map #7a4d / six-question support email #k2xg).
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
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
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
