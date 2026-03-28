---
agent_doc_session: 7f435abe-d381-4f1a-bbe9-a2184a50b311
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Session cleared.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Create a contact for melissa-lewis
https://www.linkedin.com/in/melissa-brodack/

Also create a contact for brooke-brodack
https://www.linkedin.com/in/brooke-brodack-2223427/

Melissa and Brooke are sisters.

### Contacts Created

**Melissa Lewis** — `mail/contacts/melissa-lewis/AGENTS.md`
- Maiden name: Brodack
- LinkedIn: https://www.linkedin.com/in/melissa-brodack/
- Sister of Brooke Brodack

**Brooke Brodack** — `mail/contacts/brooke-brodack/AGENTS.md`
- LinkedIn: https://www.linkedin.com/in/brooke-brodack-2223427/
- Sister of Melissa Lewis
- Partner of Brian Takita

Both have `CLAUDE.md -> AGENTS.md` symlinks. LinkedIn profiles couldn't be fetched (auth wall) -- add professional details manually or via browser.

Use chromium-bridge to fetch profiles.

### LinkedIn Profiles Fetched via chromium-bridge

Updated both contact files with LinkedIn data:

**Melissa Lewis** (`mail/contacts/melissa-lewis/AGENTS.md`):
- CEO & Founder of Wedding Insiders Network
- Education: Assumption College
- Location: Greater Hartford, CT
- 146 connections

**Brooke Brodack** (`mail/contacts/brooke-brodack/AGENTS.md`):
- YouTubing since 2004
- Company: Cambly Inc.
- Education: Mount Wachusett Community College
- Location: North Grosvenor Dale, CT
- Website: www.brookebrodack.net
- Open to work: ESL Teacher, Video Streamer, Artist roles
- 451 connections

Please read /home/brian/work/btakita/agent-loop/mail/contacts/melissa-lewis/melissa-lewis-sms.txt

Draft a reply. I want to mention the ongoing status with judith-krug's Equity Funding Source project.
We have been in conversation for 1 month. Mention how I'm scheduled to build a replacement for Mortgage Automator but they are attempting to enforce a contract. Use the most recent phone call between Robert Ward, Judith Krug, and me for information.

Please retranscribe /home/brian/work/btakita/agent-loop/mail/contacts/robert-ward/recordings/phone_20260328-000319_3109684542.amr using corky --gpu.
 

### SMS Context & Draft Reply

**Last message from Melissa** (most recent): She's checking in about rent, acknowledging money is tight between jobs, asking Brian to keep communication open.

**Key facts from the Robert Ward / Judith Krug calls (Mar 27-28):**
- Judith Krug runs **Equity Funding Source** — a commercial lending operation in LA
- She signed with **Mortgage Automator** but is trying to get out of the contract
- She got her 10% deposit reversed by her bank
- She needs her attorney to confirm she's released before committing to Brian building a custom replacement
- Robert Ward (mutual friend) is advocating "build it, don't buy it" and pushing Judith to move forward
- Judith is very busy closing loans and has been a poor communicator — it's been ~1 month of waiting
- Additional work possible: websites for Judith's health coaching, spiritual coaching, and products businesses

### Draft Reply to Melissa

> Hi Melissa, thank you for being understanding and keeping communication open. I really appreciate it.
>
> Things are progressing on the work front. I've been in conversation for about a month now with a commercial real estate broker in LA — Judith Krug, who runs Equity Funding Source. Robert Ward connected us. I'm scheduled to build a custom replacement for her loan management software (Mortgage Automator), but there's been a delay because they're trying to enforce her existing contract. She's working with her attorney to get released. Once that's resolved, the project kicks off — and there's additional website work for her other businesses too.
>
> I'm also talking to other companies next week, so I'm working multiple angles. I'll keep you posted as things develop. Thank you for your patience.

### Retranscription

Retranscribing `phone_20260328-000319_3109684542.amr` with GPU + diarization (speakers: Robert Ward, Brian Takita, Judith Krug). Running in background — will update when complete.

### Retranscription Complete

Retranscribed `phone_20260328-000319_3109684542.amr` — 117x realtime on RTX 5090. Unfortunately the same Whisper hallucination loop persists (repetitive text mid-call). This is a known issue with low-bitrate AMR phone audio. The `--diarize` flag failed — corky wasn't compiled with the `diarize` feature.

Corky should **always** be compiled with the diarize feature!!!!!
Diarize should not even be a separate feature. It should be core to corky.

