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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260619-051517.md`*

Compacted content:
- Archived 9 response topic(s): What is "Funding Commits" in the Deal Pipeline dialog?; Is the Funding Commit driven by TMO?; Document the entire Deal Pipeline flow (all roles + Broker variant); 6 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): Why "Application progress" doesn't show the accepted step; Ho...

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed the EFS Worker to production via the canonical `make deploy` path (build → apply remote D1 migrations → upload Worker), per `runbooks/deployment.md`.

**Pre-deploy:** submodule clean, on `main`, synced with `origin/main` (0/0); no pending D1 migrations; `make check` green — **822 pass, 0 fail**, `build` + `build:worker` succeeded.

**Deploy:** `make deploy` uploaded `equityfundingsource` (8.67s). **Version ID `73dd0840-8196-4b39-9f54-d016468b56a7`**. No new asset files to upload; no migrations to apply.

**Post-deploy verification:**
- Live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**.
- Migrations pending → **none**.

The recent doc-only changes (bi-directional TMO documentation) carried no code/schema delta, so this ships the current `main` Worker state cleanly.

### Re: TMO writeback "unavailable" — error placement (above button + toast) and root cause — opus-4-8

> **Queue prompt:**
>
> :pushpin: Why is there a "TMO writeback not sent: unavailable" error?


> **Queue prompt:**
>
> :pushpin: Admin > Deal Pipeline > Full Application dialog > Accept and send to TMO got an error "TMO writeback not sent: unavailable". The error message should appear above the button and there should be a toast.


> **Queue prompts:** (1) Accept and send to TMO error "TMO writeback not sent: unavailable" should appear above the button + a toast. (2) Why is there a "TMO writeback not sent: unavailable" error?

**(1) Placement + toast — done.** The "Accept and send to TMO" result now renders **inside the decision card, directly above the action buttons** (green note on success, red error on failure) and also fires a **toast** (`add_toast`, success/error) via the shared `ToastContainer`. Previously the only inline banner sat at the top of the detail page, which scrolls out of view inside the embedded Deal-Pipeline dialog — so a failure looked like nothing happened near the button. (`lib/ui--browser--efs/portal/admin/full_applications.tsx`: import `add_toast`; `approve_()` pushes the toast + sets the inline message; `decision_card` renders `note`/`error` above the button row.)

**(2) Why "unavailable".** It is **not** anything the server sends — it's the admin UI's last-resort string. `approve_()` does `const d = await res.json().catch(() => null)` then `set_error(\`TMO writeback not sent: ${d?.reason ?? d?.error ?? 'unavailable'}\`)`, so `unavailable` only appears when **`d` is null — the approve endpoint returned a body the client could not parse as JSON.** Every *controlled* failure (`not found`, `draft`, `not linked`, incomplete application, a rejected `NewLoan`) already returns JSON with a real `error`. The opaque `unavailable` meant an **unhandled exception escaped as a non-JSON 500**: `application_tmo_writeback_attempt_` ran its prelude **outside** its `try/catch` — the loan-number generator hits **live TMO** (`tmo_live_loan_R_`, which re-throws on any non-404/not-configured error), plus several D1 reads/writes. With TMO unreachable (flaky/gated per #dzmm) any throw bypassed the JSON path → 500 → `d=null` → `unavailable`.

**Fix:** wrap the entire attempt in one `try/catch` so the endpoint **always** returns structured JSON `{ ok:false, status:'failed', error }` (502); the catch marks failed best-effort (`.catch(() => null)`) so a D1 hiccup can't regress into a non-JSON 500. Admins now see the real reason — a TMO connectivity message, or for an already-associated loan like deal `7a0c1463` / `GQI751`, TMO's `NewLoan failed with 400: Duplicate loan number` (the duplicate-loan handling itself stays tracked in #5q7z).

**Verification:** `make check` green — **824 pass, 0 fail**. Committed + pushed: EFS `e869e73` (origin/main), superproject pointer `5cd5c7c56`. The EFS repo has no CI workflow, so local `make check` is the verification of record. Not deployed (preset was commit+push, not deploy); the UI change ships on the next `make deploy`.

### Re: Why "TMO writeback not sent: unavailable" (already answered above) — opus-4-8

> **Queue prompt:**
>
> :pushpin: deploy error message fix


> **Queue prompt:** Why is there a "TMO writeback not sent: unavailable" error?

Answered in the previous response (section **(2) Why "unavailable"**). In short: `unavailable` is the admin UI's last-resort fallback string, not a server message — it shows only when the `/approve` response can't be parsed as JSON (`res.json().catch(() => null)` → `null`). That happened because `application_tmo_writeback_attempt_` ran its prelude (live-TMO loan-number generation + D1 reads/writes) **outside** its `try/catch`, so a throw there escaped as a non-JSON 500. Fixed by wrapping the whole attempt so it always returns structured JSON `{ ok:false, error }`; admins now see the real reason. Shipped in EFS `e869e73` (origin/main), superproject `5cd5c7c56`, `make check` 824/0.

### Re: deploy error message fix — opus-4-8

> **Queue prompt:**
>
> Once an application is accepted by the admin, it should immediately be published. deploy


> **Queue prompt:** deploy error message fix

Deployed the EFS Worker to production via `make deploy`. The build picked up the `#tmowbjson` TMO-writeback fix (always-JSON `/approve` + inline error/toast above the Accept button) — wrangler uploaded the rebuilt frontend bundle **`index.browser-XPUZTLSF.js`** (1 new asset; 43 unchanged).

**Pre-deploy:** EFS submodule clean, on `main`, synced with `origin/main` (0/0); HEAD `e869e73` (the fix). No pending remote D1 migrations (`No migrations to apply!`).

**Deploy:** `bunx wrangler deploy` uploaded `equityfundingsource` (8.17s), Worker startup 28 ms, all bindings intact (DB, R2 ×3, Durable Objects, Access env).

**Post-deploy verification:** live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**. The admin Deal-Pipeline "Accept and send to TMO" flow now surfaces the real failure reason (structured JSON) with the message above the button + a toast, instead of the opaque "unavailable".

### Re: Auto-publish on acceptance + deploy — opus-4-8

> **Queue prompt:** Once an application is accepted by the admin, it should immediately be published. deploy

Implemented and deployed. When the admin accepts a full application (`POST /api/admin/full-applications/:id/approve` → TMO writeback succeeds, loan persisted, application → `accepted`), the loan is now **immediately published to the investor marketplace** as an Available Deal.

**Change (`#accept-autopublish`, EFS `d6c6651`):** `application_tmo_writeback_attempt_` calls `admin_deal_publish_(db, loan_id)` right after acceptance. It's **best-effort** — a publish hiccup is caught + logged and does NOT undo the successful TMO writeback/acceptance (re-publishable via `POST /api/admin/loans/:id/deal`); the approve response now carries a `published` boolean. `admin_deal_publish_` is idempotent (upserts the `investment_deals` row, computes terms/LTV). This is the complement of review #214f ("publish only when accepted") — accepted now implies published.

**Verified:** spec + test added (`#accept-autopublish` ordering: accept → publish → return, best-effort); worker build `done`; `make check` full suite **825 pass, 0 fail**.

**Deployed:** `make deploy` → no D1 migrations; uploaded `equityfundingsource` (Worker startup 21 ms); live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**. Pushed: EFS `d6c6651` (origin/main), superproject pointer `541f9a7bb`.

### Re: FTM631 still not published — keep Published, just backfill this one loan — opus-4-8

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=loan&dialog_loan_id=bbd437b0-913f-4a6f-960b-901c72a410c9 is still not published. Should we remove the Published state or make it computed based on the application being accepted?


I queried prod D1. Here's what's actually going on:

- App `94fd3f07` → `status = accepted` (accepted ~02:33am)
- Loan `bbd437b0` / **FTM631** → `tmo_loan_status = Application Received`, **no `investment_deals` row** (`deal_id`, `deal_status`, `published_at` all null) → not in the marketplace.

It was accepted **~8 minutes before `#accept-autopublish` deployed** (~02:41am), so the new auto-publish hook never ran for it. I checked the whole table: **exactly one** accepted loan has no deal — this one. Every acceptance *going forward* auto-publishes, so this is a one-time gap, not a recurring bug.

**Recommendation: keep the stored Published state — don't remove it, don't make it purely computed.** "Published" is not the same fact as "accepted":

1. The `investment_deals` row is its own entity with a lifecycle **published → funded → closed**. `funded` (`tmo_loan_status = Funded`) and `closed` (deal closed, or a loan-lifecycle change closes it) are **not** derivable from `application.status = accepted`. A computed-from-accepted flag loses the ability to close/withdraw a deal from the marketplace while the application stays accepted, and has no place for funded/closed.
2. The row **snapshots deal terms** at publish time — `sold_rate`, `min_investment_amount`, `term_months`, LTV — pulled from the TMO cache + platform settings. A computed-on-read flag has nowhere to store these and would re-derive them every read against live TMO-cache freshness.
3. Publishing fires a **side-effect** (the deal-published lender notification email) and stamps `published_at` for marketplace ordering. A boolean has no firing edge for "newly published."

So removing it or fully computing it would actually regress the deal lifecycle. The thing the "computed" idea is reaching for — *consistency* — is better gotten two ways:

- **Now (the real fix):** one-time backfill of FTM631. Two options:
  - `admin_deal_publish_(db, 'bbd437b0-…')` directly → **quiet**, no lender email (this is exactly what the auto-publish path does).
  - `POST /api/admin/loans/bbd437b0-…/deal` → publishes **and** emails lenders (`deal-published-email`).
- **Optional follow-up:** add a reconcile affordance in the pipeline loan dialog — when an app is accepted but has no published deal, surface a one-click **Publish** (or auto-reconcile on open). That gives you the consistency without throwing away the stored lifecycle.

This is a prod write that makes the loan publicly visible in the investor marketplace, so I haven't run it. **Want me to backfill FTM631 — quiet (no email) or with the lender notification?** Captured as `#ftm631-publish-backfill`.

### Re: do #e9fy — FTM631 one-time publish backfill (quiet) — opus-4-8

> **Queue prompt:**
>
> do [#e9fy]

Done. Backfilled the missing `investment_deals` row for **FTM631** (loan `bbd437b0`) so it's now an Available Deal in the investor marketplace. I used the **quiet** path — no lender email — exactly replicating what `#accept-autopublish` would have done on acceptance, which is the gap this loan fell through.

**What I did (faithful to the auto-publish path, not POST /deal):** the auto-publish hook calls `admin_deal_publish_(db, loan_id)` with **no terms input** (`applications/index.ts:1137`), so it uses `DEFAULT_PLATFORM_SETTINGS`. Traced through `deal_terms_` + `gformula.ts` against the live loan, that produces:

- `status = 'published'` — loan `tmo_loan_status = 'Application Received'` (not `Funded`).
- `term_months = 12` (constant).
- `sold_rate = NULL` — empty terms input means `sold_rate_calc_(undefined, 1)` is null. (The richer POST /deal path would pull TMO `SoldRate`/`NoteRate`, but that path also emails lenders; auto-publish does not.)
- `min_investment_amount = 10000` — default `constant` mode.
- `ltv = NULL` — onboarding has `proposed_loan_amount=100000` but **no `appraised_value`/`purchase_price`**, so `loan_ltv_calc_` returns null.

**Executed** as a single raw INSERT against prod D1 (`equityfundingsource`, `280e1539…`) via the `cf:d1:query:remote` wrapper — no `notify_email_`, so **no lender notification fired**:

- New deal `d6b78994-0970-498f-9e10-6f0035902c0d`, `published_at = 1781852961` (now), `created_at`/`updated_at` matched, `closed_at` NULL.
- `changes: 1`; verified the row reads back with the exact field set above and joins to FTM631.

**Pre-flight confirmed exactly one gap, as expected:** before the write, `deal_count = 0` for this loan (INSERT branch, not the UPDATE branch), consistent with the earlier finding that FTM631 was the single accepted loan with no deal. Stored Published state is preserved — I did **not** remove it or make it computed.

**Scope notes:** this is a one-time data backfill, not a code change — the EFS working tree is clean, so the queue preset's `#spec-test-commit-push` steps (spec/test/commit/push) are N/A; there's nothing to commit or deploy. (Separately, preflight flagged a `stale_install` for the agent-doc binary itself — unrelated to EFS; leaving it for a post-session `make install` rather than rebuilding against the live supervisor.)

### Re: Document preview on /lenders/documents — opus-4-8

> **Queue prompt:**
>
> :pushpin: Documents in https://equityfundingsource.brian-takita.workers.dev/lenders/documents should have preview. deploy


Preview is already implemented and was already live — I verified it end-to-end on the real page, then redeployed to guarantee current HEAD is on production.

**What I verified (live, logged in as DL Dude Lender):** clicking the "Preliminary Title Report" PDF on https://equityfundingsource.brian-takita.workers.dev/lenders/documents opens the in-browser viewer modal — an `<iframe>` rendering the PDF from a `blob:` object URL, no forced download. This is the `#doc-viewer` / `#c7cs` click-to-view (`DocumentFileLink` → `use_document_viewer_`) shipped 2026-06-14; it covers images, PDFs, and text inline with a Download fallback.

**Deploy:** the previous deploy's worker finished ~16s after the latest commit (`d6c6651`), so I ran a fresh `deploy:worker` to remove all doubt. Result: `No migrations to apply` (D1 already current), `No updated asset files to upload` (the preview frontend bundle was already live — consistent with the verification above), worker redeployed at version `54534f34`. Post-deploy health: home `200`, `/lenders/documents` `303` (auth redirect, expected unauthenticated).

**One gap I noticed (queued as `[recommended]`):** the page renders only *provided-from-request* documents. `/api/investor/documents` also returns the investor's published `documents` bucket (statements/tax/legal) and `public_documents`, but `documents.tsx` reads only `requests` and drops both — so a lender with published or public docs wouldn't see (or preview) them here. For this account only the provided Title Report exists, so nothing's missing today, but rendering those two buckets with the same `DocumentFileLink` preview would complete the page.

### Re: #3tek — render published + public documents with preview — opus-4-8

> **Queue prompt:**
>
> do [#3tek]


Done, deployed, and verified live. The `/lenders/documents` page now shows every document bucket the API returns, each with the `#doc-viewer` click-to-preview.

**Change** (`documents.tsx`, committed `9344962`, pushed): the page previously consumed only `requests` and dropped the `documents` (the investor's published statements/tax/legal/accreditation) and `public_documents` buckets the API already returned. Added two sections:
- **Your Documents** — the investor's published documents, each a `PublishedDocumentCard` → `DocumentFileLink` preview (with a non-linked "Not available yet" state for docs without a file).
- **Public Documents** — `PublicDocumentCard` → `DocumentFileLink` preview (hidden when empty).

Provided-from-request documents keep their existing section. Section ids (`#investor-published-documents`, `#investor-public-documents`) are asserted by new source-contract tests in `investor-documents.test.ts`.

**Verification:** `bun run check` green (825 tests pass, build + worker build clean). Deployed worker version `acba09f6`. Verified live as DL Dude Lender: the "Your Documents" section now surfaces the lender's previously-hidden accreditation evidence (`tax-k1-2025.pdf`, `account-statement.pdf`), and clicking one opens the in-browser viewer — `<iframe>` rendering the PDF from a `blob:` URL, no forced download, no error.

### Re: Remove commitment calculator + lender grouping mechanism — opus-4-8

> **Queue prompt:**
>
> :pushpin: Please create a mechanism for lenders to group deals together. A lender could invite another lender to join a deal. What would this group be called? Should we have a lender group? Should we have a lender be able to start a lender group and add other lenders to the group?


> **Queue prompt:**
>
> :pushpin: In https://equityfundingsource.brian-takita.workers.dev/lenders/available-deals Deal detail, Remove the Committment Calculator section. Each loan is for full value. There can be multiple lenders for a deal, but those lenders already know each other.


**1. Commitment calculator removed** — done and pushed (EFS `851eb0b..2399f88`). On `/lenders/available-deals`, the Deal-detail right column no longer shows the per-lender commitment calculator (amount input, EFS-locked proposal preview, position-type split). It's replaced by a read-only **Funding terms** panel: monthly interest, annual yield, a note that each loan funds at full value and co-lenders coordinate directly, and the **Open deal package** CTA. The real funding-commitment flow and proposal-amount locking are untouched — they live on the deal-package page (`deal_detail.tsx`) and marketplace. Dropped the now-dead `annual_yield_calc_`/`position_type_label_` helpers and updated the e2e assertions.

**Heads-up — the base shipped red.** A concurrent merge (`851eb0b`) landed on `origin/main` mid-cycle with a broken suite (749 pass / 5 fail + 3 errors), independent of my change. I repaired it in a separate commit (`276754a`): restored `admin_deals_by_id_borrower_selected_proposals_read_PUT_` (still wired in `admin/routes.ts` but its definition was dropped); put `blended_rate`/`selected_commitment_amount`/`tmo_updated` back into the select-lender response; dropped a duplicate `admin_repo_url` decl; and aligned a stale `LOAN_STATUS_STEPS` assertion to the current 8-step pipeline. `bun run check` now green: 831 pass / 0 fail, build + worker build clean.

**2. Lender grouping** — recommendation + plan committed (`src/equityfundingsource-dev/prds/lending-groups-plan.md`, `2399f88`); queued as gated backlog `#8667`. Answers: (a) **Name → "Lending group"** — plain, matches your "lender group" wording, avoids "pool"/"syndicate" which imply fractional/pooled capital (conflicts with full-value loans). (b) **Yes, a persistent group** — co-lenders who already know each other and repeat-invest benefit from a reusable named group over per-deal re-invites. (c) **Yes, a lender starts a group and adds others** — creator is the **lead** (invite/remove/rename), added lenders are **members**; invite by email (existing lenders match by email, new ones get a sign-up link); a group pulls itself into a deal via the existing `investor_deal_interests` flow so co-lenders share the deal package + conversation.

I did **not** build the schema yet — three one-way-door decisions to confirm first: **Name** ("Lending group" recommended / "Syndicate" / "Lender group" / other); **Scope** (persistent reusable group recommended / deal-scoped invite only / both); **Privacy** (do members see each other's proposal amounts/rates, or only that they share the deal? — recommend shared package + identities, not economics unless the lead opts in). Reply with your picks and I'll build it.

### Re: deploy — opus-4-8

Deployed. Worker version `034efce3-cbb2-436d-b963-9975df1df624` is live at https://equityfundingsource.brian-takita.workers.dev (build + remote D1 migrate clean, new browser bundle uploaded). Verified the calculator removal in the deployed artifact: the production JS bundle has zero `investor-commitment-amount` and zero `Commitment calculator`, and now contains the `Funding terms` panel + the "Each loan funds at full value" note. Remaining items unchanged — lending-groups (`#8667`) stays gated on your name/scope/privacy picks.

### Re: Fix test suite + "Sold rate %" — opus-4-8

> **Queue prompt:**
>
> :pushpin: What is "Sold rate %"?


> **Queue prompt:**
>
> :pushpin: Fix test suite


**Test suite — green.** After fast-forwarding to `origin/main` (`af3f5e1`), `bun run check` is **835 pass / 0 fail**, build + worker build clean. The failures came from the `851eb0b` "Merged 9344962 with 859a1a0" merge landing red; my merge-repair commit (`276754a`, already on origin) restored the dropped `admin_deals_by_id_borrower_selected_proposals_read_PUT_`, put `blended_rate`/`selected_commitment_amount`/`tmo_updated` back in the select-lender response, removed a duplicate `admin_repo_url` decl, and fixed a stale `LOAN_STATUS_STEPS` assertion. There's no CI workflow wired in this repo, so local `bun run check` is the gate. If you're still seeing a red suite somewhere, tell me where (a specific file, or `bun test <path>` output) and I'll chase it — from a clean checkout it's green.

**"Sold rate %"** is the annual interest rate a lender offers to fund the loan at — the rate the loan note is effectively *sold* to that lender. On a deal's interest form (`/lenders/deals/<id>`) the lender submits a **Committed amount** + a **Sold rate %**; together those are their funding proposal (`proposal_interest_rate` in the data). EFS and the borrower compare lender candidates by amount + sold rate. When EFS selects the funding lender(s), the amount-weighted blend of their sold rates becomes the deal's `sold_rate` and is pushed to TMO without changing the loan amount (that's the blending logic in the select-lender flow). In short: it's the lender's quoted yield / the borrower's interest cost for the funded note — distinct from the borrower's "Preferred sold rate," which is the rate the borrower is *asking* for.

Lending-groups (`#8667`) stays gated on your name / scope / privacy picks.

### Re: All upload preview dialogs should be full screen — opus-4-8

> **Queue prompt:**
>
> :pushpin: All upload preview dialogs should be full screen. deploy


Done and deployed. The document/upload preview dialogs now open **full screen** (edge-to-edge, no surrounding box) so the image/PDF/text fills the whole viewport.

**What changed** (`src/equityfundingsource-dev`, all in `lib/ui--browser--efs`):
- Added a reusable `full_screen` prop to the shared `PortalModal` — when set, the panel goes 100% width × `100dvh`, square corners, no backdrop gutter.
- The shared in-browser document viewer (`#doc-viewer` `DocumentViewer`, used by `DocumentFileLink`/`DocumentViewActionButton` across borrower/admin/investor document surfaces) now opens `full_screen`; image/PDF heights fill the panel instead of the old `max-w-[960px]` box.
- The admin **Lender Documents** previewer (`LenderDocumentPreviewModal`) is full screen too, and keeps its stacked `z-[1200]` so it still layers over the lender dialog (`#doc-stack-esc`).

**Verified:**
- Full suite green: `bun test` → **836 pass / 0 fail**; browser + worker builds clean. Updated two tests that pinned the old boxed widths.
- **Deployed** `make deploy` → no D1 migrations; uploaded `equityfundingsource` (Worker startup 20 ms); live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**. Confirmed the live bundle `index.browser-JCNA7GFP.js` is served and contains the full-screen panel style.
- Pushed: EFS `af3f5e1..a279f82` (origin/main), superproject pointer `28be2cea5`.

`do [#8667]` (Lending groups) stays gated — it's `[operator-verify]`, still waiting on your name/scope/privacy decisions.

### Re: Bring back the express-interest button, independent of proposed terms — opus-4-8

Done (code committed + pushed; **deploy held** — see below). A lender can now express interest in a deal with **no proposed terms**, and proposed terms are optional.

**What changed** (`src/equityfundingsource-dev`, lender deal detail `lib/ui--browser--efs/portal/investor/deal_detail.tsx`):
- Restored a standalone **"Express interest"** button (primary) that sends interest with an **empty body** — no committed amount, no sold rate required. Works even when a deal shows no remaining balance, since each loan funds at full value.
- The committed-amount / sold-rate inputs are now an **"Optional — propose loan terms"** section. Submitting it sends/updates a proposal; leaving it blank still lets you express interest.
- Once interest is sent, the card shows a "✓ Interest sent" confirmation and keeps the optional terms form (label "Update proposal") plus "Remove interest".

**No server change needed:** the `/api/investor/deals/:id/interested` POST already stored a term-free interest — `loan_term_proposal_from_body_` returns no proposal (and no error) when amount/rate are absent. This was purely a client-side coupling: the old UI threw `'proposal'` unless amount+rate were filled. Validation is now gated on `with_proposal`.

**Verified:** `bun test` → **836 pass / 0 fail** (added #express-interest assertions to the investor e2e suite); browser + worker builds clean. Committed/pushed: EFS `a279f82..7218eb3` (origin/main), superproject pointer `e53c8dff1`.

**Deploy held:** this prompt didn't include `deploy` (unlike the full-screen one), so I committed + pushed but did **not** push to the live Worker. Say **"deploy"** (or re-queue with it) and I'll `make deploy` + smoke-test it live.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> :pushpin: EFS does not arrange partial loans from independent lenders. If a lender or lender group is interested in the loan, the entity will service the entire loan. The proposed terms are optional. A lender can be interested in a loan without any proposed terms. The lender may want to learn more about the property and the borrower...such as receiving documents or viewing the property in person. Please bring back the button to express interest that is independent of the proposed terms.


Deployed. The held **Express interest** change (`#express-interest`, EFS `7218eb3`) is now live.

- **Deploy:** `make deploy` → remote D1 had no migrations to apply; uploaded the new browser bundle `index.browser-ZCVCQ7MB.js` (Worker startup 20 ms) to https://equityfundingsource.brian-takita.workers.dev.
- **Live smoke test:** homepage → **200**; the served HTML references `index.browser-ZCVCQ7MB.js` (matches the just-uploaded artifact).
- **Live-bundle verify:** grepped the deployed JS — it now contains **"Express interest"** and the **"propose loan terms"** optional-section copy, confirming the decoupled interest button (interest with no proposed terms; terms now optional) is in production.

No code changed this cycle — EFS HEAD is still `7218eb3` (== origin/main), clean tree. The earlier commit/push already landed; this was the deploy-only step.

`do [#8667]` (Lending groups) stays gated — it's `[operator-verify]`, still waiting on your name/scope/privacy decisions.

### Re: deploy (already live) — opus-4-8

This `deploy` head was already satisfied by the previous cycle — the Express-interest change shipped to the live Worker (commit `932399f0a`, bundle `index.browser-ZCVCQ7MB.js`, homepage 200, live-bundle verified). EFS HEAD is still `7218eb3` (== origin/main) with a clean tree, so there are no new commits to push — re-running `make deploy` now would be a no-op. Consuming this duplicate marker and moving on to the **Remove "Deal Summary" section** and **Title Case** items next.

### Re: Remove the "Deal Summary" section from the lender deal detail — opus-4-8

> **Queue prompt:**
>
> Remove "Deal Summary" section from https://equityfundingsource.brian-takita.workers.dev/lenders/deals/d6b78994-0970-498f-9e10-6f0035902c0d. The terms of the deal have not been settled yet. The lender will propose terms and the buyer will accept, reject, or negotiate the terms. We are simply not in a state where the terms of the deal should be displayed. Another benefit is the main CTA, "Express Interest" is at the top of the dialog.


Done (code committed + pushed; **deploy held** — the queued `deploy` comes after the Title Case item, so I'll deploy both together next). The **Deal Summary** card is removed from the lender deal-detail dialog.

**What changed** (`src/equityfundingsource-dev`, `lib/ui--browser--efs/portal/investor/deal_detail.tsx`):
- Removed the right-column **Deal Summary** card entirely — it showed loan amount, property value, LTV, **Preferred sold rate**, **Preferred term**, payment frequency, due day, and min. investment. As you noted, those terms aren't settled at this stage (the lender proposes, the borrower accepts/rejects/negotiates), so displaying them is premature.
- With that card gone, the primary **"Express Interest"** CTA (`DealInterestAction`) is now the **top** of the sticky sidebar / dialog.
- Factual loan info is **not lost** — the left-column **"Loan Details"** card (`TmoLoanDetails`) still carries the loan facts.
- Dropped the now-unused `LoanAmountValue` helper.

**Verified:** `bun run check` (test + browser build + worker build) → **836 pass / 0 fail**, builds clean. Updated the `lender-account-gate` test (the stale "deal summary stays visible" assertion) and added a regression guard (`not.toContain('>Deal Summary<')` + `not.toContain('Preferred sold rate')`).

**Committed/pushed:** EFS `7218eb3..1637a53` (origin/main), superproject pointer `298be3a2f`.

**Deploy held** until the Title Case change lands, then one `make deploy` ships both.

### Re: Use Title Case for all headings and buttons — opus-4-8

> **Queue prompt:**
>
> Use "Title Case" for all headings and buttons. We are currently using "Sentence case".


Done (code committed + pushed; **deploy held** for the next `deploy` queue item, which ships this + the Deal Summary removal together). All portal **headings and buttons** are now Title Case instead of Sentence case.

**Scope** (`src/equityfundingsource-dev`, 50 UI files across admin / borrower / broker / investor / shared portals): card-title, page-title, blank-title (empty-state), section/step headings, bare `<h1>`–`<h6>` tags, and `<button>` / `btn`-link labels — **264 strings**. Examples: "Broker dashboard" → "Broker Dashboard", "Pipeline activity" → "Pipeline Activity", "Available deals" → "Available Deals", "Express interest" → "Express Interest", "No available deals" → "No Available Deals".

**How** — a closing-tag-anchored transform that touches **only static heading/button text**, never JSX expression containers or arrow handlers (`onClick={() => …}`), so no code was mangled. It:
- Preserves acronyms (EFS, TMO, LTV, ACH, PDF, SSN, …), HTML entities (`&amp;`), and leading glyphs (`←`, `+`).
- Lowercases minor words mid-phrase (a, and, of, to, from, with, in, on, …) — e.g. "Deals of Interest", "How to Make a Payment", "Back to Lenders".
- **Left out of scope** (kept Sentence case): notification message titles (some are full sentences), body prose, and email content.

**Verified:** `bun run check` (test + browser build + worker build) → **836 pass / 0 fail**, builds clean. Updated source-inspection test assertions to the new casing (delimiter-safe, so prose assertions were untouched); reverted two over-eager assertions for the out-of-scope email CTA + notification title.

**Committed/pushed:** EFS `1637a53..873c9e4` (origin/main), superproject pointer `0834b79ef`.

**Deploy held** — the next `deploy` item will `make deploy` both the Deal Summary removal and this Title Case pass.

### Re: deploy — opus-4-8

Deployed. The **Deal Summary removal** + the **Title Case** sweep are now live (EFS `873c9e4`).

- **Deploy:** `make deploy` → remote D1 had no migrations; uploaded the new browser bundle `index.browser-WPLUGH5P.js` (Worker startup 20 ms) to https://equityfundingsource.brian-takita.workers.dev.
- **Live smoke test:** homepage → **200**; served HTML references `index.browser-WPLUGH5P.js` (matches the uploaded artifact).
- **Live-bundle verify:** grepped the deployed JS — headings/buttons are Title Case (**"Available Deals"**, **"Broker Dashboard"**, **"Express Interest"**), and **"Deal Summary" has 0 occurrences** (card removed). Remaining lowercase hits are URLs (`/lenders/available-deals`), route ids, and prose — not headings.

Both queued changes shipped in one deploy. `do [#8667]` (Lending groups) stays gated — `[operator-verify]`, awaiting your name/scope/privacy decisions.

### Re: #8667 terminology — "Lender Syndicate"? — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#8667]: Use the industry standard terminology. Is it "Lender Syndicate"?


Short answer: **"Syndicate" is a real industry-standard term, but it comes from _institutional/commercial_ bank lending — not the private trust-deed world EFS actually operates in.** So it works, with a caveat. Here are the standard options by where the term comes from:

**1. Syndicate (institutional / commercial banking).** A group of lenders jointly funding one loan to one borrower, usually with a designated **lead arranger / agent** who originates and services while the others participate. Idiomatic forms: **"syndicated loan," "loan syndicate," "lending syndicate."** Members are "syndicate participants" or "co-lenders." Note: "Lender Syndicate" is understandable but slightly less idiomatic than **"Loan Syndicate"** or just **"Syndicate."** Caveat: "syndicate" implies a lead/agent structure — if your group has **no designated lead** and just acts as one entity, the word slightly overpromises that machinery.

**2. Multi-lender loan / Fractionalized note (private money + trust-deed — EFS's actual niche).** When several private investors fund one loan, each holding a fractional interest in the note/deed of trust: **"multi-lender loan," "fractionalized loan," "multi-beneficiary deed of trust."** This is the native vocabulary of The Mortgage Office and CA/private-lending regs. Investors are "co-lenders" or "beneficiaries."

**3. Mortgage fund / pool.** Investors pool capital into an LLC/LP that is itself the single lender of record. Different structure (one entity lends), not a group-per-deal.

**Recommendation — pick by audience:**
- Selling sophistication to **institutional / larger lenders** → use **"Syndicate"** (label it "Loan Syndicate"; members = "Syndicate Participants"). Closest to your "the group services the entire loan" framing **if** you add a lead/servicer role.
- Speaking to **private trust-deed investors** (your current lender base) → **"Multi-Lender Loan"** or **"Co-Lending Group"** is more accurate and less likely to imply a lead-arranger structure you may not have.
- Want zero jargon → keep **"Lending Group."**

My pick if you want one word that signals seriousness without misrepresenting structure: **"Syndicate"** is fine — go with **"Loan Syndicate" / "Syndicate Participants."** If you'd rather stay accurate to how private deals are actually structured, **"Multi-Lender Loan."**

`#8667` stays gated — once you lock the name, I still need your calls on **persistent vs. deal-scoped** groups and **member privacy** before building. Tell me those two and I'll start (plan: `src/equityfundingsource-dev/prds/lending-groups-plan.md`).

### Re: "Multi-Lender Group" or "Lending Group"? — opus-4-8

> **Queue prompt:**
>
> :pushpin: Robert W mentioned "Fractionalized", so "Multi-Lender Group" or "Lending Group"?


**Go with "Multi-Lender Group."** It's the plain-English version of what Robert means by "Fractionalized": multiple lenders each holding a fractional interest in one loan, funded together. It says exactly what the thing is — a *group of lenders* — with no ambiguity and none of the institutional "syndicate" baggage.

Why it beats "Lending Group":
- **"Lending Group"** is ambiguous — it reads as "a group that lends" and doesn't signal that there are *multiple co-lenders* on one deal. It could be mistaken for a single lender's brand/team.
- **"Multi-Lender Group"** is unambiguous, professional, and pairs cleanly with Robert's "Fractionalized" framing: the *loan* is fractionalized across the *Multi-Lender Group*. The members are "co-lenders"; the deal still funds at full value (the group collectively funds 100%, no independent partials — consistent with the express-interest rule).

So: **loan = fractionalized; the lenders = a "Multi-Lender Group"; members = "co-lenders."** If you ever want a shorter UI label, "Lender Group" works as the casual form, but "Multi-Lender Group" is the one I'd put in product copy and the schema.

`#8667` stays gated on the last two calls before I build: **persistent vs. deal-scoped** groups, and **member privacy** (do co-lenders see each other?). Lock the name as "Multi-Lender Group" + answer those two and I'll start Phase 1 (`src/equityfundingsource-dev/prds/lending-groups-plan.md`).

### Re: do #8667 — Multi-Lender Group Phase 1 shipped — opus-4-8

> **Queue prompt:** do [#8667]. Persistent. A lender may also be part of multiple lending groups. Co-lenders do see each other... majority stake has more control... each deal may have different percentages of a loan. Admin should set up lending groups (lender accounts auto-created with an email). Lenders should be able to create a group and invite other lenders.

Decisions locked, and **Phase 1 (schema + repo + tests) is built, green, committed, and pushed** to `origin/main` (`ec08508`).

**Decisions captured** (in `src/equityfundingsource-dev/prds/lending-groups-plan.md`): name **"Multi-Lender Group"** (members = co-lenders); **persistent & reusable**, a lender may belong to multiple groups; **co-lenders see each other**; **majority stake => more control** (control derives from stake, not a fixed role); **per-deal percentages** of the loan; **admin- and lender-created** groups with **invite-by-email + auto-created lender accounts**.

**What shipped:**
- `migrations/0090_lending_groups.sql` — `lending_groups`, `lending_group_members` (role/status/`default_stake_bps`, unique per group+email and group+user), `lending_group_deal_allocations` (per-deal `allocation_bps`, unique per group+deal+member). Percentages stored as **basis points** (0-10000) to avoid float drift, matching the existing `_cents` convention.
- `lib/lending_groups/repo.ts` — group/member/allocation CRUD + a **pure `lending_group_control_`** helper that computes the majority controller (strict >50%, with tie handling), reused for both group-level and per-deal stakes.
- `lib/lending_groups/repo.test.ts` — **19 tests**: the control math, plus the repo run against the real migration via a `bun:sqlite` D1 adapter (CHECK/FK/unique constraints, cascade delete, email normalization, accept-stamps-`joined_at`, account linking, per-deal upsert, and a deal where the majority controller differs per deal).

**Verification:** full suite **855 pass / 0 fail**; `build` + `build:worker` clean.

**One design call I made for you** (flag if you'd rather change it): "majority stake => more control" is modeled as control *derived from stake share* rather than a stored "controller" flag — the `lead` (group creator) is recorded, but the controlling vote follows whoever holds the largest stake (group-level `default_stake_bps`, or per-deal `allocation_bps`). That keeps control honest as percentages shift per deal.

**Next:** Phases 2-4 are queued as actionable backlog items — `#8667p2` (group CRUD + invites, incl. admin account auto-create), `#8667p3` (deal participation + per-deal allocations + majority surfacing), `#8667p4` (admin visibility). None are blocked; say the word and I'll take Phase 2.

### Re: do #8667p2 — Multi-Lender Group Phase 2 shipped — opus-4-8

Phase 2 (group CRUD + invites + admin setup) is built, green, committed, and pushed to `origin/main` (`138448b`).

**What shipped** (`app/equityfundingsource/lib/lending_groups/`):
- `service.ts` — `ensure_lender_account_` (mirrors `ensure_admin_`; auto-creates the account, never flips an existing one's role), `lending_group_create_with_lead_`, `lending_group_invite_member_` (admin `auto_create` seats active; lender-to-lender invite stays pending), `lending_group_respond_invite_` (accept/decline; matches by user_id then invited email; links the account), `lending_group_user_is_member_`.
- `routes.ts` (mounted in `worker.ts`) — Lender API (`role=investor`): `GET/POST /api/lender/groups`, `GET /api/lender/groups/:id` (group + members, member-gated so co-lenders see each other), `POST .../invites`, `POST .../respond`. Admin API: `POST /api/admin/lending-groups` (set up a group, auto-creating lender accounts by email) + `GET /api/admin/lending-groups/:id`.
- `notify.ts` — invite + member-joined notifications (in-app always; email gated by `LENDING_GROUP_EMAIL_NOTIFICATIONS_ENABLED`), mirroring `lib/broker/notify.ts`.
- `service.test.ts` — 13 tests via the bun:sqlite D1 adapter.

**Key decision surfaced while building:** EFS has **no `lender` user role** — the lender base is the `investor` role (`lender` is only a message/request label). So auto-created "lender" accounts are `investor`-role users. Flagging in case you want a distinct role later.

**Verification:** full suite **868 pass / 0 fail**; `build` + `build:worker` clean.

Next in the queue: `#8667p3` (deal participation + per-deal allocations).

### Re: do #8667p3 — Multi-Lender Group Phase 3 shipped — opus-4-8

Striking `do #8667p2` advanced the go-queue to **#8667p3**, which I found already carried forward as uncommitted work in the EFS submodule (service + routes + tests, untracked). I verified, built, and shipped it.

**What shipped** (`app/equityfundingsource/lib/lending_groups/`, committed `8f5ab10`, pushed to `origin/main`):
- `deal_participation.ts` — `lending_group_deal_split_set_` upserts a per-deal allocation split (in bps) and **pulls the group into the deal**; rejects allocations referencing non-members. `lending_group_deal_participation_R_` returns the active co-lenders, each member's % of the deal (0 if unset), the allocated total / `fully_allocated`, and the **majority controller** derived from the per-deal allocations via `lending_group_control_`.
- `routes.ts` — lender `GET /api/lender/groups/:id/deals/:dealId` (member-gated participation read) and `POST /api/lender/groups/:id/deals/:dealId/allocations` (set the split); admin `GET /api/admin/lending-groups/:id/deals/:dealId`. `parse_allocations_` validates the request body.
- `deal_participation.test.ts` — split totals / `fully_allocated`, partial split, non-member rejection, upsert-in-place, controller derivation, the 0%-default-no-controller case, and the 50/50 tie (leader-but-no-majority).

**Verification:** 39 lending-group tests + 875 full app tests green; `build:worker` bundles clean.

**Scope call** (flag if you'd rather I'd crammed it in): the backlog item's *view rendering* half ("render co-lenders … in the deal-package / interested-lenders views") lands on the same 2,000–3,000-line admin/investor `deal_detail` components that **#8667p4** already targets, and it needs a by-deal participation query that doesn't exist yet. Rather than wedge a risky view change into a backend carry-forward commit on a live app, I shipped the participation **mechanism + API** as Phase 3 and broadened **#8667p4** to own the deal/interest-view rendering (admin **and** the investor deal-package surface) plus the by-deal query. Nothing dropped.

Next in the queue: `#8667p4`.
<!-- agent:boundary:e10e69d4 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Sync
---
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~:pushpin: do [#8667p2]~~
- :pushpin: [#8667p3]
- :pushpin: [#8667p4]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#8667p4] Multi-Lender Group Phase 4 — deal/interest view rendering: add a by-deal participation query (groups participating in a deal), then render co-lenders + each member's % + the majority controller in the admin deal / interest views AND the investor deal-package view (folds in #8667p3's view-rendering half); tests.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
- [/] [#cftk] [blocked:operator-credential] Refresh CLOUDFLARE_API_TOKEN in src/equityfundingsource-dev/.env with a D1-Edit-scoped token on account 1e7e194ff8da4ad9e40ce041aea84f76 (DB 280e1539). No stored credential has D1 access (tested: 10000/wrong-acct/7403/malformed); operator must create the token in the Cloudflare dashboard. .env CLOUDFLARE_ACCOUNT_ID is a literal passage: placeholder (left intact).
- [/] [#5eq8] Closing docs step (redesign): rebuild the closing-documents/notarization flow as its own step (replaces the removed Final Loan Documents section). Closing docs are wet-ink signed AND notarized on PAPER (not e-signed); EFS records a signed-status flag (+ final_docs_* D1 columns preserved) and that signed status is a BI-DIRECTIONAL sync with TMO (EFS <-> TMO), not an in-app e-signature flow. Re-introduces a notarize control and SHOULD re-gate allocation wire status (Sent/Confirmed) on notarization — the UI gate + server 409 were removed in the 2026-06-18 AvailableDeal restructure so funding is not hard-blocked in the interim.
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
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked (Cloudflare API token at `passage btak/d1/CLOUDFLARE_API_TOKEN`); migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#ghl-stage-bridge] [recommended] Implement an event-driven GHL stage bridge from EFS Deal Pipeline state to HighLevel contacts/opportunities/workflows for Processing, Underwriting, Terms negotiation, and Closing while keeping EFS/D1 canonical for loan, lender-selection, borrower portal, document, audit, and funding state.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG -> transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6-12 min: investors, brokers, borrowers) — push Judith + Robert helping
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
