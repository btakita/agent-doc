---
agent_doc_session: a953811a-bf72-4aa9-bbd6-5904a5a07a1d
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
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
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/1a4be6001948fa0de4b146422b07508d0fe80e429960172131103fa8c5273594-20260626-151510.md`*

Compacted content:
- Archived 6 response topic(s): Eric Irwin profile evaluation; Eric Irwin profile evaluation (visual verification); Why hide/delete; 3 more
- Prior summary/context: The bookmarklet is already in the static section of this document (## LinkedIn Conversation Export Bookmarklet, lines...

### Re: Is "20+ years full-stack" too generic for the headline? — opus-4-8

> "20+ years full-stack | Architected real-time multimodal AI avatar pipeline at Presence AI | Building agent tooling & context engineering for collaborative intelligence | Reactive & event-driven systems" is my short bio. Is 20+ years full-stack strong or too generic. Should I lead with something that can distinguish me more?

**Short answer: "20+ years full-stack" is a credibility anchor, not a differentiator. Don't lead with it — lead with the one thing almost no one else can claim.**

Why it's weak as the *opener*:
- It's the single most common phrase in senior-dev headlines — it signals seniority but says nothing about *what kind* of engineer you are. Thousands of profiles open this way.
- The first ~40 chars are all that show in feed comments, search results, and mobile previews. Spending that prime real estate on a generic phrase wastes your strongest impression.
- "20+ years" can quietly trigger age-bias filtering when it's the lead. It plays much better as a backing detail ("…, backed by 20+ yrs full-stack") than as the headline's first impression.

What's actually distinctive in your current bio — and underused because it's buried in the middle:
1. **"Architected real-time multimodal AI avatar pipeline at Presence AI"** — concrete, rare, and instantly credible. This is your differentiator.
2. **"agent tooling & context engineering"** — forward-looking and exactly the keyword space hiring right now.

So the question of *which* to lead with depends on what you're optimizing for:

**Option A — lead with the rare achievement (best for senior/staff AI-infra or eng-leadership roles):**
> Architected real-time multimodal AI avatar pipeline @ Presence AI | Building agent tooling & context engineering in Rust | 20+ yrs full-stack

**Option B — lead with what you're building *now* (best if you want your next role to be agent/AI tooling, not "more of the same"):**
> Building agent tooling & context engineering for collaborative intelligence | Architected real-time AI avatar pipeline @ Presence AI | 20+ yrs full-stack (Rust)

**Option C — keyword-dense for recruiter search (front-loads searchable terms):**
> Agent tooling & context engineering (Rust) | Real-time multimodal AI systems | Ex-Presence AI | Full-stack, 20+ yrs

My recommendation: **Option B.** Recruiters and founders skim the first phrase and decide whether to keep reading; "Building agent tooling & context engineering" tells them *where you're going*, which is what you want to attract, while "Architected … Presence AI" proves you've already shipped at that level. The "20+ yrs full-stack" still earns its place — just as the closer, where it reads as depth rather than as "generic senior dev."

Two smaller notes:
- Drop or fold in **"Reactive & event-driven systems"** — at the tail it reads as keyword-stuffing. If those terms matter for your target roles, work them into the Presence AI clause ("real-time, event-driven AI avatar pipeline") so they're load-bearing rather than tacked on.
- Whatever you pick, get the differentiator into the **first 40 characters** so it survives truncation in feed/comment/search contexts.
<!-- agent:boundary:34ad6aea -->
<!-- /agent:exchange -->

<!-- agent:queue -->
- "20+ years full-stack | Architected real-time multimodal AI avatar pipeline at Presence AI | Building agent tooling & context engineering for collaborative intelligence | Reactive & event-driven systems" is my short bio. Is 20+ years full-stack strong or too generic. Should I lead with something that can distinguish me more?
<!-- /agent:queue -->
