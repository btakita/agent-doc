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
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `734df89a-8af6-401e-83e4-fd5215a48e3f` as of 2026-05-28 from EFS commit `0afd62b`; public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260528-215443.md`*

Compacted content:
- Archived 6 response topic(s): Broker submit-deal LTV field; In /portal-broker/submit-a-deal, use `LTV (%)` and validate the field is a number.; Switch agent to claude; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ re [#2z8r]: How can we allow custom addresses?

### Re: [#2z8r] custom addresses — opus-4-7


Today's `/portal-broker/submit-a-deal` is already a free-text `property_address` input (`src/equityfundingsource-dev/lib/ui--browser--efs/portal/broker/submit_deal.tsx:114`), so "custom addresses" are already accepted there. The real question is how to keep that escape hatch once we wire #2z8r's autocomplete into the same field. The borrower side already shows the failure mode: `PropertyAddressSearch` (`src/equityfundingsource-dev/lib/ui--browser--efs/portal/borrower/new_application.tsx:425`) zeroes every structured field on input and only re-populates them when the user clicks a Census suggestion — type a rural parcel or new-construction address and you cannot advance.

Recommended pattern when we land the spike (design only; #2z8r stays open):

1. **"Use what I typed" affordance.** When the suggestion list is empty (or shows "No standardized matches"), surface a secondary button that accepts the raw string as `subject_property_address` / `property_address`, leaves the parsed `street/city/state/zip/county` empty, and stamps `address_verified=false` (plus `address_source='manual'`) on the row.
2. **Manual entry toggle.** A "Enter address manually" link reveals the four/five split inputs so off-grid parcels, APNs, or unmapped new construction can still capture structured fields without depending on the provider. Same `address_verified=false` flag.
3. **Persist provider metadata when verified.** When a suggestion is selected, keep `address_source='census'` (or whichever provider wins the spike) plus the `place_id`/`smarty_id`/`mapbox_id` and `lat/lng` so the admin pipeline can render a map preview and re-verify later.
4. **Admin pipeline badge.** `/portal-admin` pipeline rows render an "Unverified address" pill for `address_verified=false` so reviewers know to confirm with the broker (parcel ID, county records, attached docs) before underwriting.
5. **Provider choice doesn't change the contract.** Google Places, Mapbox, Smarty, and Census all answer the same way: structured + verified when matched, raw + unverified when not. The spike picks the provider on cost/coverage/free-text fallback quality; the UX/data contract above stays the same.

This keeps autocomplete as the happy path, lets brokers submit anything that has no clean USPS/Census match, and gives admin a clear flag to chase verification rather than silently accepting unparsed strings.

### Re: use recommendation [#2z8r] — opus-4-7


Implemented the borrower-side slice of the recommendation. #2z8r stays open (autocomplete spike — provider eval for broker submit-deal — is the remaining work).

Submodule commit `875038f` (parent bump `56c8228`):

- `lib/ui--browser--efs/portal/borrower/new_application.tsx`
  - `borrower_form_T` gains `address_verified: boolean` and `address_source: '' | 'census' | 'manual'`.
  - `PropertyAddressSearch` now tracks whether the user has searched; when the suggestion list comes back empty (or the lookup fails), shows a "Use what I typed" button alongside an "Enter address manually" toggle. The toggle reveals split inputs for street/city/state/zip/county.
  - Selecting a Census suggestion stamps `address_source='census'`, `address_verified=true`. Both fallback paths stamp `address_source='manual'`, `address_verified=false`. Typing in the search box resets back to unverified blank.
  - A pill renders under the field: green "Verified" for census selections, yellow "Unverified — admin will confirm" for manual entries.
  - `validate_property_` relaxes to accept either full structured parts OR a manual entry with a non-empty `subject_property_address`.
  - Review step adds an "Address Verification" detail row.

- `app/equityfundingsource/lib/loan_applications/routes.ts`
  - `onboarding_data_T` gains the two new fields; both are persisted into `onboarding_data_json` (no D1 migration needed — the column is JSON).
  - Form parser narrows `address_source` to the allowed union; `address_verified` is only honored as `true` when `address_source === 'census'`, so a manipulated client cannot self-attest verification.
  - Submit validation accepts manual entries with a non-empty `subject_property_address` even when structured parts are blank.

- `lib/ui--browser--efs/portal/admin/loan_detail.tsx`
  - Parses `onboarding_data_json` once via `parse_address_summary_` and renders a new "Subject Property" row in the Borrower Info card with the verification pill. Existing borrower mailing address row is relabeled "Mailing" to disambiguate. The full onboarding JSON dump below still shows the raw fields.

Verification:
- `make check` → 212 pass / 0 fail; build (browser + worker bundle) green.
- `bun test tests/fixtures.test.ts` → 2 pass.
- Existing fixture intentionally untouched — represents legacy data, defaults to `address_source=''` / `address_verified=false`, and still satisfies the relaxed validator because it carries full structured parts.

What this does NOT include (full #2z8r):
- Provider spike (Google Places vs. Mapbox vs. Smarty vs. Census) for `/portal-broker/submit-a-deal`.
- Wiring autocomplete into broker `submit_deal.tsx` (still plain free-text there — broker addresses already flow as "manual" by behavior, just without the explicit flag in this slice).
- Provider metadata persistence (`place_id`, `lat/lng`, map previews).

When #2z8r picks a provider, the UI contract above carries over: provider-matched → `address_verified=true`; everything else → manual + unverified, badge in admin.

### Re: use recommendation [#2z8r] — opus-4-7


Implemented the borrower-side slice of the recommendation. #2z8r stays open (autocomplete spike — provider eval for broker submit-deal — is the remaining work).

Submodule commit `875038f` (parent bump `56c8228`):

- `lib/ui--browser--efs/portal/borrower/new_application.tsx`
  - `borrower_form_T` gains `address_verified: boolean` and `address_source: '' | 'census' | 'manual'`.
  - `PropertyAddressSearch` now tracks whether the user has searched; when the suggestion list comes back empty (or the lookup fails), shows a "Use what I typed" button alongside an "Enter address manually" toggle. The toggle reveals split inputs for street/city/state/zip/county.
  - Selecting a Census suggestion stamps `address_source='census'`, `address_verified=true`. Both fallback paths stamp `address_source='manual'`, `address_verified=false`. Typing in the search box resets back to unverified blank.
  - A pill renders under the field: green "Verified" for census selections, yellow "Unverified — admin will confirm" for manual entries.
  - `validate_property_` relaxes to accept either full structured parts OR a manual entry with a non-empty `subject_property_address`.
  - Review step adds an "Address Verification" detail row.

- `app/equityfundingsource/lib/loan_applications/routes.ts`
  - `onboarding_data_T` gains the two new fields; both are persisted into `onboarding_data_json` (no D1 migration needed — the column is JSON).
  - Form parser narrows `address_source` to the allowed union; `address_verified` is only honored as `true` when `address_source === 'census'`, so a manipulated client cannot self-attest verification.
  - Submit validation accepts manual entries with a non-empty `subject_property_address` even when structured parts are blank.

- `lib/ui--browser--efs/portal/admin/loan_detail.tsx`
  - Parses `onboarding_data_json` once via `parse_address_summary_` and renders a new "Subject Property" row in the Borrower Info card with the verification pill. Existing borrower mailing address row is relabeled "Mailing" to disambiguate. The full onboarding JSON dump below still shows the raw fields.

Verification:
- `make check` → 212 pass / 0 fail; build (browser + worker bundle) green.
- `bun test tests/fixtures.test.ts` → 2 pass.
- Existing fixture intentionally untouched — represents legacy data, defaults to `address_source=''` / `address_verified=false`, and still satisfies the relaxed validator because it carries full structured parts.

What this does NOT include (full #2z8r):
- Provider spike (Google Places vs. Mapbox vs. Smarty vs. Census) for `/portal-broker/submit-a-deal`.
- Wiring autocomplete into broker `submit_deal.tsx` (still plain free-text there — broker addresses already flow as "manual" by behavior, just without the explicit flag in this slice).
- Provider metadata persistence (`place_id`, `lat/lng`, map previews).

When #2z8r picks a provider, the UI contract above carries over: provider-matched → `address_verified=true`; everything else → manual + unverified, badge in admin.
do [#2z8r]. #spec-test-commit-push-deploy
<!-- agent:boundary:a7f7248b -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue auto -->
- do [#2z8r]. #spec-test-commit-push-deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#2z8r] [recommended] Address autocomplete spike for /portal-broker/submit-a-deal Property address: compare Google Places vs. Mapbox vs. Smarty (per-request cost, accuracy, free-text fallback for off-grid parcels, lat/lng persistence, map previews in admin pipeline). Pick one, wire it to the existing free-text input with normalized address + lat/lng captured alongside the raw string.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
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
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
