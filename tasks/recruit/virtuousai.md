---
agent_doc_session: 9342c5f7-a651-49a3-bb81-4c87be5988ca
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->


### Re: VirtuousAI profiles + merged call transcript — opus-4-8

Created all 8 profiles plus the merged recruiter-call transcript. Profiles follow the repo's `mail/contacts/<slug>/AGENTS.md` convention (with `CLAUDE.md → AGENTS.md` symlinks).

**Company**
- [`mail/contacts/virtuousai/`](../../mail/contacts/virtuousai/AGENTS.md) — BAIO platform (DS2O™), Fair/Explainable + "white-box" ML for mid-market; Los Gatos, ~3–10 people, founded ~2019; stack Python/TS/React/LLMs + Neo4j/PyTorch-Geometric graph. Includes the **role brief** (engineer #2, 30-day contract→direct-hire, remote-US, $175k+ target) and the **"white box" angle**: agent-doc is a white-box UX *to agents* — the markdown doc is the transparent, live-editable control surface (prompts/queue/backlog/responses all visible), mirroring Rory's explainability thesis at the human-agent layer.

**People**
- [recruiter `laura-rondon-de-barros`](../../mail/contacts/laura-rondon-de-barros/AGENTS.md) — G2i technical recruiter (Spain-based); ran the 2026-06-17 screen, uploads resume + recording to client, decision by ~Fri 2026-06-20.
- [CTO `rory-donovan`](../../mail/contacts/rory-donovan/AGENTS.md) — Founder & CTO, hiring manager; ex-Unanimous A.I.; Ethical/Explainable AI, BCI, swarm intelligence, AutoML. *Noted the "white box" description on his profile and flagged the separate "Rory Donovan-Maiye" (Allen Institute) as a different person — not conflated.*
- [CEO `chris-happ`](../../mail/contacts/chris-happ/AGENTS.md) — appointed Jun 2024; serial SaaS operator (blueSolutions→Hubwoo, Goby→Conservice, MarketTime); "AI in a Box"; Austin TX.
- [CRO `jeff-mehalek`](../../mail/contacts/jeff-mehalek/AGENTS.md) — ex-Enlyft SVP Sales, BlueConic, Salesforce/Tableau AVP.
- [Director of Data Science `matthew-paff`](../../mail/contacts/matthew-paff/AGENTS.md) — molecular-biologist→data scientist; ex-Valkyrie, Insight; Los Gatos.
- [Full-Stack Engineer `leonardo-roese`](../../mail/contacts/leonardo-roese/AGENTS.md) — current eng #1; ex-Biodock founding engineer, Pluralsight, MGM, 3M; AWS SA-Associate; likely technical-screen peer.
- [GNN Engineer `alan-salimov`](../../mail/contacts/alan-salimov/AGENTS.md) — Head of Graph AI/ML; PyTorch-Geometric + Neo4j; Stanford-affiliated; co-inventor on VirtuousAI patents.

**Transcript** — [`tasks/recruit/transcripts/2026-06-17-virtuousai-g2i-recruiter-call-transcript.md`](transcripts/2026-06-17-virtuousai-g2i-recruiter-call-transcript.md): corky GPU transcribe + diarize of both clips, merged into one file (Part 1 14:49 + Part 2 2:12).

⚠️ One accuracy caveat worth knowing: the `--diarize` pass labeled the two speakers as "Brian/Rory," but this was a two-person call between **Brian (candidate)** and **Laura Rondón de Barros (G2i recruiter)** — Rory was *not* on the call, and the diarizer crossed attributions (e.g., the "Brian"-labeled opener says "I'm Laura, recruiter at G2i"). I added a header note saying to read content over labels rather than silently re-attributing every segment and risking errors. If you want, I can do a cleaner manual re-attribution pass to fix the per-segment speaker labels.

Nothing was committed/pushed beyond this session doc's own finalize — these are content artifacts, not a spec/test change, so I left the repo working tree for you to review and commit when ready.
<!-- agent:boundary:41fbb685 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
