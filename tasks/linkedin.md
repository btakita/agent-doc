---
agent_doc_session: a953811a-bf72-4aa9-bbd6-5904a5a07a1d
tmux_session: '0'
agent_doc_mode: stream
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

<!-- agent:exchange -->
Compacted from 730-line append-mode conversation into template-mode reference document. All decisions, implementation details, and action items preserved. Ready for new prompts in the input component.

Please create a bookmarklet to export the selected conversation thread into a transcript. Here is the page html:

### LinkedIn Conversation Export Bookmarklet

Here's a bookmarklet that extracts the currently visible LinkedIn messaging thread:

```javascript
javascript:void(function(){const msgs=[];let currentDate='';document.querySelectorAll('.msg-s-message-list-content .msg-s-message-list__event').forEach(li=>{const dateEl=li.querySelector('.msg-s-message-list__time-heading');if(dateEl)currentDate=dateEl.textContent.trim();const a11y=li.querySelector('.msg-s-event-listitem--group-a11y-heading');const nameEl=li.querySelector('.msg-s-message-group__name');const timeEl=li.querySelector('.msg-s-message-group__timestamp');const bodyEl=li.querySelector('.msg-s-event-listitem__body');if(!bodyEl)return;const name=nameEl?nameEl.textContent.trim():(a11y?a11y.textContent.trim().split(' sent ')[0]:'Unknown');const time=timeEl?timeEl.textContent.trim():'';const text=bodyEl.innerText.trim();if(text)msgs.push((currentDate?'['+currentDate+'] ':'')+'['+time+'] '+name+':\n'+text)});const title=document.querySelector('.msg-overlay-conversation-bubble__title,.msg-s-message-list-container')?.closest('[class*=conversation]')?.querySelector('h2,a[class*=name]')?.textContent?.trim()||'LinkedIn Conversation';const content='# '+title+'\nExported: '+new Date().toISOString()+'\n\n'+msgs.join('\n\n');const blob=new Blob([content],{type:'text/plain'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download=title.replace(/[^a-zA-Z0-9]/g,'-').toLowerCase()+'.txt';a.click()})()
```

**How it works:**
1. Iterates over all `msg-s-message-list__event` items in the conversation
2. Extracts date headers (`msg-s-message-list__time-heading`)
3. Gets sender name from `msg-s-message-group__name` (falls back to a11y heading)
4. Gets timestamp from `msg-s-message-group__timestamp`
5. Gets message text from `msg-s-event-listitem__body` via `innerText` (strips HTML)
6. Downloads as a `.txt` file

**To install:** Create a new bookmark, paste the `javascript:void(...)` code as the URL.

**Note:** LinkedIn lazy-loads messages — only the currently visible messages in the thread will be exported. Scroll up to load older messages before clicking the bookmarklet.

**Also note:** The HTML you pasted is 1.7MB — that's bloating this task file. Consider removing the raw HTML and keeping just the bookmarklet.

Please get the name of the person in the conversation and use the name as part of the export file name.

### Updated: name in filename

Updated bookmarklet — now extracts the conversation partner's name from the thread header and uses it in the download filename (e.g., `linkedin-mia-tomeo.txt`):

```javascript
javascript:void(function(){const msgs=[];let currentDate='';document.querySelectorAll('.msg-s-message-list-content .msg-s-message-list__event').forEach(li=>{const dateEl=li.querySelector('.msg-s-message-list__time-heading');if(dateEl)currentDate=dateEl.textContent.trim();const nameEl=li.querySelector('.msg-s-message-group__name');const timeEl=li.querySelector('.msg-s-message-group__timestamp');const bodyEl=li.querySelector('.msg-s-event-listitem__body');if(!bodyEl)return;const name=nameEl?nameEl.textContent.trim():'Unknown';const time=timeEl?timeEl.textContent.trim():'';const text=bodyEl.innerText.trim();if(text)msgs.push((currentDate?'['+currentDate+'] ':'')+'['+time+'] '+name+':\n'+text)});const headerEl=document.querySelector('.msg-overlay-bubble-header__title a,.msg-thread__link-to-profile,.msg-entity-lockup__entity-title');const contactName=headerEl?headerEl.textContent.trim():'Unknown';const slug=contactName.replace(/[^a-zA-Z0-9]+/g,'-').toLowerCase().replace(/^-|-$/g,'');const content='# LinkedIn Conversation with '+contactName+'\nExported: '+new Date().toISOString()+'\n\n'+msgs.join('\n\n');const blob=new Blob([content],{type:'text/plain'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download='linkedin-'+slug+'.txt';a.click()})()
```

**Changes:**
- Extracts contact name from the conversation header (`.msg-overlay-bubble-header__title a`, `.msg-thread__link-to-profile`, or `.msg-entity-lockup__entity-title`)
- Slugifies the name for the filename: "Mia Tomeo" → `linkedin-mia-tomeo.txt`
- Includes the contact name in the transcript header: `# LinkedIn Conversation with Mia Tomeo`
Please remove the html bloat from the git history. I will have an external file next time.

The export name was `linkedin-mia-tomeo-status-is-reachable-mobile-16m-ago.txt`
It should be `linkedin-mia-tomeo.txt`
Actually, it should be `yyyymmdd-linkedin-mia-tomeo.txt`

Please update the bookmarklet to format the export file name as `yyyymmdd-linkedin-mia-tomeo.txt`

Convert this doc to agent_doc_mode: stream
<!-- /agent:exchange -->
