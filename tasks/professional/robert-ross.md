---
agent_doc_session: 6cb2a9bf-8521-46d0-8670-e76cf4aeabd4
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: high
claude_args: --dangerously-skip-permissions
links:
  - ../dreamable/build-party.md
  - ../dreamable/build-party-course-diagram.md
  - ../dreamable/robert-ross-granola.md
  - ../research/git-agent-workflow.md
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ **Compacted 2026-04-15** — full history archived; see `.agent-doc/archives/` for prior exchange. Recent transcripts in `tasks/dreamable/transcripts/`.

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
❯ ### Re: April 15 phone call transcript

Transcribed & diarized — 43:36 call between you and Robert covering documentation strategy for CloudScore, FileVine interview aftermath, and financial planning. Saved to [`transcripts/2026-04-15-robert-ross-phone.md`](transcripts/2026-04-15-robert-ross-phone.md).

**Key topics from call:**
- **Documentation as product:** Rob wants every CloudScore doc (GitHub + mirrored Google Docs) to have consistent metadata headers (name, link, version, last updated, purpose, audience) at 100% compliance — fundamental to credibility
- **Public vs. proprietary split:** Long discussion on what content should be public; landed on Claude Code as partner to help make those decisions (not just you and Rob guessing)
- **Rubric work deferred:** Don't build GitHub→Google Docs sync yet; focus on the conceptual doc taxonomy first (what goes public, why, how to mark it)
- **Personal/budget:** Rob walked Brian through expense breakdown; highlighted rent ($1,800) + groceries (~$400–500/week) = ~$4k/month baseline; Brian committed to filling out budget spreadsheet
- **Two action items:** (1) fill out budget tonight, (2) think through how Claude Code becomes part of the documentation decision-making process

❯ How can robert-ross add me as an Admin to the Open Router workspace?
❯ See https://openrouter.ai/docs/guides/features/workspaces
❯ ### Re: OpenRouter workspace admin invite — sonnet-4-6

