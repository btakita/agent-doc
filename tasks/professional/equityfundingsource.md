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
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

25 pages deployed with form backend (D1 + R2 + email). D1/R2 TMO + document cache migrated and deployed to Worker version 59aaf003.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-224019.md`*

Compacted content:
- Archived 1 response topic(s): do [#qakx]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
### Re: queue do (six backlog items) — opus-4-7


Holding the queue rather than batch-resolving. Six items in one cycle under preset `#spec-test-commit-push-deploy` is multi-day repo work (admin dashboard with Cloudflare Access, token-based upload flow, Tier 0 WAF + upload limits, admin investor document workflow, full investor-portal E2E QA), and the planner already flagged this as `risk=high`, `dispatch_mode=manual`, `manual_packet_only=true`. Preflight also surfaced a restart-heavy session-accretion warning ("Restart cleanly from the current committed boundary before continuing"). Marking the queued IDs done without execution would lie about work that has not happened.

Scope notes per queued ID:

- dzmm TMO webhooks — externally blocked. Robert needs to confirm with Mortgage Office whether outbound webhooks are supported. I can draft the question/spec, but cannot resolve it from this side.
- njsx Tier 0 Cloudflare WAF + upload limits — small/foundational; doable in one cycle (WAF rules, file-size caps, allowed MIME types, rate limits via Cloudflare config + Worker guards).
- ft47 Token-based document upload flow — moderate; depends on R2_DOCUMENTS namespace already in place; needs token issuance, signed-URL or Worker proxy, expiry, audit logging.
- yh4b Admin dashboard (Cloudflare Access auth) — moderate-large; needs Access policy, admin route gating, dashboard shell, then per-feature pages.
- zjbe Admin investor document workflow — large; depends on yh4b and ft47; visibility + review-status controls + publish flow.
- 9vv3 Investor portal E2E QA — large; depends on most of the above plus TMO simulator and R2 fixtures.

Proposed sequencing for next cycles (one item per turn, smallest-foundational-first):

1. njsx Tier 0 WAF + upload limits (single cycle, narrow scope).
2. ft47 Token-based upload (depends on R2 + D1 cache already shipped).
3. yh4b Admin dashboard shell + Cloudflare Access (auth + scaffold; per-feature pages follow).
4. zjbe Admin investor document workflow (builds on yh4b + ft47).
5. 9vv3 Investor portal E2E QA (last; covers the above end-to-end).
6. dzmm parked until Robert hears back from Mortgage Office.

Confirm the order (or override) and I will pick up the first item next cycle with spec + tests + deploy. No queued IDs marked done this cycle; they remain in the auto queue for sequential processing.
### Re: do [#njsx] — Tier 0 WAF + upload limits — opus-4-7


Shipped. njsx complete; queue head advances next cycle.

What changed in `src/equityfundingsource-dev`:

