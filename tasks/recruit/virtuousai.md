---
agent_doc_session: 9342c5f7-a651-49a3-bb81-4c87be5988ca
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
pending_done_guard: warn
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#corky-bug': Please create a plan for corky to fix this issue. Add to the backlog of agent-loop/tasks/software/corky.md
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-commit': update spec + tests. commit
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Create profiles for:
❯ - recruiter laura-rondon-de-barros https://www.linkedin.com/in/laura-rondon-de-barros-67999813011/
❯ - CTO & hiring manager rory-donovan https://www.linkedin.com/in/rory-donovan/
❯   - Note his description "Responsible for both the Virtuous AI platform for Fair and Explainable ML Tools and "white box" foundational models for businesses large and small. He is engaged in first-hand operations, development, and execution of that technology.". Commentary for this session (not his profile), is that agent-doc provides a "white box" UX to agents.
❯ - CEO chris-happ https://www.linkedin.com/in/clhapp/
❯ - CRO jeff-mehalek https://www.linkedin.com/in/jeff-mehalek/
❯ - Director of Data Science matthew-paff https://www.linkedin.com/in/matthew-paff/
❯ - Full-Stack Engineer leonardo-roese https://www.linkedin.com/in/leonardo-roese/
❯ - Graph Neural Networks Engineer alan-salimov https://www.linkedin.com/in/alan-s-8154ab71/

❯ Create a profile for virtuousai.com. Corky GPU transcribe + diarize part 1 [2026-06-17 11-01-37-compressed.mp4](../../../../../Videos/2026-06-17%2011-01-37-compressed.mp4) and part 2 [2026-06-17 11-16-36-compressed.mp4](../../../../../Videos/2026-06-17%2011-16-36-compressed.mp4) into a single transcript.

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

❯ Yes, do the cleaner manual re-attribution pass.

### Re: cleaner manual re-attribution pass — opus-4-8

Done. I rewrote [`transcripts/2026-06-17-virtuousai-g2i-recruiter-call-transcript.md`](transcripts/2026-06-17-virtuousai-g2i-recruiter-call-transcript.md) with correct per-segment speaker labels. Every line is now **Brian** (candidate) or **Laura** (G2i recruiter) — no more "Rory" (he was never on the call) and no more crossed attributions.

What I did:
- **Re-read every segment and assigned the real speaker by content**, not by the diarizer's track. The diarizer had alternated "Brian/Rory" roughly every segment, which was wrong — e.g. the opener ("I'm Laura, recruiter at G2i"), the whole VirtuousAI company pitch, and all the screening questions are Laura; the catfishing/monsterrodholders story and the long AgentDoc/TSIFT/Lazily monologue are Brian.
- **Split the crossed segments** where one diarizer block actually contained a turn change (e.g. Laura's "any questions?" → Brian's "Okay, go over them"; Brian's industry question → Laura's "I don't know"; the closing farewells). Sub-turns from a split share or omit the precise timestamp since I can't compute sub-segment timings.
- **Dropped the confidence scores** (they scored the wrong labels) and **updated the front-matter note** to say labels were manually re-attributed rather than just "read content, not labels."

One genuinely garbled spot survives: the brief "I'm laggy / you're laggy" crosstalk near the compensation question is hard crosstalk in the audio — I made a best-effort split but that 2-3 line stretch is the one place I'd treat the labels as approximate. Also fixed an obvious mistranscription ("Thank you, Ryan" → "Thank you, Laura") in the sign-off.

Still uncommitted to the repo beyond this session doc's finalize — it's a content artifact, so I left it for you to review and commit when ready.
<!-- agent:boundary:8d1267d0 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- Research https://www.virtuousai.com/ and https://www.virtuousai.com/resources. Get the transcript from https://www.youtube.com/watch?v=Tmb3CJS07Nk. I will be creating a custom resume for my services for Virtuous AI. Emphasize my monsterrodholders work. Create a message for laura-rondon-de-barros, who will forward to chris-happ, to demonstrate what value I can bring to VirtuousAI and how I am the best candidate for their company.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
