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
queue: start
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-074303.md`*

Compacted content:
- Archived 2 response topic(s): Replace the single upload area with one per form type; File-type icons, delete buttons, upload activity feed, and realtime updates
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Review & E-Sign — printed-name validation + legal compliance — opus-4-8

> **Queue prompt:**
>
> In the Review & E-Sign step ![img_10.png](img_10.png), should we validate the printed name is the same as the borrower's account name? Is this Review & E-Sign compliant with legal requirements for a proper signature for the borrower application?


Good questions. I reviewed the current Review & E-Sign step (`portal/borrower/full_application.tsx` step 6) and the server-side signature capture (`borrower/applications/routes.ts`). This is an assessment with recommendations — I queued the concrete follow-ups as backlog items rather than implement a name-check this cycle, because both questions need a product/legal decision first (details below). Not legal advice.

**What the flow captures today (solid foundation):** an explicit ESIGN/UETA consent checkbox, typed initials, printed name, `signed_at` timestamp, `client_ip`, and `user_agent`, plus acknowledgement + credit/background authorization text. That covers the core e-signature elements: intent to sign, consent, attribution, and an audit trail.

**1. Should we validate printed name == borrower's account name?**
Recommend **a soft check, not a hard block** — and compare against the **application's legal name** (`personal.first_name + last_name`), not the account display name:
- A hard equality block would lock out legitimate signers: nicknames/name changes, typos in the account profile, co-borrowers signing, and entity/trust applicants (an LLC's authorized signer won't match the borrower name).
- Better UX: normalize both (lowercase, collapse whitespace/punctuation) and on mismatch show a **non-blocking confirmation** ("This doesn't match the name on your application — confirm it's correct?"). Let them proceed after confirming.
- Either way, **record the match/mismatch and the compared values in the signature audit row** — that's more useful for fraud review than a blunt block. Queued as an actionable item.

**2. Is this Review & E-Sign legally compliant for a borrower application?**
It's a **reasonable, good-faith ESIGN Act (15 USC 7001) / UETA e-signature** — it has the four things courts look for: intent to sign (explicit "E-sign application" action), consent to transact electronically, association of the signature with the record (linked to the application id), and an attribution/audit trail (IP, UA, timestamp, authenticated user). For many purposes that holds up.

Gaps to close for **full consumer-ESIGN** confidence (queued as an actionable item):
- **Consumer disclosures (ESIGN §101(c)):** before consent, consumers should be shown hardware/software requirements, the right to a paper copy, and the right to withdraw consent. The current checkbox asserts consent but doesn't present these disclosures.
- **Retain what was signed:** store a tamper-evident snapshot or hash of the exact application content at signing, so EFS can later prove *what* the borrower agreed to — not just that they signed.
- **Distinct consent timestamp** vs. `signed_at`.
- **Jurisdiction:** CA hard-money lending carries its own disclosure obligations. **Have EFS counsel review before launch sign-off** — I can implement whatever they require, but the compliance determination should come from them, not me.

Bottom line: the mechanism is sound for intent + attribution; the main compliance work is the consumer disclosure layer and signed-content retention. Nothing was code-changed or pushed this cycle (analysis only); both recommendations are now in the backlog.

### Re: Ongoing doc + draft email to Judith re: e-sign legal requirements — opus-4-8

> **Queue prompt:**
>
> Please create an ongoing document that includes a draft email to judith-krug about the ESign legal requirements and for her to consult a lawyer.


Created the ongoing document: **`tasks/professional/equityfundingsource/esign-legal-compliance.md`** (committed `a9d43d858`). It's a living tracking doc for the borrower-application e-signature compliance, with three parts:

1. **Where the e-signature stands today** — the engineering assessment from the prior cycle (intent to sign, ESIGN/UETA consent, association with the record, audit trail via IP/UA/timestamp).
2. **Open items before launch sign-off** — consumer disclosures (ESIGN §101(c)), signed-content retention (snapshot/hash), distinct consent timestamp, CA hard-money specifics (e.g. RE 885), and the printed-name handling — cross-linked to backlog `#fxsv` and `#gz9h`.
3. **A draft email to Judith** — short, per her tone (she's busy; keep it brief, not pushy). It tells her the e-sign step is built to ESIGN/UETA standards, recommends her **attorney review** the disclosures before launch, and offers to prepare a summary the lawyer can work from. It does not send anything — it's a draft she/you can edit and send when ready.

I kept the email deliberately short and put the detail in the tracking doc so the lawyer-facing summary lives somewhere durable rather than in an inbox.

**Scope note on "push":** I committed the new doc to the agent-loop superproject locally (`a9d43d858`). I did **not** push the superproject — it has unrelated in-flight submodule changes from other sessions, so a push would sweep those in. Say the word and I'll push, or push it yourself when the tree is clean. (The deployable EFS code from the earlier cycles is already on `origin/main` of the EFS submodule.)

Next in the queue: `do #gz9h` (soft printed-name match), `do #fxsv` (ESIGN/UETA hardening), `do #xn77` (realtime/poll admin updates) — I'll take those one per cycle as the queue drains.

### Re: do #gz9h — soft printed-name match on the e-signature — opus-4-8

> **Queue prompt:**
>
> do [#gz9h]


Done — implemented, tested, committed + pushed (EFS `428b980`; not deployed — no `deploy` in the preset). Migration ships, so this goes live on the next deploy (the deploy step runs D1 migrations before the worker).

**Behavior:** when the borrower types their printed name in Review & E-Sign, it's compared to the legal name on the application (`personal.first_name + last_name`), normalized case/space/punctuation-insensitively. On a mismatch the form shows a **non-blocking amber confirmation** — "this doesn't match the name on your application (X); that's fine if you're a co-borrower, signing for an entity/trust, or your name changed — confirm below" — with a checkbox. Signing proceeds once they confirm or correct it. It **never hard-blocks** (per the recommendation: co-borrowers, trusts/LLCs, and name changes are legitimate).

**Audit trail:** the match result and the compared legal name are recorded on the signature row via two new columns — `name_match` (1/0/NULL) and `name_match_expected` — added in migration `0063`. Both the borrower and broker sign routes compute and store it. NULL means "not evaluated" (legacy rows or no legal name on file).

**Files:** `sections_repo.ts` (shared `normalize_name_`/`names_match_` + setter/reader columns), `borrower/applications/routes.ts` (both sign handlers), `borrower/full_application.tsx` (mismatch confirm UI), migration `0063`, +6 tests in `tests/esign-name-match.test.ts`. Full suite **606 pass / 0 fail**; builds clean.

Queue continues: next up `do #fxsv` (ESIGN/UETA hardening), then `do #xn77` (realtime/poll admin updates).

### Re: do #fxsv — ESIGN/UETA hardening — opus-4-8

> **Queue prompt:**
>
> do [#fxsv]


Done — all three parts implemented, tested, committed + pushed (EFS `a869545`; not deployed). Migration ships with the code, so it goes live on the next deploy.

**1. Consumer disclosures before consent (ESIGN §101(c)).** The Review & E-Sign step now has an "Electronic record & signature disclosure" panel above the consent checkbox covering the three required elements: system requirements, the right to a paper copy at no charge, and the right to withdraw consent. The consent label now reads "I have read the disclosure above and consent…". The wording is a standard first pass — **EFS counsel should finalize it** (per the earlier recommendation).

**2. Tamper-evident snapshot of what was signed.** At signing, both sign routes serialize the full application content (all sections except the signature block) into deterministic JSON (recursively sorted keys) and compute its SHA-256. The snapshot JSON + hash are stored on the signature row, so EFS can later prove exactly what the borrower agreed to — not just that they signed.

**3. Distinct consent timestamp.** The client records when the consent box was checked and sends it; the server stores `consent_at` separately from `signed_at` (falling back to signing time if absent).

**Schema:** migration `0064` adds `consent_at`, `signed_content_hash`, `signed_content_json` to `application_signatures`. **Files:** `sections_repo.ts` (setter/reader columns), `routes.ts` (`stable_json_` + `signed_content_snapshot_` helpers, both sign handlers), `full_application.tsx` (disclosure panel + consent-time capture), migration `0064`, +4 tests in `tests/esign-hardening.test.ts`. Full suite **610 pass / 0 fail**; builds clean.

Queue continues: last item `do #xn77` (realtime/poll admin updates).
<!-- agent:boundary:f12e7a83 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~Please create an ongoing document that includes a draft email to judith-krug about the ESign legal requirements and for her to consult a lawyer.~~
- ~~do [#gz9h]~~
- ~~do [#fxsv]~~
- do [#xn77]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xn77] [recommended] Realtime/poll admin updates: refresh admin application/pipeline views on document uploads + status changes without manual reload. Cheapest first: poll list/detail APIs every ~15-30s with ETag/If-None-Match. CF push options: Server-Sent Events from the Worker (one-way, simplest) or a Durable Object broadcast (EFS already runs the BorrowerChatRoom DO) fanning an 'application:<id> changed' event to open admin sessions via EventSource/WebSocket. Start polling, upgrade to SSE/DO if latency matters.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
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
