---
name: reference_efs_site_url_unset_use_request_origin
description: "EFS SITE_URL is NOT set in production [vars]; emailed links must fall back to request origin or they render hostless"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 65399167-ace9-4135-bda4-4cdb24f29a52
---

EFS production has **no `SITE_URL`** in `wrangler.toml [vars]` (verified in deploy env dump 2026-07-09). Any code that builds an emailed/CTA link as `env.SITE_URL ? new URL(path, SITE_URL) : path` therefore emits a **hostless relative path** — which Gmail mangles into `http:///lenders/deals/...` (empty host).

**Fix pattern (used across the codebase):** resolve base as
`env.SITE_URL?.trim() || new URL(c.req.url).origin || 'https://www.equityfundingsource.com'`,
then always `new URL(path, base)`. Request origin on prod = `https://equityfundingsource.brian-takita.workers.dev`.

Reference impls: `admin/payment_management/index.ts` `site_url_()`, `admin/application_leads/index.ts` `site_url_()`, `admin/loan_management/index.ts` `lifecycle_env_()`. Deal-packet fix: `deal_packet/routes.ts` `request_origin_()` (#dpkt-portal-abs-url).

When auditing a new email/notify route, grep for `SITE_URL?.trim()` with no origin fallback — that's the bug shape.

**Same root cause bites email assets:** `email/templates/lifecycle.ts` `EFS_EMAIL_LOGO_URL = '/assets/images/logo-nav-640.webp'` is relative → broken `<img>` in inboxes. Fixed via `email_logo_url_(base_url)` = `new URL(EFS_EMAIL_LOGO_URL, base_url)`, threaded through `layout_input_T.logo_url`; the packet email passes `packet_url` (absolute) as the base. Logo asset serves 200 only at the workers.dev origin — `www.equityfundingsource.com` does not resolve yet. Two brittle source-snapshot tests assert the footer impl verbatim: `admin-pipeline.test.ts` "centered EFS logo footer" and `admin-investor-pipeline.test.ts` "EFS letterhead" — update them when the footer signature changes.

**Deal-packet recipients dedupe (#dpkt-dedupe-recipients):** `deal_selected_recipients_R_` joins `lender_deal_interests` and can return duplicate rows for one lender → N emails. Fixed with `SELECT DISTINCT` + a lowercased-email `seen` Set in `notify_lender_deal_packet_`. Packet email subject uses property address, not loan number.

**Lender packet deep link (#dpkt-deep-link-authoritative):** `/lenders/deals/<id>?tab=packet` — the login 303 preserves `next=...%3Ftab%3Dpacket`, and `investor/deal_detail.tsx` now treats the deep link as authoritative (shows the Deal Packet tab even before `selected_for_commitment` confirms). Packet view = property title + `DealPhotoCarousel` + deal documents + request-more panel.

**Pre-existing baseline failure (not mine):** `borrower-property-photos-portal-merge.test.ts` "admin deal_photos retired" fails because `deal_marketplace/index.ts` still contains `[...admin, ...borrower]` — concurrent-session incomplete work, unrelated to deal-packet edits.
