---
agent_doc_session: 6cb2a9bf-8521-46d0-8670-e76cf4aeabd4
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: high
claude_args: --dangerously-skip-permissions
links:
  - ./build-party.md
  - ./build-party-course-diagram.md
  - ./robert-ross-granola.md
  - ../research/git-agent-workflow.md
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**Compacted 2026-04-15** — full history archived; see `.agent-doc/archives/` for prior exchange. Recent transcripts in `tasks/dreamable/transcripts/`.

## Relationship arc (Apr 1 → Apr 14)

- **Apr 1** — Rob reached out re: Lead Software Engineer role at Dreamable (6-person co). Brian's positioning: "Software → AI → Agents" thesis maps directly to agent-doc / corky / agent-loop stack.
- **Apr 2** — First Zoom (1:30pm PT). Live-demoed agent-doc to Rob + Miguel during call. Email threading bugs (in_reply_to vs threadId) surfaced → filed against corky in `tasks/software/corky.md`.
- **Apr 4–10** — Multiple follow-up calls (see `transcripts/2026-04-0{5,6,8}-robert-ross.md` + `phone_20260408-180506_*.md`, `phone_20260410-214753_*.md`). Scope converged on **claudescore.com** — a scoring API that grades AI coding sessions against a rubric. Rob wants Brian building it.
- **Apr 10** — Rob flagged calibration problem: rubrics scored Brian's session ~82/100 expected. He suggested adding calibration points (Axon winner, Gary Tan's repo). Brian distinguished two scoring methodologies (~100 factors each), not two factors.

## 2026-04-14 call — Brian / Robert / Miguel (3h37m)

Full diarized transcript: [`transcripts/2026-04-14-robert-ross-miguel.md`](transcripts/2026-04-14-robert-ross-miguel.md). Miguel dropped off around the 1h mark; back half was a 1-on-1 compensation/commitment conversation.

**Key topics:**
- **Document taxonomy / sync.** Rob is overwhelmed by the volume of agent-doc / spec / agents.md / rubric markdown files. Demanded consistent metadata header on every doc (document name, github link, version, datetime last updated, purpose, intended audience) at 100% compliance, in both GitHub markdown and mirrored Google Docs.
- **GitHub → Google Docs automation (Brian MVP, est. 8–16h).** Trigger on git push, create/update Google Doc per markdown file in `claudescore/session-share-docs` Drive folder, use Docs *tabs* for linked sub-docs, color-code diffs, Slack notify on significant updates, transfer ownership `brian@dreamable.com` → Rob on creation. Miguel demoed a Cloud Code → Mermaid → FigJam MCP workflow to spec it visually first.
- **Rubric / scoring bug (the original bookmark).** Claude Code + Superpowers repos scored artificially low because rubrics contained factors that didn't apply to the repo — non-applicable factors were scored **zero** instead of excluded, dragging aggregates down. Rubrics live in `clodscore.com/scoring-api`, derived from the spec + DeepEval. Fix: mark factors not-applicable per repo (mostly deterministic).
- **Sam / MIT validators thread.** Agreed framing for public reply: "showing to my dev — we want to run evals on agentic codebases / improve our CLAUDE.md harness." Avoid revealing claudescore.
- **Notion vs Google Docs.** Acknowledged Notion is better long-term (every.to runs on Notion + agents). Deferred — stick with Google Docs MVP to unblock Rob.
- **Provisional patent.** Rob is filing ~9-page provisional covering claudescore + a separate live-event / mixed-room coordination stack (router agent for n humans + m agents, parallel dispatch, cross-session memory, expertise-weighted voting, auto community formation). Brian named co-inventor (no ownership — contractor terms, dreamable owns). Target filing: 1–2 weeks.
- **Build party Delphi-style agents.** Live-called "David" (a Delphi AI persona) during the meeting for product advice on agent-doc. Rob wants to clone Delphi's model inside build party with named agents: **Nova** (room host), **Echo** (session memory / catch-up), **Orbit** (build buddy / external research), **Flare** (post-session social highlights), **Apex** (launch monitoring). Expertise-weighted voting ("only qualified peers vote") extends to the claudescore expert network.
- **Compensation / commitment / FileVine.** Brian has a FileVine interview **2026-04-15**. Rob floated **$16–18k/month** working range. Tensions: (a) Brian's time estimates are unreliable — Brian admits rubric work has too many unknowns, (b) "loosey-goosey" working style without dev plans, (c) OpenAI Codex Score could ship first, (d) risk Brian leaves for FileVine. Rob's bluntest moment: *"you haven't shipped product over and over again that had enough value that stuck in the marketplace."*
- **Personal-finance digression (~30m).** Rob walked Brian through a budget spreadsheet. Brian has no health insurance (self, Brooke, Leilani-5), no dental, no life, rent $1,800, food $300–600/wk. Rob's framing: *"your budget is the harness of your financial stability"* and "you can't model this for Leilani if you don't do it yourself."

