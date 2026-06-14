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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260614-065755.md`*

Compacted content:
- Archived 11 response topic(s): Use proper label names on the Declarations summary; deploy; "Drop or choose a replacement file" → "Drop file or click to select file"; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Wide Deal Pipeline kanban — UX options to cut horizontal friction — opus-4-8

> **Queue prompt:**
>
> The Deal Pipeline kanban board is very wide...which can add friction to the admin workflow. What are some UX options to mitigate this issue? Discuss...


The root cause is one column per TMO stage. With ~8–12 stages, the board is wider than any screen, so admins horizontal-scroll to find work and lose the at-a-glance overview that makes a kanban worth having. Options, cheapest-to-richest:

**1. Collapse the many stages into a few macro-phases (recommended default).**
Group the detailed TMO stages under 4–5 phase columns — e.g. *Intake → Underwriting → Approval → Closing → Funded* — and keep the precise stage as a chip/label on the card (and a sub-row if you want). Drag between phases still works; the exact stage shows on the card and in the detail drawer. This is the single biggest width win because column count drops by ~half or more, and it matches how an admin actually triages ("where is this deal, roughly?") before drilling in.

**2. Collapsible / condensed columns.**
Let admins collapse any stage to a thin rail showing just the name + count badge, expanding on click. Auto-collapse empty stages. Pairs well with #1 — collapse the phases you're not working today. Low effort, no data model change.

**3. List/table view toggle.**
Offer a dense, sortable table (group or sort by stage) as an alternate view. Kanban is great for visual flow; a table is better for bulk triage and handles many stages with zero horizontal sprawl. A view toggle (Board / List) lets each admin pick. This is the most reliable "never scroll sideways again" answer.

**4. Saved filters / focused views.**
"My deals", "active only", single-stage, by-broker, by-date. Most width pain is from rendering deals nobody is touching right now; a default "active" filter shrinks the working board immediately. Cheap, high payoff, composes with everything above.

**5. Card density control.**
A compact card mode (one line: borrower + amount + stage chip) narrows every column, so more fit before scroll kicks in. Quick CSS-level change.

**6. Master–detail stage rail (bigger rework).**
Replace the horizontal columns with a vertical left rail of stages + counts; clicking a stage shows its deals in the main pane. Eliminates horizontal width entirely. More build, but the most scalable if the stage list keeps growing.

**Recommended path:** ship **#1 (phase grouping) + #4 (default "active" filter) + #3 (List toggle)** together — phase grouping fixes the structural width, the active filter trims the working set, and the List view is the escape hatch for power triage. #2 and #5 are low-cost polish to layer on after. This stays inside the existing #67v2 kanban work rather than a rewrite.

This is a discussion, so I haven't changed code. If you want, say the word and I'll queue the chosen combination as an implementation item against #67v2.

### Re: Deal Pipeline horizontal width improvements — gpt-5

> **Queue prompt:**
>
> Implement the recommended path for the Deal Pipeline horizontal width improvements.


Implemented the recommended path for the Deal Pipeline width issue in `src/equityfundingsource-dev`.

Changed paths:
- `lib/ui--browser--efs/portal/admin/pipeline.tsx`
- `tests/admin-pipeline.test.ts`
- `tests/application-status-ui.test.ts`
- `tests/portal-table-dialog.test.ts`
- `prds/investor-pipeline-admin.md`
- `prds/equity-funding-source-lending-portal-prd.md`

What changed:
- Deal Pipeline now defaults to an **Active** scope that hides terminal/parked cards.
- Board columns are grouped into macro phase lanes: **Intake**, **Processing**, **Underwriting**, with **Funded / Closed** and **Parked** available under **All**.
- Cards keep the precise raw status visible as a stage chip, so phase grouping does not hide TMO/application state.
- Added a `?view=table` dense triage view over the same filtered pipeline cards.
- Drag/drop still delegates through the shared loan-status writer; phase drops use the lane's primary target status.
- Updated PRD notes and static tests that previously asserted the board was Kanban-only.

Verification:
- `bun test tests/admin-pipeline.test.ts` — pass, 50 tests.
- `bun test tests/portal-table-dialog.test.ts tests/application-status-ui.test.ts` — pass, 23 tests.
- `bun run check` — pass, 674 tests + app build + worker build.
- `make check` — pass, same EFS full check path.
- `git diff --check` — pass.

Commits/pushes:
- EFS submodule: `0dd1f15` (`admin pipeline: add compact triage views`) pushed to `btakita/equityfundingsource-dev main`.
- Superproject pointer: `99579ecfa` (`efs: update deal pipeline triage views`) pushed to `btakita/agent-loop main`.

GitHub Actions:
- No workflow named `CI` exists in the EFS submodule or superproject.
- The superproject's only workflow, `Agent Doc`, ran for `99579ecfa` and failed before any runner started: both jobs had `runner_id: 0`, no steps/logs, and the annotation says recent account payments failed or the spending limit needs to be increased. That is an external CI-start/billing blocker, not a code/test failure.

No new actionable follow-up item was created by this cycle; the next queued item already covers the optional #2 + #5 polish.

Note: preflight also warned the installed `agent-doc` binary is stale relative to source; I did not rebuild it because this cycle was scoped to EFS pipeline work.

### Re: Deal Pipeline #2 + #5 polish — gpt-5

> **Queue prompt:**
>
> Add #2 + #5 polish for the Deal Pipeline horizontal width improvements


Implemented the Deal Pipeline horizontal-width polish requested for queue item #2 by combining the #2 and #5 board improvements:

- Added collapsible phase lanes in `lib/ui--browser--efs/portal/admin/pipeline.tsx`. Empty lanes now auto-collapse into narrow count rails, and each expanded lane has a collapse control while each rail has an expand control. The collapsed rails still accept drops.
- Added a board-only Density segmented control with Roomy/Compact modes. Compact mode is persisted with `?density=compact`, narrows lane widths/gaps, tightens card padding, and renders one-line compact card summaries so more lanes/cards fit horizontally.
- Kept the card action menu/details behavior available in compact mode, including the modal/detail flows and existing publish/close/delete menu affordances.
- Updated `tests/admin-pipeline.test.ts` to pin the new lane-collapse and card-density UI wiring.
- Updated `prds/investor-pipeline-admin.md` and `prds/equity-funding-source-lending-portal-prd.md` to document collapsible rails, empty-lane auto-collapse, compact density, and the `?density=compact` URL state.

Verification:

- `git diff --check` passed.
- `bun test tests/admin-pipeline.test.ts tests/application-status-ui.test.ts tests/portal-table-dialog.test.ts` passed: 73 tests.
- `bun run check` passed: 674 tests plus app build and worker build.
- `make check` passed: delegates to the same full Bun test/build/worker check.
- `gh run list --workflow CI --limit 1` reports no workflow named `CI` in either `btakita/equityfundingsource-dev` or the parent `btakita/agent-loop` repo. `gh workflow list` shows no workflows in the EFS repo and only `Agent Doc` in the parent repo.

Commits pushed:

- EFS submodule: `da452b8438b273f8906ce53a90e42894be7fb1ce` (`admin pipeline: add compact collapsible board controls`) to `btakita/equityfundingsource-dev:main`.
- Parent pointer: `c8a886e1e` (`efs: update deal pipeline board polish`) to `btakita/agent-loop:main`.

Commit message used for the code change: `admin pipeline: add compact collapsible board controls`.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed the EFS worker to Cloudflare. The Deal Pipeline width work (triage views `0dd1f15`, collapsible/compact polish `da452b8`) was already committed and pushed, so this cycle shipped the live deployment of that code — no new source change, nothing to commit/push.

Deploy (`bun run deploy:worker` in `src/equityfundingsource-dev`):
- Remote D1 migrations: `✅ No migrations to apply!` (0025/0026 already applied).
- Worker uploaded and deployed: 1 new static asset, total 908.70 KiB / gzip 211.05 KiB, startup 17 ms.
- URL: https://equityfundingsource.brian-takita.workers.dev
- Scheduled trigger preserved: `0 * * * *` (hourly tmo-sync).
- Current Version ID: `e28a3536-2a0d-4555-951c-872a3c5fb729`.
- All bindings intact (D1 `equityfundingsource`, R2 assets/documents/tmo-cache, BorrowerChatRoom/AdminEventsRoom Durable Objects, Access env vars).

Smoke check:
- `GET /` → 200
- `GET /funded-deals` → 200

Verification scope: deploy only — no code, test, or build-logic change this cycle, so I did not rerun the full Bun suite (prior cycle: 674 tests + app/worker build green). Cloudflare auth confirmed via `cf:whoami` (account `1e7e194f…`, `brian.takita@gmail.com`).

### Re: Intake missing completed + accepted applications — opus-4-8

> **Queue prompt:**
>
> The new Deal Pipeline kanban is missing the completed application + accepted application from the intake section. deploy


Root cause: the earlier phase-grouping refactor merged the four funnel stages (Draft / Complete / Under review / Accepted applications) into one **Intake** macro-lane, so the completed and accepted application stages lost their own columns — they survived only as per-card chips. Per your choice, I split Intake back into its underlying stages while keeping the later phases grouped.

Verified against live prod D1 first: `full_applications` = 1 accepted, 19 draft, 0 submitted, 0 in_review; `loans` = 1 in `Application Received`. So there is 1 accepted-application loan (now its own **Accepted applications** lane) and 0 completed/submitted applications (the **Complete Applications** lane will show as an empty auto-collapsed rail until a borrower submits one — that is data, not a bug).

Changes (`src/equityfundingsource-dev`):
- `lib/ui--browser--efs/portal/admin/pipeline.tsx` — added a `split: true` flag on the Intake phase def; `pipeline_phase_columns_` now `flatMap`s, expanding a split phase into one lane per underlying stage (each with its own label, count, and drop target) while non-split phases stay merged. Empty stages still auto-collapse to thin rails; the read-only funnel stages (Draft/Complete/Under review) remain non-droppable, Accepted applications stays a drop target.
- `tests/admin-pipeline.test.ts` — pinned the split-Intake wiring.
- `prds/investor-pipeline-admin.md`, `prds/equity-funding-source-lending-portal-prd.md` — updated the board description: Intake split into stage lanes, later phases macro-grouped. (#pipeline-intake-split)

Verification:
- `bun run check` — 674 pass, 0 fail (4256 assertions) + app build + worker build green.
- `git diff --check` clean.

Commits / pushes:
- EFS submodule: `9a11735` (`admin pipeline: split Intake into per-stage lanes`) → `btakita/equityfundingsource-dev:main`.
- Superproject pointer: `a61d97001` (`efs: split admin pipeline Intake into per-stage lanes`) → `btakita/agent-loop:main`.

Deploy:
- `bun run deploy:worker` — no D1 migrations to apply; worker uploaded + deployed. Version `863ffa1c-9bf6-4e15-ae2b-3a85db0bda74`, hourly tmo-sync schedule preserved, all bindings intact.
- URL: https://equityfundingsource.brian-takita.workers.dev
- Smoke: `/` 200, `/funded-deals` 200, `/portal-admin/pipeline` 303 (Cloudflare Access login redirect — expected for the admin-gated route).

CI: no `CI` workflow exists in either repo; the superproject's only workflow (`Agent Doc`) remains blocked at runner-start by the account billing/spending-limit issue, an external CI-start blocker, not a code failure.

### Re: Horizontal collapsed-lane titles — opus-4-8

> **Queue prompt:**
>
> I find the sideways text difficult to read...particularly with the all-caps ![img_13.png](img_13.png). Please layout the column title in a way that is more horizontal while the column is collapsed. Propose options.


You're right — the collapsed lane (`pipeline.tsx:863`) rotates the label with `writing-mode:vertical-rl;transform:rotate(180deg)` and forces `text-transform:uppercase`, so "APPLICATIONS UNDER REVIEW" reads bottom-to-top in all caps in a 76px-wide lane. The constraint driving the rotation is width: the full labels are long, and 76px can't hold them on one horizontal line. Here are four ways to make the title horizontal, ordered by how much I'd recommend them.

**Option A — short label, horizontal, mixed-case (recommended).** Drop the rotation and the all-caps. Render a short label horizontally (wrapping to 2 lines), keep the full name in the existing `title` tooltip. Map: Draft Applications → "Draft", Complete Applications → "Complete", Applications Under Review → "Under Review". Cleanest and most scannable; lane stays 76px.
```
// replace the rotated div with:
<div title={`${column.label} - ${column.stage_labels.join(' / ')}`}
  style="writing-mode:horizontal-tb;font-weight:700;font-size:11px;line-height:1.2;color:#334155;text-align:center;white-space:normal;overflow-wrap:anywhere;">
  {column.short_label ?? column.label}
