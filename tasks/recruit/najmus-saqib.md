---
agent_doc_session: 51be5055-1c22-4bb0-baf9-604c2fc913ac
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Symlinks fixed. Draft reply still ready for review.
<!-- /agent:status -->

## Exchange
<!-- agent:exchange patch=append -->
Please create a contact for najmus-saqib https://www.linkedin.com/in/najmus-saqib-937102152/

Read /home/brian/work/btakita/agent-loop/mail/contacts/najmus-saqib/20260322-linkedin-najmus-saqib.txt

Draft a reply
### Re: Contact + Draft Reply

Created `mail/contacts/najmus-saqib/CLAUDE.md` (with `AGENTS.md` symlink).

**Role summary:** Senior Full Stack Dev for Guidehouse/SBA. Remote. Python (Django/Flask/FastAPI) + Vue.js (or React/Svelte/Angular) + AWS. Strong fit — you hit every requirement and most nice-to-haves.

**Draft reply:**

Hi Najmus,

Thanks for reaching out — this role looks like a strong fit. I have 20+ years of full-stack experience with deep expertise in Python (FastAPI, Flask, SQLAlchemy), TypeScript/React, and AWS.

I'm available and interested. What's the best way to move forward — do you need my resume, or would you like to schedule a call first?

Best,
Brian

Want to tweak, or ready to send?

AGENTS.md should be the canonical file. CLAUDE.md should symlink to AGENTS.md
<!-- agent:boundary:8e800b85-e6a2-4bc7-998e-91eb2a8a1728 -->
### Re: Symlink Fix

Fixed. `AGENTS.md` is now the canonical file, `CLAUDE.md` symlinks to it — for all contacts created this session:
- `mail/contacts/najmus-saqib/`
- `mail/contacts/lucas-noble/`
- `mail/contacts/brooke-yates/`
- `mail/contacts/benjamin-becker/`
<!-- agent:boundary:e4d1cf65-8577-47cf-a873-dcc9c989e72f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Send reply to Najmus Saqib via LinkedIn
- Generate custom resume if requested
<!-- /agent:pending -->
