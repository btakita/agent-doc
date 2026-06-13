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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-084343.md`*

Compacted content:
- Archived 6 response topic(s): Review & E-Sign; Ongoing doc + draft email to Judith re: e-sign legal requirements; do #gz9h; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: #advance-review — triaged the 11 review items into actionable backlog — opus-4-8

> **Queue prompt:**
>
> #advance-review


Went through all 11 gated review items and queued the next concrete step for each that has one: 7 new backlog items, one stale record resolved, three genuinely-gated items left as-is.

**Agent-actionable now (no external gate):**
- `#qakx-impl` — Investor portal document pages: D1/R2-backed APIs + authed upload/request/download, replacing fixtures. Buildable today.

**Partially actionable (non-TMO half buildable now; TMO sync stays gated on #dzmm/#k2xg):**
- `#vd8s-dragwire` — Committed-lane drag-to-confirm-wire with multi-pending-fund disambiguation.
- `#67v2-kanban` — TMO-aligned-stage Kanban UI with manual advance; auto-advance deferred.
- `#37ah-backend` — Broker portal D1 backend; TMO sync behind the webhook gate.
- `#xcvg-admin` — Borrower-message admin owner + in-app notify + inbound-reply→D1 append. This also answers the borrower-portal-messages question: yes to an admin owner and in-app round-trip now; the email legs (notify on send, reply-by-email both directions) wait on the Resend sending domain (#f4rq) + Google Workspace SMTP (#m6ch/#p40t).

**Operator/comms follow-ups (these unblock the external gates):**
- `#dzmm-followup` — send Robert the six-question TMO webhook prompt; this one unblock clears the writeback gate behind #dzmm/#k2xg/#7a4d/#vd8s/#67v2/#zwws.
- `#jnzs-approval` — draft the funded-deal copy/lien/legal approval request to Judith/EFS reviewer to unblock invoicing + launch.

**Left as-is (no new step — same gates, already covered):**
- `#zwws` — remaining phases share the TMO writeback + #f4rq gates captured above.
- `#next-steps` — production-state record; custom-domain cutover intentionally deferred, SSO gated on #m6ch.
- `#dropbox-sync-adapter` — blocked on Dropbox access.

Resolved **#1v5m** (a completed 2026-05-25 Cloudflare-token record, not pending work) out of review. No spec/test/code changed this cycle — pure triage, so nothing to test.

Note: preflight flagged a low-confidence semantic match between #vd8s and the shipped #vtx8 (lead-vanishing admin pipeline). They're distinct (investor-pipeline drag-wire vs. lead visibility), so #vd8s stays.
<!-- agent:boundary:0d141752 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#qakx-impl] Investor portal document pages: implement D1/R2-backed APIs for Documents, Statements, and Accreditation with authenticated upload/request/download, replacing fixture data. Fully actionable now. Advances review #qakx.
- [ ] [#vd8s-dragwire] Investor Pipeline phase 2 (non-TMO half): drag-to-confirm-wire from the Committed lane with multi-pending-fund disambiguation. TMO AddLoanFunding Active-lane sync stays gated on #dzmm/#k2xg. Advances #vd8s. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [ ] [#67v2-kanban] Admin loan pipeline: build TMO-aligned-stage Kanban UI (drag cards, manual stage advance) wired to dashboard with TMO stages as single source of truth; auto-advance-on-requirements deferred to #dzmm/#k2xg. Advances #67v2.
- [ ] [#37ah-backend] Broker portal backend: replace static broker demo data with D1-backed submit-deal, pipeline, commissions, documents, and notifications; TMO sync behind the #dzmm webhook gate. Advances #37ah.
- [ ] [#xcvg-admin] Borrower secure-message admin: add admin owner/assignment, in-app notification surface, and inbound-reply -> D1 chat-append plumbing (non-email half of #xcvg). Email send/receive stays gated on #f4rq/#m6ch/#p40t. Advances #xcvg.
- [ ] [#dzmm-followup] Send Robert the six-question TMO outbound-webhook prompt (src/equityfundingsource-dev/docs/tmo-webhooks-question.md) and record his answer; this single unblock clears the writeback gate behind #dzmm/#k2xg/#7a4d/#vd8s/#67v2/#zwws. Operator/comms action.
- [ ] [#jnzs-approval] Draft the funded-deal images/copy/lien-position/legal-disclosure approval request to Judith or the EFS reviewer to unblock invoicing and launch sign-off. Advances #jnzs. Operator/comms action.
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