</div>
```
(needs a `short_label` on the column model, or a small lookup; one-time data add.)

**Option B — full label, horizontal, word-per-line.** Keep every word but stack each on its own upright line, centered, mixed-case. No rotation, no new data — "Applications / Under / Review" reads top-to-bottom but each word is upright and legible. Good if you don't want to maintain short labels.
```
style="writing-mode:horizontal-tb;font-weight:700;font-size:11px;line-height:1.25;color:#334155;text-align:center;white-space:normal;overflow-wrap:break-word;"
```

**Option C — widen the collapsed lane to ~104px + horizontal wrap.** Bump `flex:0 0 76px` to `~104px` so the full mixed-case label wraps to 2-3 horizontal lines comfortably. Keeps full labels with zero abbreviation, costs ~28px of board width per collapsed lane.

**Option D — count-forward chip, label in tooltip only.** Most compact: show only the `+`, the count badge, and a 2-letter monogram ("DA", "CA", "UR"); full label lives in the tooltip. Best if board density matters more than reading the title at a glance — but it hides the words, which is the opposite of what you asked for, so I'd skip it.

My pick: **Option A** for the best readability/width tradeoff, falling back to **Option B** if you'd rather not maintain a short-label map. Both keep the lane at its current 76px. Tell me which (or mix — e.g. A's short label with C's slightly wider lane) and I'll implement it. Queued a backlog item to carry the chosen implementation. (Exchange is ~167 lines; flagging size, continuing the queue.)

### Re: Do Option A — horizontal collapsed-lane titles — opus-4-8

> **Queue prompt:**
>
> Do Option A. deploy

Implemented **Option A** for the collapsed-rail title: dropped the rotated `writing-mode:vertical-rl` all-caps label and now render a short, mixed-case alias horizontally, with the full stage label preserved in the rail tooltip. So a 76px collapsed rail reads "Under Review" left-to-right instead of "APPLICATIONS UNDER REVIEW" bottom-to-top.

Changes (`src/equityfundingsource-dev`):
- `lib/ui--browser--efs/portal/admin/pipeline.tsx` — added `LANE_SHORT_LABELS` (keyed by lane id) + `lane_short_label_(column)`: Draft Applications → **Draft**, Complete Applications → **Complete**, Applications under review → **Under Review**, Accepted applications → **Accepted**; any other lane strips a trailing " Application(s)" and falls back to the full label. The collapsed lane title is now `writing-mode:horizontal-tb`, mixed-case (`font-weight:700`, no `text-transform:uppercase`), centered, wrapping with `overflow-wrap:anywhere`. The `title` tooltip still carries the full label + stage list.
- `tests/admin-pipeline.test.ts` — pinned the Option A wiring (`lane_short_label_`, the Under-Review short alias, `{lane_short_label_(column)}`, `writing-mode:horizontal-tb`, and asserted `writing-mode:vertical-rl` is gone).
- `prds/investor-pipeline-admin.md`, `prds/equity-funding-source-lending-portal-prd.md` — documented the horizontal mixed-case collapsed-rail title (#pipeline-collapsed-short-label).

Verification:
- `bun test tests/admin-pipeline.test.ts` — 50 pass.
- `bun run check` — 674 pass, 0 fail + app build + worker build green. (`make check` is the same path.)
- `git diff --check` — clean.

Commits / pushes:
- EFS submodule: `790b8f1` (`admin pipeline: horizontal mixed-case collapsed-lane titles`) → `btakita/equityfundingsource-dev:main`.
- Superproject pointer: `7dd5e42a2` (`efs: horizontal collapsed-lane titles in admin pipeline`) → `btakita/agent-loop:main`.

Deploy (`bun run deploy:worker`):
- No D1 migrations to apply; worker uploaded + deployed. Version `c0229838-018f-4dfd-bfde-b4a86a7fabff`, hourly tmo-sync schedule preserved, all bindings intact.
- URL: https://equityfundingsource.brian-takita.workers.dev
- Smoke: `/` 200, `/funded-deals` 200, `/portal-admin/pipeline` 303 (Cloudflare Access login redirect — expected for the admin-gated route).

CI: no `CI` workflow exists in either repo; the superproject's only workflow (`Agent Doc`) remains blocked at runner-start by the account billing/spending-limit issue — an external CI-start blocker, not a code failure.

### Re: Same collapse strategy on the Lender Pipeline — opus-4-8

> **Queue prompt:**
>
> Do the same collapse strategy on the Lender Pipeline.


Ported the Deal Pipeline horizontal-width collapse strategy to the **Lender Pipeline** board (`investor_pipeline.tsx`, `AdminInvestorPipeline` → `InvestorPipelineBoard`). The five lifecycle lanes (Signed Up → Accredited → Lender Ready → Committed → Active) still overflow narrower admin screens, so they now get the same controls the Deal Pipeline uses.

What changed (`src/equityfundingsource-dev`):
- **Collapsible lanes** — each expanded lane has a `-` collapse control; collapsed lanes become a thin **76px count rail** with a `+` expand control. Empty lanes **auto-collapse** by default (`investor_lane_is_collapsed_` = override ?? `count === 0`); an explicit click records a per-lane override that wins over the empty default.
- **Horizontal collapsed-lane title** — the rail renders its title **horizontally in mixed-case** (`writing-mode:horizontal-tb`, no rotation, no all-caps), tinted with the lane accent, full label preserved in the tooltip. The five stage names are already short, so no short-alias map was needed.
- **Card density toggle** — a board-level **Roomy / Compact** control persisted as `?density=compact`. Compact narrows lane width (280px → 238px), tightens lane gap + card padding, and condenses each card's routine email/accreditation/lender rows into one scannable line.
- The collapsed **Active** rail keeps its drop target and the **awaiting-wire** block stays visible in both densities, so drag-to-confirm-wire (#vd8s-dragwire) survives collapse + compact.

Spec + tests (per `#spec-test-commit-push`):
- `prds/investor-pipeline-admin.md` — added **Phase 6 — horizontal-width collapse strategy (#lender-pipeline-collapse, shipped)**.
- `tests/admin-investor-pipeline.test.ts` — 4 new tests: collapse helper + auto-collapse + expand/collapse controls; horizontal mixed-case rail title (asserts no `writing-mode:vertical-rl`); persisted density helpers + Roomy/Compact toggle + lane width/gap; compact one-line card while the awaiting-wire block is preserved.

Verification:
- `bun test tests/admin-investor-pipeline.test.ts` — 48 pass, 0 fail.
- `make check` / `bun run check` — **678 pass, 0 fail** + app build + worker build green (the `d1 down` / tmo-outbox lines are expected error-path fixtures). `git diff --check` clean.

Commits / pushes (no deploy — preset was commit + push):
- EFS submodule: `7d0d691` (`lender pipeline: collapsible lanes + compact density board controls`) → `btakita/equityfundingsource-dev:main`.
- Superproject pointer: `dd9288e07` (`efs: lender pipeline collapsible/compact board controls`) → `btakita/agent-loop:main`.

CI: no `CI` workflow exists in either repo; the superproject's only workflow (`Agent Doc`) stays blocked at runner-start by the account billing/spending-limit issue — an external CI-start blocker, not a code failure. (Exchange is large at ~230 lines but the queue is active, so I'm continuing the drain rather than pausing to compact.)
<!-- agent:boundary:05f0593f -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
