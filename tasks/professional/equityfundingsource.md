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
<!-- agent:boundary:3298dfd8 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:pending -->
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
