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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260523-225835.md`*

Compacted content:
- Archived 2 response topic(s): Send inquiry email; Direct draft email send
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ The images on http://127.0.0.1:8787/about are broken. Please download asset files.
### Re: About image assets — gpt-5

Downloaded the four missing `/about` team image assets from the `equityfundingsource-assets` R2 bucket into the local gitignored asset cache:

- `public/assets/images/about/judith-krug-team.png`
- `public/assets/images/about/diko-baghdasarian.png`
- `public/assets/images/about/april-armas.png`
- `public/assets/images/about/jessica-rian.png`

Then repacked the Worker static asset bundle so `http://127.0.0.1:8787` serves the new files from `app/equityfundingsource/dist/worker-assets`.

Verification:

- `curl -I http://127.0.0.1:8787/assets/images/about/{judith-krug-team,diko-baghdasarian,april-armas,jessica-rian}.png` returned `200 OK` with `Content-Type: image/png`.
- `curl -I http://127.0.0.1:8787/about` returned `200 OK`.
- `bun run check` in `src/equityfundingsource-dev` passed: 30 tests, app build, and worker build.
- `make check` at the repo root passed.
- `gh run list --workflow CI --limit 1` found no workflow named `CI`. The available `Agent Doc` workflow’s latest run, `26345829441`, is red, but both `tmux-ci` and `check` jobs have empty step lists and `gh run view --job ... --log` returns `log not found`, so I’m classifying that as an external CI-start blocker rather than a code or tmux regression.

The repo download script (`bin/assets-download.sh`) still needs S3/Boto credentials that are not present in this checkout, so I used the active Wrangler auth path directly for the missing objects.
<!-- agent:boundary:c0e9c584 -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:pending -->
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
