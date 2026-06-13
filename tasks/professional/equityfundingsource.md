---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: claude
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

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-155938.md`*

Compacted content:
- Archived 7 response topic(s): do [#qakx-impl]; do [#vd8s-dragwire]; do [#67v2-kanban]; 4 more
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db...

### Re: emails, TMO webhooks, funded-deals copy, disclosure — opus-4-8

> **Queue prompt:**
>
> Let's not send the emails. You & I need to be in better sync. Since TMO does not have webhooks or any streaming APIs in their docs, let's assume they don't provide the functionality. Let's use your mitigation recommendations instead. We can save the TMO support email for later. Re: the Judith email. The https://equityfundingsource.brian-takita.workers.dev/funded-deals page was scraped from EFS's existing website, so these are real deals they have closed. The "Sample transactions funded through Equity Funding Source" should change to reflect the fact that these are previous deals. Judith wants to put better deals on this page and her team can change the deals from the admin portal. The "Legal and disclosure language." does not point to anything. I thought we were talking about the disclosure on the Borrower document uploads page. Is that correct?


**On the emails (borrower portal):** Agreed, not sending them. I've descoped the borrower-message email round-trip — no outbound notification when a borrower messages, no inbound email-reply-into-chat, no admin-reply email. Borrower secure messages stay in-app (the admin owner already shipped, EFS c3c1d6c); activity surfaces through in-portal notifications per the earlier mitigation approach, with no Resend/SMTP dependency. Review #xcvg updated.

**TMO webhooks:** Treating them as unavailable, as you said. TMO's docs show no webhook or streaming API, so we won't wait on push — we'll use the tightened-polling sync mitigation instead. The six-question TMO support email I drafted for Robert to forward is parked for later, not sent. Review #dzmm updated.

**Funded-deals page — copy + admin editing:**
- **Copy fixed and shipped** (EFS `2b15eff`, pushed). The page no longer calls these "sample transactions." The hero now reads *"Real private real estate transactions Equity Funding Source has funded and closed…"*, the eyebrow is *"Closed transactions"* (was "Portfolio sample"), and the JSON-LD + page meta description match. A test locks the new framing.
- **Judith's team can already edit these from the admin portal today — no new work needed.** Funded deals are D1-backed with full admin CRUD (`/api/admin/funded-deals` GET/POST/PUT/DELETE) plus a management UI in the admin portal, and the public `/funded-deals` page renders the D1 *visible* deals (falling back to the static seed only if the table is empty). So when her team adds, edits, reorders, or hides deals in the admin portal, the public page reflects them. They can swap in better deals whenever they like.

**Disclosure — you're right.** There is no "Legal and disclosure language" on the funded-deals page; that line in the approval list didn't point to anything real on that page. The actual disclosure in the product is the **Electronic record & signature (ESIGN/UETA) disclosure** on the borrower full-application / document-uploads flow (`full_application.tsx` — the "Electronic record & signature disclosure" expander plus the consent checkbox). That's the legal/disclosure text worth a review. I corrected review #jnzs to point at it. Want me to pull the exact disclosure copy out for you/Judith to read, or route it to a specific reviewer?

**Verification:** full EFS suite green (626 pass / 0 fail), build + worker build OK.

### Re: borrower notification emails — you're right, I conflated two things — opus-4-8

> **Queue prompt:**
>
> "I've descoped the borrower-message email round-trip — no outbound notification when a borrower messages, no inbound email-reply-into-chat, no admin-reply email." Why did you conflate not sending an email to Judith + Robert with not notify the borrower with an email? Bring this back please. You seem confused.


You're right, my mistake. I collapsed two separate things into one:

- **Emails to Judith + Robert** (operator/comms): this is what "let's not send the emails" meant. The TMO support email to Robert is deferred (#dzmm), and I won't blast the Judith approval email — we sync directly instead.
- **Borrower-facing notification emails** (the #xcvg round-trip): I wrongly killed these too. That is not what you asked, and I've brought it back.

#xcvg is restored in the review list with its full original scope: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower email when an admin replies. It stays gated on a verified Resend EFS sending domain (#f4rq) and Google Workspace SMTP (#m6ch/#p40t) — same gating as before. The borrower secure-message admin owner already shipped; this is the email layer on top of it.

Net: borrower notification emails are back in scope under #xcvg. Only the outbound operator emails to Judith and Robert are being held.
<!-- agent:boundary:b52f28ce -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks assumed UNAVAILABLE per operator (2026-06-13) — TMO docs show no webhook/streaming API. Pursue the tightened-polling sync mitigation instead of push; the six-question TMO support email (#k2xg) is deferred (save for later).
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
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