**Additional detail extracted from cleaner portions:**
- Mortgage Automator's doc mapping was quoted at ~$3K but the actual invoice came in at $6K+, pushing total from $18K to ~$21K
- Judith never signed the separate doc mapping invoice
- The price bait-and-switch strengthens her case for contract release
- Leo (Judith's ex) showed up wanting money for some website work — Judith wants to hire Brian instead
- Judith mentioned a "Taiyo" for her nutritional business website (not ready yet)

### Diarized Retranscription Complete

Rebuilt corky with `diarize` feature and retranscribed successfully. **3 speakers detected, 198 segments.** The transcript now has per-speaker attribution with confidence scores.

The Whisper hallucination loop (01:06–03:05) persists — this is a Whisper model issue with low-bitrate AMR audio, not a corky issue. The meaningful portions before and after the loop are now properly speaker-labeled.

Agreed — diarize should be core to corky, not an optional feature. Added to corky backlog.

**Cleaned-up call summary from diarized transcript:**

| Speaker | Key Points |
|---------|------------|
| **Judith Krug** | Got 10% deposit reversed by bank. Wants Brian to build custom software but stuck until attorney confirms she's released from Mortgage Automator contract. Doc mapping invoice jumped from $3K to $6K+ (never signed it). Business booming — "closed a shit ton of loans." Has financial website but needs websites for coaching/nutritional businesses. |
| **Robert Ward** | Advocated "build it, don't buy it." Pushed for Brian to do website work. Mentioned biohacking networking event this weekend. |
| **Brian Takita** | Noted some work isn't dependent on the Mortgage Automator decision. Thanked Judith for catching up. |

release corky changes
response patch should go into /home/brian/work/btakita/agent-loop/tasks/software/corky.md 

### Corky v0.22.0 Released

- **Version**: 0.21.2 → 0.22.0
- **New features**: Search backends (sift local + ragie cloud), unified search CLI
- **Fix**: Diarize feature now always compiled in release builds
- **Updated** CLAUDE.local.md to reflect diarize is compiled
- **Published**: Pushed to GitHub with tag, release binary built, crates.io publish in progress
- **Parent submodule** pointer updated

Noted — moved the corky v0.22.0 release details to `tasks/software/corky.md`. Future corky release patches will go there.

Please draft a response to Melissa about the status of Judith's project. Melissa is also a loan officer so we can go more indepth on the Mortage Automator + The Mortage Office status...along with how the agent harness automation that I will be building will fit in.

Also mention some of my other conversations I will be having this week, including with bob-remeika, ECI, my LinkedIn agent harness posts, and other conversations.

### Updated Draft Reply to Melissa (v2 — more detail)

Since Melissa is a loan officer, this version goes deeper on the lending software landscape and the automation angle.

> Hi Melissa, thank you for being understanding and keeping communication open. I really appreciate it.
>
> I wanted to give you a fuller picture of where things stand on the work front.
>
> **Judith Krug / Equity Funding Source:** I've been in conversation for about a month with Judith Krug — she's a commercial mortgage broker in LA running Equity Funding Source. Robert Ward connected us. She's been evaluating loan management platforms — specifically Mortgage Automator (~$21K first year with doc mapping fees that ballooned from $3K to $6K) vs The Mortgage Office (~$600/mo, lighter commitment). She signed with Mortgage Automator but is trying to get released from the contract — she never signed their inflated doc mapping invoice, and her bank reversed their 10% deposit.
>
> Once she's clear of that contract, I'm building her a custom loan management system. The key differentiator is that I'm using AI agent automation — the same technology I've been developing over the past few months. It dramatically reduces development cost and time compared to traditional software development. For Judith, that means a custom system tailored to her workflow (investor outreach, deal sourcing, pooling/pitches, analytics) at a fraction of what these off-the-shelf platforms charge. Since you're a loan officer yourself, you can probably appreciate how clunky those generic platforms can be when they don't fit your actual process.
>
> **Other conversations this week:**
> - I'm reconnecting with Bob Remeika, CEO of Ragie (a RAG-as-a-Service startup backed by Craft Ventures). We overlapped at Grockit years ago. He's interested in my interactive document and agent harness work — exploring a collaboration.
> - I have conversations with Eric Yang (former co-founder at Presence, my last company) about a creator platform direction.
> - My LinkedIn posts about AI agent harnesses have been getting good engagement from engineering leaders — generating inbound interest.
> - I'm also in the pipeline with a couple of recruiters for senior engineering roles ($150K-$180K range).
>
> So multiple angles are moving forward. I'll keep you posted as things develop. Thank you for your patience — I know this transition period hasn't been easy, and I appreciate you working with me.
<!-- agent:boundary:d48bd6bd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Review draft v2 SMS reply to Melissa and send when ready
<!-- /agent:pending -->
