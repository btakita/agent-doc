---
agent_doc_session: 38d73b6a-87c7-442f-ae47-f12045659a34
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Centerbeam Nova research complete — ready to draft reply to Drew
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Create a contact for drew-anderson
https://www.linkedin.com/in/drewanderson/

### Re: Create contact for drew-anderson

Created contact at `mail/contacts/drew-anderson/`:
- **AGENTS.md** — full contact file (canonical)
- **CLAUDE.md** — symlink to AGENTS.md

Profile from LinkedIn:
- **Name:** Drew Anderson
- **Company:** Carat Venture Partners (Silicon Valley)
- **Location:** Los Gatos, CA
- **Focus:** AI architecture, conversational AI systems, enterprise voice infrastructure
- **Patents:** 5 (ad selection, personalization, content reconfiguration)
- **Certs:** AWS Business Professional, HIPAA, CERT Emergency Technician
- **LinkedIn:** https://www.linkedin.com/in/drewanderson/

The contact is minimal since LinkedIn data was partially redacted. You can flesh out the relationship and tone sections once you've had communication.

### Re: Create contact

Created `mail/contacts/drew-anderson/` with:
- **AGENTS.md** — professional profile from LinkedIn
- **CLAUDE.md** → symlink to AGENTS.md

**Profile summary:**
- **Drew Anderson** — AI Architect & Tech Strategist at Carat Venture Partners
- **Location:** Los Gatos, CA (Silicon Valley)
- **Focus:** AI, OTT video, cloud tech, nearshoring
- **Patents:** 4 (2010) — ad tech & content personalization
- **Certs:** AWS (Business Pro, Cloud Practitioner, Security), HIPAA
- **Volunteer:** CERT / Santa Clara County Fire Dept (2012–present)

What's the context for connecting with Drew? I can help draft outreach or add relationship notes.

Drew reached out to me on LinkedIn. See contacts/drew-anderson/*.txt for his message 

### Re: Drew's LinkedIn outreach

Read his message in `20260325-linkedin-drew-anderson.txt`. Updated the contact file with:

- **Relationship:** Fellow freelance AI developer, reached out 2026-03-25 re: voice agents
- **Tone:** Casual-professional, conversational, nostalgic about programming history
- **Topics:** Full-duplex voice agents, avatars, Centerbeam Nova SDK/API, "5th wave coding"
- **Message log:** Indexed his initial outreach

**Drew's message summary:** He's interested in full-duplex voice agents and adding avatars. Asks for your thoughts on Centerbeam Nova SDK/API (https://centerbeam.ai/#nova-bot). Sees a kinship — fellow freelancer using AI to build/rebuild apps, coding since 1975. Prefers "5th wave coding" over "vibe coding."

Would you like me to:
1. **Research Centerbeam Nova** so you can give an informed reply
2. **Draft a response** to Drew on LinkedIn
3. **Both** — research first, then draft

Let's research first and discuss.

### Re: Research Centerbeam Nova

**Centerbeam AI / Nova SDK — Research Summary**

**What it is:** A real-time voice SDK for embedding conversational AI in web apps. Tagline: "Enterprise media agents. Built once. Runs anywhere."

**Technical details:**
- JS/TS SDK (`@centerbeam/nova`) — ~4 lines to embed a voice agent
- Full-duplex voice pipeline (device-neutral API relay gateway)
- Uses **OpenAI Realtime** as the underlying LLM
- Event-driven tool/workflow integration
- Low-latency browser voice experience

**Code example from their site:**
```javascript
import { Nova } from "@centerbeam/nova";
const nova = new Nova({ apiKey: process.env.NOVA_API_KEY });
await nova.mount("#agent");
await nova.start();
```

**Enterprise features:** SOC 2 Type II, SSO/SAML, RBAC, audit logs, private networking, multi-deployment (cloud/hybrid/private/on-prem)

**Use cases:** Customer support, onboarding, intake forms, product guidance, internal copilots

**What I couldn't find:**
- No public reviews, GitHub repos, or third-party coverage — very low profile online
- No confirmed connection between Drew Anderson / Carat Venture Partners and Centerbeam AI (though he's clearly promoting it)
- No pricing information publicly available
- The SDK package `@centerbeam/nova` doesn't appear in npm search results

**Assessment:**
This looks like an early-stage voice AI platform wrapping OpenAI Realtime with enterprise deployment features. The polished website but zero external footprint (no reviews, no npm package visibility, no GitHub) suggests it's either very new or pre-launch. Drew may be involved with Centerbeam directly (investor via Carat VP, or advisor/founder).

**Comparison context:** Competing with LiveKit Agents (open source), OpenAI's native voice agents SDK, Amazon Nova Sonic, and Deepgram — all of which have much larger developer ecosystems.

**Discussion points for your reply to Drew:**
1. Do you want to explore a potential collaboration, or just exchange thoughts?
2. His mention of "adding avatars" suggests they're building toward multimodal (voice + visual) agents
3. The OpenAI Realtime dependency is notable — worth asking if they plan to support other models (Claude, open-source)

What direction do you want to take the reply?
<!-- agent:boundary:613b3bd6 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Draft reply to Drew's LinkedIn message
<!-- /agent:pending -->
