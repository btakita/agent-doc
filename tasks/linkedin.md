---
agent_doc_session: a953811a-bf72-4aa9-bbd6-5904a5a07a1d
tmux_session: '0'
agent_doc_mode: template
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
Compacted from 730-line append-mode conversation into template-mode reference document. Bookmarklet moved to its own top-level section. All merge artifacts and duplicate content cleaned up.

See the output in /home/brian/work/btakita/agent-loop/mail/contacts/mia-tomeo/20260311-linkedin-mia-tomeo.txt

Modify bookmarklet
Remove `Status is ...`
All I care about is the correspondence text and timestamp

### Re: Remove status text from bookmarklet

Looked at the exported file — the header element contains nested HTML with "Status is reachable" and "Mobile • 28m ago" inside `<span>` children. The `childNodes[0].textContent` approach was still capturing that structure.

Fixed by extracting only direct text nodes from the header element (skipping all child elements), and cleaning whitespace. Also simplified the transcript output — just `[timestamp] Name: message`, no date headers cluttering the format.

The bookmarklet wanted to save the file as `20260311-linkedin-.txt`...it should be `20260311-linkedin-mia-tomeo.txt`

### Re: Empty name in filename

The `childNodes` text node filter returned nothing because LinkedIn wraps the name in a `<span>`, not as a bare text node. The fallback `split(/\s{2,}/)` also failed on the whitespace-heavy `textContent`.

**Fix:** Split `textContent` on newlines, trim each, take the first non-empty line. The LinkedIn header structure puts the name on the first line, with "Status is reachable" and "Mobile • 28m ago" on subsequent lines. This is the simplest reliable approach.

Try the updated bookmarklet — it should now save as `20260311-linkedin-mia-tomeo.txt`.

Help me write a post sharing
https://www.linkedin.com/events/7433272878466359296/

What I have so far:
> I caught the tail end of this call. But Eric Ries is making a bold stance on applying ethics in the agentic age.

I want to emphasize how we can define and apply philosophy to find deep alignment with each other and with our tools.

### Re: Eric Ries / Incorruptible post

Eric Ries's new book *Incorruptible* argues organizational failure is structural, not ethical — systems quietly reshape behavior. Notably uses Anthropic as a case study. The LinkedIn event was likely a live discussion around these themes.

**Post draft** (ready to paste):

---

I caught the tail end of Eric Ries's call on building incorruptible organizations. He's making a bold stance: the failure of companies isn't ethical — it's structural. The systems we build quietly reshape behavior until even principled people produce outcomes they never wanted.

This resonates deeply with what I'm building.

If organizational drift is structural, then alignment — between people, between teams, between humans and AI agents — is a design problem. Not a values problem. Not a policy problem. A systems design problem.

This is why I'm obsessed with defining terms precisely. In Existence Lang, every concept starts from the universal set (Existence) and scopes down through explicit context. When your ontology is sloppy, your systems drift. When your terms are precise and composable, alignment becomes structural too.

The same principle applies to AI agents. An agent without well-defined scope, context, and domain boundaries will drift just like an organization without mission-locked governance. agent-doc is my attempt to make that structure explicit — every interaction captured in a document, every decision traceable, every scope bounded.

Ries uses Anthropic as a case study in Incorruptible. It's no accident that the company building Claude is also experimenting with novel corporate structures to stay aligned with its mission. Alignment isn't a feature you bolt on — it's architecture.

What systems are you building to keep your work aligned with your intent?

https://www.linkedin.com/events/7433272878466359296/

#Incorruptible #EricRies #ExistenceLang #AgentDoc #AI #Alignment #OrganizationalDesign #Philosophy

---

Connects your Existence Lang philosophy (explicit scoping, precise ontology) to Ries's thesis (structural alignment > ethical intentions). The Anthropic case study reference ties it together — same company building Claude. Adjust tone or length as needed.

post to linkedin

### Posted to LinkedIn

Published via corky: https://www.linkedin.com/feed/update/urn:li:share:7437915217499471872

Draft file: `mail/social/20260312-eric-ries-incorruptible-linkedin.md` (status updated to `published`).


From David Alger
> Really cool stuff. I wonder if organizations and companies should have time limits. After that, they have to start over.Otherwise they keep getting stretched and changed for purposes they weren't designed to accomplish.

Help me draft a reply. What I have so far:

> Thank you David! People and institutions may change. It would be interesting to capture the moment and context of when the company during certain moments...such as when it was on a good path. Perhaps other people or companies can then take on the essence of that company in a good state and carve out a different path onward? Kindof like exploring alternative paths via improv?
> 
> I wonder if character sheets can be made so an actor c

### Re: Reply to David Alger

Your improv/forking metaphor is strong. Here's a polished version building on your draft:

---

Thank you David! People and institutions do change — and that drift is often invisible until it's too late.

Your "time limit" idea is fascinating. What if instead of hard resets, we could snapshot the essence of an organization at its best — capture the principles, the culture, the decision patterns — and let others fork from that state? Like version control for organizational DNA.

It's similar to improv: you take the best "yes, and..." moments and branch into new scenes. The original company may drift, but the patterns that worked become seeds for something new.

That's actually what draws me to spec-driven tooling — making the intent explicit and traceable so it doesn't quietly erode over time. If you can diff your current state against your original mission, drift becomes visible.

---

Adjust tone/length as you like. The version-control metaphor ties naturally to your tech background and the Existence Lang work without being too self-promotional for a comment reply.


From David Alger
> Really cool stuff. I wonder if organizations and companies should have time limits. After that, they have to start over.Otherwise they keep getting stretched and changed for purposes they weren't designed to accomplish.

David was my improv teacher around 2006-2009. His theater + shows is in Oakland CA + San Francisco CA. So he works with many in the tech world. I want to craft my reply to be exploratory with characters and stories.

Thank you David! People and institutions may change. It would be interesting to capture the moment and context of when the company during certain moments...such as when it was on a good path. Perhaps other people or companies can then take on the essence of that company in a good state and carve out a different path onward? Kindof like exploring alternative paths via improv?
 
I wonder if character sheets can be made so an actor can "align" with the subject at various phases in story.


---

Thank you David! People and institutions do change — and that drift is often invisible until it's too late.

Your "time limit" idea is fascinating. What if instead of hard resets, we could snapshot the essence of an organization at its best — capture the principles, the culture, the decision patterns — and let others fork from that state? Like version control for organizational DNA.

It's similar to improv: you take the best "yes, and..." moments and branch into new scenes. The original company may drift, but the patterns that worked become seeds for something new.

That's actually what draws me to spec-driven tooling — making the intent explicit and traceable so it doesn't quietly erode over time. If you can diff your current state against your original mission, drift becomes visible.

I wonder if character sheets can be made so an actor can "align" with their subject at various phases in story.

---


<!-- /agent:exchange -->