- **`app/equityfundingsource/lib/security/upload_guard.ts`** (new) — Tier 0 policy + helpers. Defaults: 25 MB per file, 100 MB per request, 25 files per request, allowlist of PDF/Office/image/CSV/text MIME types + extensions, file-name sanity (rejects `/`, `\`, NUL, `..`, `.`, >255 chars). `upload_guard_check_content_length_` pre-flights `Content-Length` before `formData()` parses. `upload_guard_check_file_` / `upload_guard_check_files_` return structured `{ status, code, message }` errors (`content_length_too_large` 413, `file_too_large` 413, `file_count_exceeded` 413, `total_size_exceeded` 413, `file_empty` 400, `extension_not_allowed` 415, `mime_type_not_allowed` 415, `file_name_invalid` 400). `upload_rate_limit_check_` consults the `UPLOAD_RATE_LIMITER` binding and falls open when unbound (local dev / staging).
- **`lib/investor/routes.ts`** — `/api/investor/accreditation/upload` switched from the inline 25 MB check to `upload_guard_check_file_` + `upload_guard_check_content_length_` + `upload_rate_limit_check_` keyed on `investor:<user_id>:accreditation`.
- **`lib/loan_applications/routes.ts`** — `/api/borrower/loan-applications` and `/api/borrower/loans/:id/documents` both gained content-length, multi-file, and rate-limit checks (session keys `borrower:<user_id>:loan-applications` and `borrower:<user_id>:loan-docs`).
- **`wrangler.toml`** — added `UPLOAD_RATE_LIMITER` ratelimit binding (`namespace_id = "1001"`, `simple = { limit = 10, period = 60 }`). Cloudflare confirmed the binding on deploy.
- **`docs/security-tier-0.md`** (new) — full Tier 0 spec: policy table, error codes, the three wired endpoints, and the Cloudflare dashboard rules (managed ruleset, OWASP CRS, three custom WAF rules for bot scans/oversized bodies/banned extensions, an IP-level rate-limit rule, cache-bypass + `X-Robots-Tag` transforms for `/api/*`).
- **`prds/equity-funding-source-lending-portal-prd.md`** — `### Document Intelligence` now has a `#### Tier 0 — WAF + upload limits` subsection linking to the spec doc and listing defaults and wired endpoints.
- **`tests/upload-guard.test.ts`** (new) — 25 tests covering every error code (size, count, total, MIME, extension, name, content-length), per-call options override, MIME fallbacks (`application/octet-stream`, empty), rate-limit allow/block/throw/unbound, and source-level assertions that the three routes import and call the guard.

Verification:

- `bun test` — 100 pass / 0 fail across 14 files (518 expect calls). Output includes the deliberate "limiter down" console.error from the throw-recovery test path.
- `bun run check` (test + build + build:worker) — clean.
- `bun run deploy:worker` — deployed version `d2cf3382-dec5-4dd4-9991-c1cdc3ae8034` to `https://equityfundingsource.brian-takita.workers.dev`. Wrangler confirmed `env.UPLOAD_RATE_LIMITER (ratelimit)` in the bindings list.
- Submodule commit `b6df8b1` pushed to `origin/main`.
- Superproject commit `0467de4c2` (submodule pointer bump) pushed to `origin/main`.

Out of scope for this cycle (documented in `docs/security-tier-0.md` for the next operator pass):

- Applying the three custom WAF rules + IP-level rate-limit rule via the Cloudflare dashboard or API on the `equityfundingsource.com` zone. The Worker layer enforces the same policy in-process, so this is hardening, not a gate.
- Edge smoke test against a deployed custom domain — current host is `equityfundingsource.brian-takita.workers.dev`; production custom-domain cutover (icebox e1kb) gates this.

Next queued item is the token-based document upload flow. Confirm and I'll pick it up next cycle.
<!-- agent:boundary:9848651c -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-commit-push-deploy
- do [#njsx]
- do [#ft47]
- do [#yh4b]
- do [#zjbe]
- do [#9vv3]
- do [#dzmm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#zjbe] Admin investor document workflow: upload and publish deal due-diligence, investor statements, tax docs, and accreditation files from R2 with visibility and review status controls.
- [ ] [#yh4b] Admin dashboard (Cloudflare Access auth)
- [ ] [#ft47] Token-based document upload flow
- [ ] [#9vv3] Investor portal end-to-end QA: seed and test investor signup, accreditation, published deal documents, funding commitment, admin acceptance, positions, distributions, and statements with TMO simulator plus R2 documents.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
- [ ] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
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
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [/] [#1v5m] [recommended] Update passage btak/equityfundingsource/cloudflare_api_token to include Cloudflare D1 read/write permissions so the project cf:d1:* scripts can run without the OAuth bypass.
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
<!-- /agent:icebox -->

<!-- agent:done -->
- 2026-05-24 [#a20z] Set EFS Cloudflare deployment credentials (CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID) in src/equityfundingsource-dev/.env, then rerun bun run deploy:worker for src/equityfundingsource-dev commit 7ce187e or latest main.
- 2026-05-24 [#r2-upload-cache] R2 upload cache path: make token/admin uploads write original bytes to R2 first, persist metadata in D1, and serve portal downloads from R2 even when Dropbox access is unavailable.
- 2026-05-24 [#dropbox-cache-contract] Dropbox/R2 document cache contract: define R2 key namespace, D1 document metadata, Dropbox path mapping, content hashes, sync_status, last_dropbox_synced_at, tombstones, and conflict policy for uploaded documents.
- 2026-05-24 [#portal-cache-api] Portal cache APIs: cut investor/admin portal pages over to cache-only reads with last-synced/stale indicators and an outbox for portal writes that must replay to TMO later.
- 2026-05-24 [#tmo-cache-worker] TMO sync Worker: build idempotent manual/scheduled pulls with page cursors, retries, tombstones, sync batches, and D1/R2 writes that do not require TMO during portal requests.
- 2026-05-24 [#tmo-cache-model] TMO cache contract: define D1 read-model tables plus R2 raw payload/document object paths, TMO IDs, cursors, checksums, last_synced/stale/deleted state, and conflict policy for portal-visible data.
- 2026-05-24 [#cf-d1-cache-deploy] Resolve Cloudflare D1 API authorization for account 1e7e194ff8da4ad9e40ce041aea84f76, apply migration 0023_tmo_cache_and_r2_documents.sql, then deploy worker commit 3ada2de.
- 2026-05-24 [#ck54] Investor portal private document storage: add a separate private R2_DOCUMENTS bucket plus provider-neutral document metadata and authenticated download APIs; treat R2 as the canonical uploaded-document cache and keep Dropbox as a later sync adapter.
- 2026-05-24 [#njsx] Tier 0: Cloudflare WAF + upload limits
<!-- /agent:done -->