**Decisions / agreements:**
- Metadata header on every claudescore doc, github + Google Doc mirror, 100% compliance.
- Brian builds GitHub → Google Docs sync MVP (create/update/delete + tabbed sub-docs + metadata header + ownership transfer). Slack notify + color-diff are stretch, not MVP blockers.
- Notion migration deferred.
- Brian = co-inventor on Rob's provisional, no ownership.
- Comp range: $16–18k/month working figure, no formal contract signed.
- **Hard cutoff:** if claudescore can't ship by ~**2026-12-01**, kill the project rather than burn more money.
- Brian reworks rubric scoring to exclude non-applicable factors (immediate technical priority).
- **NO AWS deployment yet.** Rob had been pushing for AWS; Brian convinced him deploying broken scoring creates rework. Defer until benchmark repos pass.
- Rob follows up morning of **2026-04-15** before FileVine interview.

**Open questions / unresolved:**
- **Time estimates.** Rob wants per-task dev plans; Brian prefers "give me X hours/days to scope it first." Rob's ask: don't go silent — say "I don't know yet."
- **AWS deploy criteria.** No concrete pass/fail thresholds written down. Action implied, not assigned.
- **FileVine vs stay.** Unresolved. Brian said "stick around" but Rob explicitly noted that's not a money-backed guarantee. Tabled to 2026-04-15 morning call.
- **Bidirectional Docs comments → GitHub issues.** Brian floated; Rob said most of his comments are scratch — don't pollute the issue tracker. Explore GitHub Discussions.

**Action items:**
- **Brian** — GitHub → Google Docs sync MVP; fix rubric scoring (exclude non-applicable factors); define concrete AWS deploy pass-criteria; reply to Sam on public thread with agreed framing; investigate rubric refactor scope (days vs weeks) and report; decide on FileVine before interview.
- **Robert** — file provisional patent in 1–2 weeks (Brian co-inventor); follow up morning of 2026-04-15; bring more devs on to build party so Brian isn't sole backend.
- **Miguel** — none explicit; demoed Mermaid/FigJam workflow, dropped off ~1h.

## Artifacts in-flight

- `corky` threading bugs (Message-ID resolution + Gmail API `threadId` field) — tracked in `tasks/software/corky.md`.
- Call prep PDF: `tmp/robert-ross-call-prep.pdf` (from Apr 2 prep, may be stale).
- Transcripts dir: `tasks/dreamable/transcripts/` — all Rob calls archived here, never embed inline.

❯ transcribe + diarize /home/brian/work/btakita/agent-loop/mail/contacts/robert-ross/recordings/phone_20260415-194318_4156839775.m4a
### Re: April 15 phone call transcript

Transcribed & diarized — 43:36 call between you and Robert covering documentation strategy for CloudScore, FileVine interview aftermath, and financial planning. Saved to [`transcripts/2026-04-15-robert-ross-phone.md`](transcripts/2026-04-15-robert-ross-phone.md).

**Key topics from call:**
- **Documentation as product:** Rob wants every CloudScore doc (GitHub + mirrored Google Docs) to have consistent metadata headers (name, link, version, last updated, purpose, audience) at 100% compliance — fundamental to credibility
- **Public vs. proprietary split:** Long discussion on what content should be public; landed on Claude Code as partner to help make those decisions (not just you and Rob guessing)
- **Rubric work deferred:** Don't build GitHub→Google Docs sync yet; focus on the conceptual doc taxonomy first (what goes public, why, how to mark it)
- **Personal/budget:** Rob walked Brian through expense breakdown; highlighted rent ($1,800) + groceries (~$400–500/week) = ~$4k/month baseline; Brian committed to filling out budget spreadsheet
- **Two action items:** (1) fill out budget tonight, (2) think through how Claude Code becomes part of the documentation decision-making process
<!-- agent:boundary:6a4a4717 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