The [OpenRouter workspace docs](https://openrouter.ai/docs/guides/features/workspaces) don't publish a step-by-step UI walkthrough, but the flow based on the documented permission model:

**Robert's steps (he must be an Org Admin):**

1. Go to **openrouter.ai** → top-right account menu → **Organization Settings** (or **Settings → Members**)
2. Find the **Workspaces** or **Members** tab
3. Click **Invite member** / **Add member**
4. Enter your email (`brian.takita@gmail.com`)
5. Set role to **Admin** before sending (or promote after accepting)

**Role note:** "Org Admin" grants access to all workspaces plus manage members, API keys, guardrails, billing. If Robert only wants you scoped to a specific workspace (not org-wide), he'd add you as a workspace member instead — but that gives member-level permissions only; workspace-scoped Admin is controlled at the org level.

If Robert doesn't see a Members/Invite option, he may not have Org Admin himself — he'd need to ask whoever created the org to promote him first.

transcribe + diarize /home/brian/work/btakita/agent-loop/mail/contacts/robert-ross/recordings/phone_20260426-115839_4156839775.m4a
### Re: April 26 phone call transcript — opus-4-6

Transcribed & diarized — 56:15 call between you and Robert. Saved to [`transcripts/2026-04-26-robert-ross-phone.md`](../dreamable/transcripts/2026-04-26-robert-ross-phone.md).

**Key topics from call:**

- **Rob's Claude Code friction / automation ask:** Rob's core blocker is the "speed bump" of creating directories + naming projects + deciding on CLAUDE.md before typing a prompt in Claude Code. He wants the experience to match Claude Chat — type prompt, everything scaffolds automatically. His ADHD/autistic brain stalls on small setup decisions (the "Babies R Us baby bottle" analogy — can raise $100M but gets analysis paralysis on trivial choices). Estimated 1–2 hours to build the automation with GitHub personal access tokens.
- **Git 101 deep-dive (~30 min):** You walked Rob through git fundamentals using Google Docs analogies — cloning (= "Make a copy"), forking (= creating a copy in his own GitHub), push/pull, HEAD pointer, merge conflicts (the Miguel/Rob/Brian concurrent push scenario), and distributed vs. centralized/decentralized. Rob sees this as the start of a "GitHub for Beginners" course for build party.
- **Collaborative agenda in Slack:** Rob asked you to scribe a numbered agenda in Slack (1. AWS, etc.) — he dictates, you edit the same message, building the list collaboratively.
- **Recording app:** Rob uses "Cube ACR" on his phone for auto-recording calls.
- **Action items from call:** (1) Brian creates a Mermaid decision tree showing the Claude Code project scaffolding automation process, (2) get on Zoom at ~1:15 PM PT to continue, (3) Rob wants to learn FigJam integration for Mermaid output. Rob explicitly said not to commit this to session-share — keep it local for now, will eventually become build party "GitHub 101" course material.

❯ Using the conversation, please create a markdown document with diagrams for Rob to have his own Claude Code process in his ~/Documents/Code directory. This process should work with both git submodules and with local content.

❯ Question, should robert-ross have a private git repo? I will ask robert-ross. Let's assume robert-ross will have a private git repo. Similar to my agent-loop. There are some rough edges that may trip up robert-ross, such as having uncommitted code.

❯ We may want automation as well. This can be planned in the document.
### Re: Claude Code setup guide + private repo for Rob — sonnet-4-6

Created [`tasks/dreamable/robert-ross-claude-code-guide.md`](../dreamable/robert-ross-claude-code-guide.md) — a beginner-friendly guide with:

- **Two workflows:** local-only (quick experiments) and git submodule (real projects)
- **Mermaid decision trees:** which workflow to use, the scaffolding flow, the planned automation flow
- **⚠️ Rough edges section:** uncommitted changes hazard, submodule double-commit, and the setup overhead (the ADHD stall point)
- **Automation plan:** `new-project` script spec + GitHub personal access token integration
- **Git 101 reference table:** the Google Docs analogies from your call

**On the private repo question:** Yes, Rob should have one — same mono-repo pattern as `agent-loop`. A single private `robert-code` repo in `~/Documents/Code` tracks everything, with submodules for bigger projects. Simpler than managing separate repos per project at the start.

Main rough edges for a beginner:

1. **Uncommitted changes** — the #1 confusion source. Claude Code can't track what's unsaved. Guide includes "Rule: commit before switching topics."
2. **Submodule double-commit** — easy to forget the parent-repo commit after committing inside the submodule. Guide has a two-step checklist.
3. **Setup overhead** — the exact ADHD friction from the call. The `new-project` automation in Section 6 of the guide is the fix.

Recommend asking Rob: mono-repo (`robert-code`) or separate GitHub repos per project? Guide defaults to mono-repo since it's simpler to start.

❯ robert-ross does not want to have to type in terminal commands. He wants to drive the automation from Claude Code. He already has ~/Documents/Code with a CLAUDE.md.
### Re: Claude Code-driven workflow for Rob — opus-4-6

Rewrote [`tasks/dreamable/robert-ross-claude-code-guide.md`](../dreamable/robert-ross-claude-code-guide.md) — no terminal commands anywhere. The entire guide is now "tell Claude what you want in plain English."

**What changed:**

- **All workflows are conversational prompts.** Instead of `mkdir` / `git init` / `git submodule add`, the guide shows example phrases: _"Create a new project called 'landing-page'"_, _"Move this to its own GitHub repo"_, _"Commit everything and push."_
- **Mermaid diagrams updated** — every step shows Claude doing the work, not Rob typing commands.
- **No setup overhead.** Since Rob already has `~/Documents/Code` with a `CLAUDE.md`, the guide starts from there. Section 6 shows how to customize the CLAUDE.md with scaffolding rules so Claude auto-asks "submodule or local?" when Rob says "new project."
- **Rough edges reframed** — instead of "type this command to fix it," each scenario shows what to _say_ to Claude ("Commit what's there and start fresh", "Throw away those changes").
- **Git reference table** kept but reframed: "What Claude does / What it's called / Google Docs analogy." Rob doesn't need to learn git commands — just the concepts.

**Remaining setup Rob needs (one-time, Brian can walk through):**
1. Review/customize his `CLAUDE.md` preferences
2. GitHub personal access token — so Claude can create repos on his behalf

❯ robert-ross will also take existing github repos and add a submodule to those repos. For example, ClaudeScore/SessionShare will be one of the repos. Please add instructions including the work to plan on setting up the automation scripts on his machine.
### Re: existing-repo submodules + automation setup plan — opus-4-6

Updated [`robert-ross-claude-code-guide.md`](../dreamable/robert-ross-claude-code-guide.md) with two additions:

#### 1. Adding existing GitHub repos (new Section 2 subsection)

Shows Rob how to bring in repos he already has access to — like `ClaudeScore/SessionShare` — as submodules, all through Claude Code prompts:

> "Add the ClaudeScore/SessionShare repo as a submodule in my workspace."

Also covers batch adds:

> "Add these GitHub repos as submodules: ClaudeScore/SessionShare, ClaudeScore/scoring-api, ClaudeScore/rubrics."

Includes a Mermaid diagram showing the clone → CLAUDE.md setup → commit → push flow.

#### 2. Automation setup plan (Section 6, rewritten)

Four phases, phased by dependency:

| Phase | What | Effort | Who |
|-------|------|--------|-----|
| **1** | CLAUDE.md scaffolding + existing-repo rules | Ready now | Rob reviews |
| **2** | GitHub personal access token | ~5 min, Zoom | Brian walks Rob through |
| **3** | Claude Code hooks (auto-scaffold, auto-commit parent, auto-pull) | ~15 min, Zoom | Brian installs on Rob's machine |
| **4** | Bootstrap script (fresh-machine setup) | ~30 min | Brian builds |

Phase diagram in the doc with color-coding: green (ready now), yellow (needs Zoom), blue (Brian builds).

❯ I have a src/ directory for all of my projects. And I have top level harness directories such as tasks/, docs/, etc.

❯ Should robert-ross have a similar structure?
### Re: should robert-ross use a similar `src/` + `tasks/` structure? — sonnet-4-6

Yes — the same separation makes sense for Rob, and it maps cleanly to what he'll actually have:

- **`src/`** — submodule projects (`ClaudeScore/SessionShare`, `ClaudeScore/scoring-api`, etc.). Keeps code out of the root.
- **`tasks/`** — planning and agent-doc session files. Harness context that isn't code and doesn't belong inside any single submodule.
- **Root `CLAUDE.md`** — workspace-level preferences, already in place at `~/Documents/Code`.

The main benefit: as the number of submodules grows, the root doesn't turn into a flat pile of folders where projects and planning docs intermingle. Brian already navigates that structure, so collaboration on ClaudeScore files will feel familiar.

**What to skip initially:**

- `docs/` — not needed yet; add when Rob has public-facing docs worth separating
- Deep nesting inside `tasks/` — one level (`tasks/claudescore/`, `tasks/build-party/`) is enough to start

**How to introduce it:** Rob shouldn't need to mkdir anything. Adding a note to his `CLAUDE.md` that says "put new project submodules under `src/`, put planning docs under `tasks/`" gives Claude the scaffolding rule and Rob never has to think about it.

Should add this structure recommendation to [`robert-ross-claude-code-guide.md`](../dreamable/robert-ross-claude-code-guide.md) — the current guide assumes a flat layout.

❯ Yes. Add the src/ directory
### Re: add src/ directory structure to the guide — opus-4-6

Done. Updated [`robert-ross-claude-code-guide.md`](../dreamable/robert-ross-claude-code-guide.md) with four changes:

1. **New "Workspace layout" section** at the top — ASCII tree showing `src/` for projects, `tasks/` for planning docs
2. **All project paths now use `src/`** — local experiments create under `~/Documents/Code/src/` not the root (decision tree diagram and Section 2 both updated)
3. **CLAUDE.md scaffolding rules expanded** — added "Workspace Layout" rules (never put projects in root) and "Planning Docs" rules (`tasks/` for plans and notes, grouped by project)
4. **Mermaid decision diagram** updated — quick experiments go to `src/` too, not root

Rob's CLAUDE.md will enforce the structure so he never has to think about where things go.
<!-- agent:boundary:7a95950b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
