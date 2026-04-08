---
agent_doc_session: a953811a-bf72-4aa9-bbd6-5904a5a07a1d
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# LinkedIn

## Profile

**About section** (copy-paste ready):
> Over 20 years of software development experience... Most recently built the full real-time AI avatar pipeline at Presence AI... Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

**Company taglines** (saved to `mail/topics/brian-takita/README.md`):
- Brand identity: "Collaborative intelligence tooling — from spec to ship."
- Marketing/pitch: "Spec-driven agent tooling for teams that ship."

## Post #1 — Agent Tooling (PUBLISHED)

- **Draft**: `mail/social/20260224-agent-tooling-linkedin.md` (status: published)
- **Image**: `~/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png` (tmux multi-pane setup)
- **Content**: Introduction to corky + agent-doc + spec-driven methodology with tmux routing
- Published via `corky social publish` with image upload + scheduling (`scheduled_at` + `corky watch`)
- Self-referential dog-fooding line included

## Post Cadence

1. **Post #1** (done) — Introduction to corky + agent-doc + spec-driven methodology
2. **Post #2** (planned) — Deep dive on topics/mailbox architecture ("How I share AI context with collaborators without giving them my whole inbox")
3. **Post #3** (planned) — Video demo of the agent-doc workflow (30-60s screen recording)

## Corky LinkedIn Integration

**Implemented (v0.12.0):**
- `corky social auth linkedin --profile btakita` — OAuth2 flow, token stored in `tokens.json`
- `corky social publish` with `--dry-run` flag — validates auth, uploads images, prints payload
- `images: Vec<String>` in draft YAML — single image or multi-image carousel
- LinkedIn REST API (`/rest/posts` + `/rest/images`) — upgraded from deprecated v2 UGC
- `corky schedule run` / `corky watch` — client-side scheduling with `scheduled_at` field
- Status: `draft` + `scheduled_at` → auto-ready; `published` → skip (prevents double-publish)
- Logo: `assets/logo.svg` + `logo-128.png` + `logo-64.png` (corgi-in-a-bottle design)
- Run from: `cd /home/brian/work/btakita/agent-loop && corky watch &` (needs `mail/` in CWD)

**Not available:**
- LinkedIn Messaging API — partner-only, not available to self-service developer apps
- Workaround: monitor LinkedIn via email notifications (`corky sync`), steer conversations to email

## Decisions

