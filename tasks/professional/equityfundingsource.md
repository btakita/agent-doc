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
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---

## Status

<!-- agent:status patch=replace -->
25 pages deployed with form backend (D1 + R2 + email). Document pipeline architecture: Tier 0-3 (WAF -> Workers AI -> Claude -> Ragie).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-142913.md`*

Compacted content:
- Archived 6 response topic(s): TMO simulator API; TMO simulator persistence; Closeout correction; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ What are #next-steps to fully implement the functionality of the investor portal. Note I do not yet have access to Dropbox. The upload can go into a private R2 temporarily...R2 can also be used as a staging area in case of Dropbox issues.
### Re: investor portal next steps — gpt-5

Short version: do not wait on Dropbox. Use a separate private R2 bucket as the active document backend now, and design the database around provider-neutral document metadata so Dropbox can become a later export/sync target instead of the thing the portal depends on.

What is already real: investor auth/role routing exists; available deals load from published admin deals; deal detail loads live deal data; funding commitments persist to `funds`; admin acceptance calls TMO `AddLoanFunding`; positions/dashboard/distributions are derived from accepted fund records and TMO/payment-schedule data. The main unfinished investor-portal surface is document/account data: Documents, Statements, and Accreditation still use static fixtures, and existing borrower loan-document storage is named and wired around Dropbox.

I would implement the remaining work in this order:

1. Add private R2 document storage. Do not reuse `R2_ASSETS`; that bucket is for site/blog assets. Add a separate `R2_DOCUMENTS` binding and a small storage adapter with `put`, `get`, `delete`, and authenticated download/stream behavior. Store keys under predictable private prefixes like `loans/{loan_id}/...`, `deals/{deal_id}/...`, and `investors/{user_id}/...`.

2. Make document metadata provider-neutral. Either migrate `loan_documents` or add a new canonical document table so records can store `storage_provider`, `storage_key`, `content_type`, `file_size`, `sha256`, `owner_user_id`, `loan_id`, `deal_id`, `category`, `visibility`, `review_status`, and optional `external_provider/external_id/external_path`. Existing `dropbox_id` and `dropbox_path` can remain compatibility fields, but new code should not require them.

3. Replace Dropbox-only upload paths with the storage adapter. Borrower uploads and any investor/accreditation uploads should write to R2 first. If Dropbox later becomes available, add a background/manual sync that copies selected R2 objects to Dropbox and records the Dropbox id/path, without changing the portal-facing APIs.

4. Finish investor document APIs. Add authenticated endpoints for investor document lists and downloads, plus statement/accreditation records. Investors should only see documents explicitly visible to them or attached to published deals they can access. Deal due-diligence documents can reuse the admin loan-document source once visibility/category rules exist.

5. Wire the investor UI off live data. Replace `INVESTOR_DOCUMENTS` usage in Documents and Statements, and replace the hard-coded Accreditation status with user/document-backed state. The UI should support: request document, upload/update accreditation evidence, download/view statements and tax docs, and view deal due-diligence packages.

6. Add admin publishing and review controls. Admins need to upload or attach documents, mark them investor-visible, assign category/type, review accreditation files, publish statements/tax docs to a specific investor, and see a clear audit trail of who published or approved what.

7. Harden the file boundary before real users. Enforce Cloudflare/WAF upload size limits, MIME/extension checks, private-bucket-only access, role checks on every download, audit logging, and clear retention/delete behavior. The Tier 0-3 document intelligence pipeline can consume the same R2 object keys later.

8. Verify the full investor lifecycle. Seed a borrower/admin/investor path with the TMO simulator: publish a deal, attach R2 due-diligence docs, view them as investor, submit lender-account data, commit funds, accept the fund as admin, confirm positions/dashboard/distributions update, then publish and download a statement.

The key architecture choice is to make R2 the current source of truth for private uploads and treat Dropbox as an optional downstream integration. That avoids blocking on Dropbox credentials and also gives you a staging layer if Dropbox fails later.
❯ We do not yet have agent integration to perform OCR. Please carve out #next-steps for OCR integration.
❯ We may also need a proper DB. Can the sqlite + graphdb approach work? Is tsift useful?
❯ Should we use convex, neo4j, or another graphdb? Consider costs, effectiveness, and maintenance.
### Re: OCR integration and database choice — gpt-5

I would not make an interactive agent the OCR engine. OCR should be an app-native async pipeline: upload to private R2, enqueue processing, persist structured OCR/extraction results, send uncertain fields to admin review, and only write to TMO after staff approval. Agents can help inspect/review outputs later, but the portal should not depend on an agent session being present to process documents.

Concrete OCR next steps:

1. Add a document-processing schema around the private R2 document store: `documents`, `document_versions`, `ocr_runs`, `document_pages`, `document_chunks`, `extracted_fields`, `review_items`, `entity_mentions`, and optional `entity_edges`. Keep the original R2 key, content hash, content type, provider, run status, retry count, cost, and source page/bounding-box evidence.

2. Add an async state machine: `uploaded -> screened -> queued -> parsed -> extracted -> review_required -> approved/rejected -> indexed -> tmo_writeback_done`. This matches the PRD requirement that uploaded docs are screened, classified, extracted, reviewed, and only then written to TMO.

3. Build a parser adapter, not a single hard-coded provider. Interface: `parseDocument({ r2Key, contentType, documentType }) -> pages/chunks/elements`. Start with a manual/admin-triggered job before automating every upload.

4. Use Ragie Parse first for hard document OCR if budget allows, because it handles scanned PDFs, tables, forms, key-value pairs, signatures, handwritten notes, and bounding boxes. Keep the output in our D1/R2 schema so Ragie is replaceable.

5. Use cheaper Cloudflare-native steps for early screening: file type/size/MIME, basic classification, embeddings/search, and maybe markdown conversion where it is good enough. Do not send every document to expensive OCR/Claude by default.

6. Add a Claude extraction layer only after parse/OCR has produced page text/elements. Claude should produce typed JSON field candidates with citations back to page/box/chunk ids, not opaque summaries.

7. Build the admin review UI before TMO writeback. Required fields: extracted value, normalized value, confidence, source page/snippet, reviewer decision, reviewer note, and target TMO field. Approval should be explicit per field or grouped review packet.

8. Add cost and safety guardrails: per-document page count cap, per-account daily processing cap, provider timeout/retry limits, PII-aware logs, audit trail, and a manual reprocess button.

Database recommendation: keep D1/SQLite plus R2 as the canonical system for now. It is already deployed in this app, and it is the right shape for portal auth, users, loans, documents, funds, review queues, and audit history. For graph-like behavior, start with normal SQL tables: `entities`, `entity_mentions`, and `entity_edges`. That gives us enough to ask, "which borrowers/properties/docs/deals are related?" without running a second production database.

A full graph database should be a derived index later, not the source of truth. The trigger to add one is when SQL edge tables become painful for real workloads: repeated multi-hop relationship questions, graph visualization/analytics, fraud/relationship analysis, or knowledge-graph queries that are central to Judith's document dashboard. Until then, a separate graph DB mostly adds dual writes, sync bugs, auth duplication, backups, and operational cost.

Provider read:

- D1/R2 is the lowest-maintenance path. Current D1 pricing is usage/row based with no active-instance charge, and the paid plan includes large monthly read/write allowances before overage. R2 has 10 GB-month free, low per-GB storage, request-based Class A/B charges, and no egress fee. Sources: [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/), [R2 pricing](https://developers.cloudflare.com/r2/pricing/).
- Cloudflare Vectorize is a good first semantic-search/index layer if we want to stay Cloudflare-native; official examples price even a 50k-vector/200k-query production-ish workload at a few dollars/month before included usage. Source: [Vectorize pricing](https://developers.cloudflare.com/vectorize/platform/pricing/).
- Ragie is useful for OCR/parse/index if we want quality faster than building the pipeline ourselves. Current public pricing shows Starter at $100/month, Pro at $500/month, included page processing, and per-page overage for fast/hi-res processing. Source: [Ragie pricing](https://www.ragie.ai/pricing), [Ragie Parse](https://www.ragie.ai/parse).
- Convex is not a graph database. It is a realtime/backend platform. It could be good for a greenfield app, but adopting it here would mean migrating away from the existing Cloudflare Worker/D1 architecture. Current Professional pricing starts at $25/developer/month, with enterprise minimums much higher. Source: [Convex pricing](https://www.convex.dev/pricing).
- Neo4j is the serious graph option, but not the first move for EFS. AuraDB Professional currently starts at a 1 GB cluster around $65/month and scales from there. Use it only if graph queries become a primary product feature. Source: [Neo4j pricing](https://neo4j.com/pricing/).
- `tsift` is useful for us as a development and agent-navigation tool, not as the production EFS knowledge store. It can help inspect repo/session/docs and maybe prototype extraction ideas, but it is not tenant-authenticated, admin-reviewable, or designed as the client-facing document index.

My recommended architecture is: D1 for canonical records and review state, R2 for private originals and derived artifacts, Ragie Parse or a pluggable OCR provider for hard parsing, D1 tables for extracted fields/entities/edges, and Ragie/Vectorize as the derived retrieval layer. Defer Neo4j/FalkorDB/Convex until a measured search/relationship workload proves that SQL plus a vector/retrieval layer is insufficient.
<!-- agent:boundary:4550b8d0 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:pending -->
- [ ] [#7fx4] OCR ingestion schema/state machine: add provider-neutral document_processing tables for documents, versions, OCR runs, pages/chunks, extracted fields, entity mentions/edges, review decisions, provider cost, and retry/error state.
- [ ] [#d1h2] OCR provider adapter: process private R2 documents through pluggable parsers starting with Ragie Parse or Workers AI/Claude where appropriate; keep original R2 object as source of truth and persist source page/bounding-box evidence.
- [ ] [#f5kq] OCR admin review workflow: show extracted fields with source evidence, confidence, and diffable approval/rejection before any TMO borrower, loan, lender, or loan-application writeback.
- [ ] [#ntcr] Search/index spike: keep D1/R2 canonical and evaluate Ragie index plus Cloudflare Vectorize/SQL edge tables before adopting Neo4j, FalkorDB, Convex, or another separate graph database.
- [ ] [#ck54] Investor portal private document storage: add a separate private R2_DOCUMENTS bucket plus provider-neutral document metadata and authenticated download APIs; keep Dropbox as a later sync adapter.
- [ ] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [ ] [#zjbe] Admin investor document workflow: upload and publish deal due-diligence, investor statements, tax docs, and accreditation files from R2 with visibility and review status controls.
- [ ] [#9vv3] Investor portal end-to-end QA: seed and test investor signup, accreditation, published deal documents, funding commitment, admin acceptance, positions, distributions, and statements with TMO simulator plus R2 documents.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
- [ ] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#kxsx] Form submission pipeline
- [ ] [#njsx] Tier 0: Cloudflare WAF + upload limits
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#yh4b] Admin dashboard (Cloudflare Access auth)
- [ ] [#ft47] Token-based document upload flow
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
<!-- /agent:pending -->

## Review

<!-- agent:review -->
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
<!-- /agent:review -->
