---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent_doc_format: template
agent_doc_write: crdt
agent: codex
codex_args: -s danger-full-access
claude_model: opus
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260523-044614.md`*

Compacted content:
- Archived 2 response topic(s): dev portal links; dev database
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ http://localhost:4200/login?role=broker has an error...Database not configured
❯ #spec-test-commit-push
### Re: localhost broker login — gpt-5

Fixed the `http://localhost:4200/login?role=broker` failure path. The lightweight `bun dev` server still cannot perform real auth because it has no D1/Worker bindings, but login/signup/Google callback submissions now render the role-specific auth page with a clear local-preview setup message instead of the raw `Database is not configured` 500 body.

Changed paths in `src/equityfundingsource-dev`:

- `app/equityfundingsource/lib/auth/routes.ts`
- `app/equityfundingsource/dev-routes.test.ts`
- `docs/deploy.md`
- `app/equityfundingsource/bin/cf-wrangler.ts`

Verification:

- `bun test` passed: 24 tests, 0 failures.
- `bun run check` passed: tests, site build, and worker build.
- `git diff --check` passed.
- Local smoke test with `bun run dev` on port 4200: `GET /login?role=broker` returns 200; `POST /login?role=broker` returns the Broker Login page with the setup message instead of the raw database error.
- CI/tmux evidence: this repo has no GitHub Actions workflows or runs (`gh workflow list` and `gh run list` returned empty), so there was no remote CI leg to inspect.

Commit/push:

- Committed `fdd4b84 fix: handle local preview auth setup` in `src/equityfundingsource-dev`.
- Pushed `main` to `origin` (`f8d1717..fdd4b84`).

Escalation: none. Confidence: high. The dev server is currently running at `http://localhost:4200`; use `http://127.0.0.1:8787` via `bun run dev:worker` for real broker sign-in with local D1.
<!-- agent:boundary:53367799 -->
<!-- /agent:exchange -->
###

<!--
Please directly send the draft email using brian.takita@gmail.com
-->

## Pending / Not Built

<!-- agent:pending -->
- [ ] [#jnzs] [recommended] Before invoicing Judith, run final EFS polish: apply the D1 migration/deploy, smoke-test /funded-deals and /portal-admin/funded-deals on the live Worker, and get Judith's approval on funded-deal images/copy/legal wording.
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