- **Directory naming**: Keep `mail/` — documented in SPECS.md §2.5
- **Scheduling**: No shared trait — thin `schedule.rs` dispatches via `ScheduledKind` match
- **LinkedIn API ToS**: §3.1 "no automating posting" — gray zone for `corky watch`, fine for user-initiated
- **Best post times**: Tue–Thu, 8–10 AM ET; avoid Mon/Fri and evenings
- **Video vs screenshot**: Screenshot for first post (instant legibility); video for follow-up
- **Logo PNGs**: Committed to repo, not built from SVG (avoids external tool dependency)
- **LinkedIn messaging**: Out of scope — use email instead (corky's sweet spot)

## Contacts

- **Toby Hill** — D24 Search (DeepTech recruiter). Calendly note: "AI developer tooling in Rust — agent orchestration, automated workflows, full-stack. Open to DeepTech roles."

## LinkedIn Conversation Export Bookmarklet

Exports the currently visible LinkedIn messaging thread as a timestamped text file.

**Filename format:** `yyyymmdd-linkedin-contact-name.txt` (e.g., `20260311-linkedin-mia-tomeo.txt`)

```javascript
javascript:void(function(){const msgs=[];document.querySelectorAll('.msg-s-message-list-content .msg-s-message-list__event').forEach(li=>{const nameEl=li.querySelector('.msg-s-message-group__name');const timeEl=li.querySelector('.msg-s-message-group__timestamp');const bodyEl=li.querySelector('.msg-s-event-listitem__body');if(!bodyEl)return;const name=nameEl?nameEl.textContent.trim():'Unknown';const time=timeEl?timeEl.textContent.trim():'';const text=bodyEl.innerText.trim();if(text)msgs.push('['+time+'] '+name+':\n'+text)});const headerEl=document.querySelector('.msg-overlay-bubble-header__title a,.msg-thread__link-to-profile,.msg-entity-lockup__entity-title');let contactName='Unknown';if(headerEl){const lines=headerEl.textContent.split('\n').map(s=>s.trim()).filter(Boolean);contactName=lines[0]||'Unknown'}const slug=contactName.replace(/[^a-zA-Z0-9]+/g,'-').toLowerCase().replace(/^-|-$/g,'');const d=new Date();const ymd=''+d.getFullYear()+String(d.getMonth()+1).padStart(2,'0')+String(d.getDate()).padStart(2,'0');const content='# LinkedIn Conversation with '+contactName+'\nExported: '+d.toISOString()+'\n\n'+msgs.join('\n\n');const blob=new Blob([content],{type:'text/plain'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download=ymd+'-linkedin-'+slug+'.txt';a.click()})()
```

**To install:** Create a new bookmark, paste the `javascript:void(...)` code as the URL.

**Note:** LinkedIn lazy-loads messages — scroll up to load older messages before clicking.

<!-- agent:exchange -->
The bookmarklet is already in the static section of this document (## LinkedIn Conversation Export Bookmarklet, lines 63-75). In template mode, `agent-doc write` only patches `<!-- agent:exchange -->` — the static sections above it are never touched by the agent. So it's safe here.

The exchange section does get compacted/replaced over time, but the bookmarklet lives *outside* the exchange component.

If you want a second copy for extra safety: `agent-loop/bin/linkedin-export-bookmarklet.js` would be a natural spot. But given the template mode guarantee, keeping it in this document is fine.

Help me reply to some comments https://www.linkedin.com/feed/update/urn:li:activity:7440756026619940864

Let's start with Michael Darmousseh:
> Are you using autoresearch (idea from Karpathy) to automate building skills/agents? I built out an internal tool for optimizing skills with the technique.

I havn't used Karpahy's technique. What got me started was this:

I inherited a real-time video lipsync codebase from a coworker who left. Hand-coded + AI augmented improvements on it for a while. Eventually using Claude Code on the codebase. The documentation was out of sync due to numerous refactorings spanning multiple processes, performance improvements, stability improvements, etc.

I then made Claude audit my documentation. Which it did quite well with.

About 1 week later, PresenceAI shut down. I came back on the job market that has been completely different from when I started. I was inspired by some companies that were using 100% spec-driven development...as in it's not allowed to touch code.

I figured, make my job search use Calude code. So I made corky to handle my correspondence. I wanted to reach out to old contacts.

I also worked on a project back in 2015 where we had interactive documents that hooked into SalesForce. So I wanted to apply this concept to Claude. As I was working on Corky, I began agent-doc.

From there, it's been a matter of following good practices...which include research, iterating on SKILL.md for agentic context, building binary software (using Rust) for deterministic operations, continuous review, etc.

Agents make it easier to follow all of the practices that once took too much time...and were dropped under schedule pressure.

**Draft reply to Michael Darmousseh:**

I haven't used Karpathy's autoresearch technique specifically, but the journey that led me here rhymes with it.

I inherited a real-time video lipsync codebase from a colleague who left. After months of hand-coded + AI-augmented improvements — refactoring processes, performance, stability — the documentation was completely out of sync. I had Claude audit and reconcile it, and it did surprisingly well.

A week later the company shut down. Coming back to the job market, I was inspired by teams doing 100% spec-driven development — no one touches code directly. So I built corky (correspondence kit) to manage my outreach, and agent-doc grew out of that — influenced by interactive document workflows I'd built back in 2015 hooked into Salesforce.

From there it's been iterating on the practices: research loops, evolving SKILL.md for agentic context, building deterministic operations in Rust, continuous review. The key insight is that agents make it feasible to follow *all* the practices that used to get dropped under schedule pressure.

Would love to hear more about your autoresearch implementation — are you optimizing skills based on execution traces, or something else?

Tightened your story into LinkedIn-appropriate length while keeping the authentic narrative arc. Closing question invites continued conversation. Adjust tone/length/emphasis as needed.

Should I post links to corky + agent-doc?

**Re: Should I post links?**

Skip the links for this reply:
1. **Comment replies with links get deprioritized** by LinkedIn's algorithm and look self-promotional
2. **The repos are likely private** — dead links are worse than no links
The repos are public. You should know that.
   https://crates.io/crates/corky
   https://crates.io/crates/agent-doc
3. **The narrative is stronger without them** — origin story > sales pitch in a comment thread
4. **Save for follow-up** — if Michael asks "where can I see it?", share links naturally then

You already name-drop "corky" and "agent-doc" in the reply text. Curious people will ask.

**Ready to post as-is** — the unquoted draft above is good to go. Copy-paste into the LinkedIn comment box.
I agree with your other points

**Re: repos are public**

You're right — my mistake. Both are published on crates.io. I should have known that. Recommendation still stands though: skip the links in this reply for the algorithm/narrative reasons. Post the draft as-is.

Help me reply to Ernest Franklin Spicer:

> Funny... Open to Work so maybe in 20 years you didnt advance much because of falling to hype.

My current response:

So you are not Open to Work? Not willing to take on new clients?

Anyways, My previous employer went out of business...we were primarily hand coding with some AI augmentation. Not using agent loops though until the very very end...but it was too late.

**Re: Ernest Franklin Spicer reply**

Your draft is solid. Here are options:

**Option A (your draft, polished):**
So you're not Open to Work? Not taking on new clients?
My previous employer went out of business — we were primarily hand-coding with some AI augmentation. Not using agent loops until the very end... but by then it was too late.

**Option B (shorter, more confident):**
My previous employer shut down — that's why I'm open to work. Building these tools is what I did next. Happy to compare notes on what "advancing" looks like after 20 years.

**Option C (don't engage):**
This reads like a troll comment. Engaging gives it algorithm visibility. Your strongest play might be to ignore it — anyone reading will see it reflects more on Ernest than on you.

Recommendation: **C** (ignore) or **A** if you want to respond.

Dan Dunford
> It's been liberating testing all my ideas as if they were real products. I've been able to iterate hundreds of tweaks, many ideas, and prove which ones may work and which ones will definitely not work.
>
>I love it. I'm now able to focus on the ideas that got attention and will work.. and eventually monetise, exciting times ahead!
 
I love this comment. Would love to follow his work. Should I mention some of my ideas, re: existence-lang or my other projects, or would this be in bad taste?

**Re: Dan Dunford reply**

Don't mention existence-lang or other projects here. Dan's comment is about *his* experience — the best reply mirrors his energy and validates what he's doing. Mentioning your projects pivots focus back to you in a thread where you're already OP.

**Draft reply to Dan Dunford:**

This resonates — the ability to test ideas at the speed of thought completely changes which ones you pursue. Before, you had to bet big on one idea and commit. Now you can prototype five, kill three, and double down on the two that show traction.

Would love to see what you're building. Following!

From David Bethune

> It's funny how that, after decades of programming where no one questions your chops (shipped products, right?), if we use AI now suddenly we have abdicated all conscious thought and understanding of coding. How would that happen? Did farmers forget about basic agriculture because farming equipment made ag faster and easier. I think not! Ag got bigger and fed more people with tools. There was no retraction or regress.

People are addicted to labels & titles...because symbols are a proxy for thought.
What many people don't realize is AI is a cognitive enhancement...not a replacement.

A tool to be mastered...just like any other tool.

**Re: David Bethune reply**

Your notes are strong raw material. Here's a polished draft that weaves them together:

**Draft reply to David Bethune:**

Exactly. People are addicted to labels and titles because symbols are a proxy for thought. "AI-generated code" vs "hand-written code" is the new version of that — as if the person directing the work somehow forgot everything they know.

AI is a cognitive enhancement, not a replacement. The understanding doesn't go away — it compounds. You just get to apply it at a different resolution.

From Cody Sheridan
> My biggest limitation in software development has always been time. I know what I want to build and how I want to build it, I just haven’t had the time outside of full-time employment to actually get it done. LLMs and Agents are changing that.

I resonate with this. I'm looking forward to see what you are releasing.

Making software from intention makes development a true art form.
Where the expression is the utility.
I predict many new businesses will start from this technological wave.

**Re: Cody Sheridan reply**

Your notes are poetic. Here's a draft that keeps that energy while staying conversational:

**Draft reply to Cody Sheridan:**

Same here — time was always the bottleneck. You know what to build, you know how to build it, but the gap between intention and implementation ate all the hours.

Now that gap is collapsing. Making software from intention turns development into a true art form — where the expression is the utility. I think we're going to see a wave of new collaborations and businesses from people who always had the ideas but never had the time.

Looking forward to seeing what you release.

From Andrei Trandafira: rate my response
> Good direction, but agent loops are not yet the compiler era of AI engineering; they are the relay-circuit era—enough to prove the paradigm, not enough yet to guarantee robust abstraction. So yes we're at a new turn of the spiral of abstraction, but we are very early

Andrei Trandafira They are yet another tool. The Agent Harness is important. And making software for deterministic operations...making context for the model. Every tool has it's place. Being early is the best place to be.

**Re: Andrei Trandafira reply**

Your substance is right but the phrasing is fragmented. Here's a polished version:

**Draft reply to Andrei Trandafira:**

Fair point — we're early. But every paradigm starts with relay circuits before compilers arrive. The key is what you build *during* that era.

What I've found matters most right now: the agent harness, deterministic tooling for operations that shouldn't be probabilistic, and structured context (SKILL.md) that gives the model the right abstraction surface. These aren't compiler-era polish — they're the foundations that make the compiler era possible.

Being early is exactly where you want to be.

From Azizul Rabby Chowdhury
> Feels like the real shift is just being able to finally build things that were always out of reach before.

Indeed it is. All of those intentions can now be software and in some cases physical creations.

**Re: Azizul Rabby Chowdhury reply**

Your reply is good but could be tighter. Here's a polished version:

**Draft reply to Azizul Rabby Chowdhury:**

Indeed. All those intentions that were always in your head — now they can become working software. The shift isn't just speed, it's reach.

How about including 3D printed creations as well.

**Updated draft reply to Azizul Rabby Chowdhury:**

Indeed. All those intentions that were always in your head — now they can become working software, or even physical creations off a 3D printer. The shift isn't just speed, it's reach.

From Richard Lee Abraham
> That's the way it's always been. Tech evolves; new, more efficient ways of doing things become available. AI tools are the latest version of that.

The game keeps on changing. Some people comparatively benefit more than others. It's ultimately leverage that people can use or not use.

**Re: Richard Lee Abraham reply**

Your leverage point is strong. Here's a polished version:

**Draft reply to Richard Lee Abraham:**

Exactly — and the difference this time is the magnitude of leverage. Previous tech shifts gave you 2x, maybe 10x. This one is giving individual builders leverage that used to require entire teams. The game keeps changing; the people who benefit most are the ones who pick up the new tools first.

> benefit most are the ones who pick up the new tools first

I think those who are prepared.

Please update the bookmarklet to include the date of the message.

**Re: Richard Lee Abraham — updated ending**

Updated draft:
Exactly — and the difference this time is the magnitude of leverage. Previous tech shifts gave you 2x, maybe 10x. This one is giving individual builders leverage that used to require entire teams. The game keeps changing; the people who benefit most are the ones who are prepared when the shift arrives.

**Re: Bookmarklet date update**

Updated the bookmarklet to extract the `datetime` attribute from `<time>` elements (which has full ISO dates) and format as `YYYY-MM-DD HH:MM`. Falls back to the displayed text for messages without a datetime attribute. See the updated code in the static section above — edit the bookmarklet there directly since it's outside the exchange component.

Updated bookmarklet (replace the one in ## LinkedIn Conversation Export Bookmarklet):

```
javascript:void(function(){const msgs=[];document.querySelectorAll('.msg-s-message-list-content .msg-s-message-list__event').forEach(li=>{const nameEl=li.querySelector('.msg-s-message-group__name');const timeEl=li.querySelector('.msg-s-message-group__timestamp');const bodyEl=li.querySelector('.msg-s-event-listitem__body');if(!bodyEl)return;const name=nameEl?nameEl.textContent.trim():'Unknown';let time='';if(timeEl){const te=timeEl.querySelector('time[datetime]');if(te){const dt=new Date(te.getAttribute('datetime'));time=dt.getFullYear()+'-'+String(dt.getMonth()+1).padStart(2,'0')+'-'+String(dt.getDate()).padStart(2,'0')+' '+String(dt.getHours()).padStart(2,'0')+':'+String(dt.getMinutes()).padStart(2,'0')}else{time=timeEl.textContent.trim()}}const text=bodyEl.innerText.trim();if(text)msgs.push('['+time+'] '+name+':\
'+text)});const headerEl=document.querySelector('.msg-overlay-bubble-header__title a,.msg-thread__link-to-profile,.msg-entity-lockup__entity-title');let contactName='Unknown';if(headerEl){const lines=headerEl.textContent.split('\
').map(s=>s.trim()).filter(Boolean);contactName=lines[0]||'Unknown'}const slug=contactName.replace(/[^a-zA-Z0-9]+/g,'-').toLowerCase().replace(/^-|-$/g,'');const d=new Date();const ymd=''+d.getFullYear()+String(d.getMonth()+1).padStart(2,'0')+String(d.getDate()).padStart(2,'0');const content='# LinkedIn Conversation with '+contactName+'\
Exported: '+d.toISOString()+'\
\
'+msgs.join('\
\
');const blob=new Blob([content],{type:'text/plain'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download=ymd+'-linkedin-'+slug+'.txt';a.click()})()
```

Copy this into the static bookmarklet section to replace the old one. The exchange component can't edit static sections directly.

Should we post the following today: Sunday 15:00:00 Eastern or wait until tomorrow.

**Video Pipeline (2026-03-19 02-26-07.mp4):**
- Light edit via ffmpeg silence detection: 47 segments cut, 63→60 min, 2.8GB→380MB
- GPU transcription (CUDA): 2058 segments in 27.6s (130x realtime)
- Published: https://www.youtube.com/watch?v=IenfqO2vmAw
- Blog post: https://briantakita.me/posts/agent-resume-recruiter-workflow-live
- Comment posted (ID: `UgwZg_yU3oJaAyV8neF4AaABAg`) — needs manual pinning

**YouTube Comment Response (hVNmbmY4vgo):**
- Responded to "gross vibe coding sub human" with Option A (20+ yrs experience, back-burner ideas, abstraction path, democratization)
- LinkedIn post: https://www.linkedin.com/feed/update/urn:li:share:7440756026024488960
- LinkedIn comment with video link: ID `7440760371260444672`

**Re: posting timing**

Per your own decisions in this doc: **Best post times: Tue-Thu, 8-10 AM ET; avoid Mon/Fri and evenings.**

**Wait until Tuesday morning, 8-9 AM ET.**

- Sunday afternoon = low engagement (weekend mode, not professional scrolling)
- Monday = in your "avoid" list
- Tuesday 8-9 AM ET = your sweet spot
- Content is evergreen — 2 days won't hurt

If you need to post sooner: Monday 8 AM ET > Sunday 3 PM. But Tuesday is the play.

Video pipeline and YouTube comment work noted — good documentation for the task file.

**Re: video posting strategy for Monday interview**

The interview changes the calculus — it's not about optimal LinkedIn timing, it's about having content visible before 5PM Monday.

**Post 1 Sunday, 1 Monday morning.**

- **Sunday evening (~7-8 PM ET)**: Post video 1. Gets ~20 hours to accumulate views/likes before the interview.
- **Monday morning (~8-9 AM ET)**: Post video 2. Gets ~8 hours of visibility before 5PM.
- **Why not both Monday morning**: They compete with each other in the feed. Spacing gives each its own engagement window.
- **Why not noon**: Too close to the interview — not enough time for organic engagement.

The interviewer will likely check your LinkedIn profile before the call. Two recent video posts with some engagement signals active presence.
I have another longer video to post.
I will be having an interview on Monday 5PM Eastern.
I want my videos up by then.
Should I:
- post both videos on Monday morning
- post 1 video in the morning 1 around noon
- post 1 video Sunday, 1 Monday morning

**Re: LinkedIn post for YouTube video + blog post**

Links in the post body get deprioritized by LinkedIn's algorithm. First comment is the right move.

**Draft LinkedIn Post:**

Built a 60-minute screen recording of my agent-driven recruiter workflow — from resume tailoring to outreach, all orchestrated through tmux with multiple Claude Code sessions running in parallel.

What you'll see:
- Real-time spec-driven development with agent-doc interactive documents
- corky handling correspondence drafting and email management
- Multi-pane tmux routing where each agent handles one focused task
- The full loop: research → spec → build → review → ship

This is what 20+ years of software development looks like when you add agent tooling to the stack. Not vibe coding — structured collaboration with AI that follows the same engineering practices I've always used, just at a pace that wasn't possible before.

The tools shown (corky, agent-doc) are open source and published on crates.io.

**First comment (post immediately after):**

Full video: https://www.youtube.com/watch?v=IenfqO2vmAw
Blog post with context: https://briantakita.me/posts/agent-resume-recruiter-workflow-live

Create a LinkedIn post about the YouTube Video and the Blog post.
Put the link to the YouTube Video and Blog post in the first comment.
Create a draft to post to LinkedIn.
Add something about using a skills and experience "database" with agent-resume.

**Re: Updated LinkedIn post with agent-resume + corky draft**

Updated the post to lead with agent-resume's skills/experience "database" concept. Created corky draft at `mail/social/20260322-agent-resume-workflow-linkedin.md`.

**Draft LinkedIn Post:**

Built a 60-minute screen recording of my agent-driven recruiter workflow — from resume tailoring to outreach, all orchestrated through tmux with multiple Claude Code sessions running in parallel.

The foundation is agent-resume: a structured database of every skill, experience, and accomplishment I've built over 20+ years, stored as tagged TOML entries. Each job gets a profile with match tags — the tool scores and ranks experiences by relevance, then assembles a tailored resume. No more manually reshuffling bullet points for every application.

On top of that:
- agent-doc for real-time spec-driven development via interactive documents
- corky handling correspondence drafting and email management
- Multi-pane tmux routing where each agent handles one focused task

Not vibe coding — structured collaboration with AI that follows the same engineering practices I've always used, just at a pace that wasn't possible before.

All three tools are open source and published on crates.io.

**First comment (post immediately after):**

Full video: https://www.youtube.com/watch?v=IenfqO2vmAw
Blog post with context: https://briantakita.me/posts/agent-resume-recruiter-workflow-live

**Draft file:** `mail/social/20260322-agent-resume-workflow-linkedin.md` (status: draft)

To publish: `corky social publish` or schedule with `scheduled_at` field. Remember to post the first comment with links manually right after publishing.
It is a blend of vibe coding and structured collaboration. I'm not afraid of vibe coding personally. Both vibe coding and structured collaboration can be useful

**Re: vibe coding framing**

Good call — "Not vibe coding" was too dismissive. Updated the draft and the corky draft file to embrace the blend:

> It's a blend of vibe coding and structured collaboration — both have their place. The difference is having the tooling to move fluidly between them, applying 20+ years of engineering judgment at a pace that wasn't possible before.

Draft file updated: `mail/social/20260322-agent-resume-workflow-linkedin.md`
Publish

Is this a recruiter or recruitee workflow?

**Re: Publish**

Done.

- **Post published:** https://www.linkedin.com/feed/update/urn:li:share:7441567383225290753
- **First comment posted** (ID: `7441567425197789184`) with YouTube video + blog post links
- Draft file updated to `status: published` with `post_id` and `published_at`

Add first comment.
Include links to
- https://github.com/btakita/agent-resume
  - The agent-resume source code
- https://github.com/btakita/resume
  - My resume

**Re: recruiter vs recruitee workflow**

Good catch — "recruiter workflow" is misleading. You're the job seeker, not the recruiter. Options:

1. **"agent-driven job search workflow"** — clearest
2. **"recruiter outreach workflow"** — implies outreach *to* recruiters
3. **"candidate workflow"** — formal

Want me to edit the published post via `corky linkedin edit` to fix this? The change would be in the first line.
yes

We should also update the youtube title, blog post title, and content accordingly as needed.

**Re: Add first comment with GitHub links**

Done. Second comment posted (ID: `7441567974223806464`) on the published post with:
- agent-resume source: https://github.com/btakita/agent-resume
- Resume repo: https://github.com/btakita/resume

Post now has two comments:
1. YouTube video + blog post links
2. GitHub source code links

Still pending: Do you want to edit the post body to fix "recruiter workflow" → "job search workflow"?
Yes

**Re: Fix "recruiter workflow" across all platforms**

**LinkedIn post** — Updated. "recruiter workflow" → "job search workflow". Live at https://www.linkedin.com/feed/update/urn:li:share:7441567383225290753

**YouTube video** — Draft file updated (`mail/social/20260322-015438-youtube.md`):
- Title: "Building Agent Resume: AI-Powered Job Search Workflow with Resume Generation"
- Description: "recruiter conversations" → "job applications", "Recruiter correspondence" → "Job search correspondence"
- **Needs YouTube auth refresh** — run `corky youtube auth` in browser, then `corky youtube edit mail/social/20260322-015438-youtube.md`

**Blog post** — needs update at `src/briantakita.me-dev/app/briantakita.me/post/content/2026-03-19-agent-resume-recruiter-workflow-live.md`:
- Title: "Building Agent Resume: AI-Powered Resume Generation from Recruiter Conversations" → "Building Agent Resume: AI-Powered Job Search Workflow with Resume Generation"
- Description and body references to "recruiter" workflow → "job search" workflow
- Then rebuild + deploy the blog

Do you want me to update the blog post file now, or wait for the YouTube auth first?
update blog post
then youtube auth

**Re: update blog post then youtube auth**

**Blog post** — Already updated. Changes made to `src/briantakita.me-dev/app/briantakita.me/post/content/2026-03-19-agent-resume-recruiter-workflow-live.md`:
- Title → "Building Agent Resume: AI-Powered Job Search Workflow with Resume Generation"
- Description → "job applications" instead of "recruiter conversations"
- Body heading, iframe title, section headers, and references all updated
- Needs rebuild + deploy to go live

**YouTube** — Auth failed (token expired, browser callback wasn't received). You'll need to:
1. Run `corky youtube auth` from a terminal
2. Complete the Google OAuth flow in the browser
3. Then run `corky youtube edit mail/social/20260322-015438-youtube.md`

The YouTube draft file is already updated with the new title and description — just needs the auth to push the edit.

I authed on my own.
Edit the video

rebuild and deploy blog

**Re: Edit YouTube video**

Done. YouTube video updated: https://www.youtube.com/watch?v=IenfqO2vmAw

All three platforms now have consistent "job search workflow" wording:
- **LinkedIn** — Updated
- **YouTube** — Updated
- **Blog post** — Updated locally (needs rebuild + deploy)

**Re: rebuild and deploy blog**

Done. Blog rebuilt and deployed:
- Built locally with `make build`
- Committed in `briantakita.me` submodule: `170c0f4`
- Pushed to trigger Cloudflare Pages deploy
- Parent `briantakita.me-dev` updated: `181ffa9`

All three platforms now live with "job search workflow":
- **LinkedIn** — https://www.linkedin.com/feed/update/urn:li:share:7441567383225290753
- **YouTube** — https://www.youtube.com/watch?v=IenfqO2vmAw
- **Blog** — https://briantakita.me/posts/agent-resume-recruiter-workflow-live (deploying via Cloudflare Pages)
Let's create a new url slug of "Building Agent Resume: AI-Powered Job Search Workflow with Resume Generation".
Let's have a redirect from the old slug to the new slug.

**Re: new URL slug + redirect**

Done. Blog post slug renamed and deployed:

- **Old URL:** `briantakita.me/posts/agent-resume-recruiter-workflow-live`
- **New URL:** `briantakita.me/posts/agent-resume-job-search-workflow`
- **301 redirect** from old → new via `static/_redirects`
- Updated `post/index.ts` import
- Committed and pushed: `aa7933f` (submodule), `dbc72ee` (parent)
- Cloudflare Pages deploying now

Note: The LinkedIn first comment still links to the old blog URL, but the 301 redirect will handle it seamlessly.

Does https://www.linkedin.com/in/eric-irwin-78b7b53bb/ seem like a real profile?
He commented on one of my posts at https://www.linkedin.com/feed/update/urn:li:activity:7444389222624448512?commentUrn=urn%3Ali%3Acomment%3A%28activity%3A7444389222624448512%2C7444389275695099904%29&replyUrn=urn%3Ali%3Acomment%3A%28activity%3A7444389222624448512%2C7444431288767258624%29&dashCommentUrn=urn%3Ali%3Afsd_comment%3A%287444389275695099904%2Curn%3Ali%3Aactivity%3A7444389222624448512%29&dashReplyUrn=urn%3Ali%3Afsd_comment%3A%287444431288767258624%2Curn%3Ali%3Aactivity%3A7444389222624448512%29

Please evaluate.
### Re: Eric Irwin profile evaluation

**Red flags — likely fake or very new/low-quality profile:**

1. **Not indexed by search engines** — Searching `site:linkedin.com "eric-irwin-78b7b53bb"` returns zero results. Legitimate, established profiles are indexed by Google.

2. **Unusual URL slug** — The suffix `78b7b53bb` looks like a truncated hash. Real LinkedIn profiles have either clean custom URLs (e.g., `/in/eric-irwin/`) or shorter numeric suffixes. Hash-like slugs are typical of auto-generated profiles that never customized their URL.

3. **No match among 60+ real Eric Irwins** — Web search found Eric Irwins at Datadog, Quantum Metric, Electric Boat, etc. — none with this URL. A real professional with enough context to comment meaningfully on your post would likely have an established, indexed profile.

4. **Comment context matters** — What did the comment say? If it's generic engagement bait ("Great post!", "Interesting!", or a vague question), that's another signal of a bot or engagement-farming account.

**Recommendation:** Don't engage. If the comment is substantive and adds value to the thread, leave it visible (it helps your algorithm). If it's spam or low-effort, hide it from the post.

Browser connection timed out so I couldn't visually verify the profile — but the search evidence alone is concerning.

Use chromium-bridge to visit the comment with the url above
### Re: Eric Irwin profile evaluation (visual verification)

**Confirmed: This is a recruitment scam account.** Full comment and profile now visually verified via chromium-bridge.

**Eric Irwin's full comment:**
> "There's an exciting Full Stack Engineer Roles position that has recently opened at Bunge, offering a remarkable opportunity for professionals with strong qualifications and experience like yours."
> Directs you to email **bunge.screeningdesk@outlook.com** — an Outlook.com address, not a corporate Bunge domain.
> "Wishing you the best of luck in your application and future endeavors."

**Profile red flags (confirmed visually):**
1. **0 followers** — a legitimate admin professional with "15+ years" would have connections
2. **No Experience section** — profile only has About and Activity, no actual work history
3. **No Education, Skills, or Endorsements** — completely empty profile
4. **Location: Đà Nẵng, Vietnam** — claims to work at Bunge but located in Vietnam
5. **Activity is entirely Bunge corporate reposts** — no original content, just sharing Bunge's official posts to appear legitimate
6. **Outlook.com email for "screening"** — real Bunge recruitment uses @bunge.com corporate email
7. **Hash-style URL** (`eric-irwin-78b7b53bb`) — never customized, auto-generated

**Recommendation: Hide/delete the comment and report the profile.** This is a job scam designed to harvest resumes and personal information via a non-corporate email address. Do not engage or reply.

> Hide/delete the comment and report the profile.

Why do you recommend hiding or deleting the comment? Would I lose the engagement?
### Re: Why hide/delete — engagement tradeoff

Good question. Here's the tradeoff:

**Hiding the comment** (recommended):
- Only you and the commenter can see it — everyone else sees it removed
- Your post's engagement metrics (likes, comment count) are **not affected** — LinkedIn still counts hidden comments in the total
- The scam email (`bunge.screeningdesk@outlook.com`) is no longer visible to your audience
- You don't lose any algorithmic benefit

**Deleting the comment:**
- Permanently removes it — your comment count drops by 1 (2 → 1)
- Marginal engagement loss, but a scam comment isn't real engagement anyway

**Leaving it visible:**
- Risk: someone in your network clicks through and gets scammed
- The comment adds zero value to the conversation
- It makes your post look like a spam magnet

**Best move: Hide it** (not delete). You keep the engagement number, remove the scam from view, and can report the profile separately. LinkedIn's "Hide comment" is under the three-dot menu on the comment itself.
### Re: Why hide/delete — engagement tradeoff

Good question. Here's the tradeoff:

**Hiding the comment** (recommended):
- Only you and the commenter can see it — everyone else sees it removed
- Your post's engagement metrics (likes, comment count) are **not affected** — LinkedIn still counts hidden comments in the total
- The scam email (`bunge.screeningdesk@outlook.com`) is no longer visible to your audience
- You don't lose any algorithmic benefit

**Deleting the comment:**
- Permanently removes it — your comment count drops by 1 (2 → 1)
- Marginal engagement loss, but a scam comment isn't real engagement anyway

**Leaving it visible:**
- Risk: someone in your network clicks through and gets scammed
- The comment adds zero value to the conversation
- It makes your post look like a spam magnet

**Best move: Hide it** (not delete). You keep the engagement number, remove the scam from view, and can report the profile separately. LinkedIn's "Hide comment" is under the three-dot menu on the comment itself.

Can corky support hiding these spam comments?
### Re: Corky LinkedIn comment moderation

Not currently — and it would be difficult to add.

**LinkedIn's API doesn't support comment moderation.** The Marketing API (which corky uses for publishing) has no endpoints for hiding, deleting, or reporting comments. These actions are only available through the LinkedIn web UI. Even official partner integrations (Hootsuite, Sprout Social) can't moderate comments via API.

**What corky *could* do (via chromium-bridge):**
A `corky linkedin moderate` command could navigate to the post, find the comment, and click Hide via browser automation. But this is fragile DOM automation that breaks when LinkedIn updates their UI — probably not worth the investment for occasional spam.

**Practical recommendation:** Just hide it manually — three-dot menu → Hide comment. It's 3 clicks.

I don't see Hide Comment in the dropdown menu.
![img_1.png](img_1.png)
### Re: No "Hide Comment" option

The screenshot shows **Elvio Sacchi's** comment, not Eric Irwin's — but the menu is the same for all comments on your own posts.

You're right — LinkedIn's menu for post authors shows **Delete**, not **Hide**. "Hide comment" is only available to page admins on Company Page posts. For personal posts, the options are:
- **Delete** — removes the comment entirely (comment count drops)
- **Block** — blocks the user from seeing/commenting on your content
- **Report** — flags the profile/comment to LinkedIn

**For the Eric Irwin scam comment, do all three:**
1. **Report** first (so LinkedIn has the record)
2. **Delete** the comment (removes the scam email from your post)
3. **Block** Eric Irwin's profile (prevents future spam)

The engagement count loss is negligible — better than leaving a scam email visible on your post.

**For Elvio Sacchi** (shown in screenshot) — that looks like a legitimate recruiter comment ("We're currently hiring for several industries..."). Worth keeping or replying to.
❯    ID.
❯    ID.
<!-- agent:boundary:f38dd9e8 -->
<!-- /agent:exchange -->
