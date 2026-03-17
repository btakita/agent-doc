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
<!-- /agent:exchange -->
