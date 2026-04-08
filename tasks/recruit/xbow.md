---
agent_doc_session: b3206de4-6170-4b8f-8f2a-246f8a45bc5c
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Interview prep for XBOW (autonomous offensive security AI, M Series C unicorn, Jan 2024). Stack: TypeScript/Kotlin/Go/AWS, multi-agent LLM swarm. Core differentiator: 'proof not probability' (deterministic validator confirms exploits). Key domains: multi-agent orchestration, LLM tooling, CTF benchmark-driven eval, offensive security depth. Questions drafted for AI Systems/Security/SRE/Product roles. Open: transcription of 2026-04-07 09-00-51.mp4 requested.
Please compact, transcribe, and diarize [2026-04-07 09-00-51.mp4](../../../../../Videos/2026-04-07%2009-00-51.mp4)

### Re: Transcript — XBOW Recruiter Screen 2026-04-07

Compacted. Transcript saved to [tasks/recruit/transcripts/xbow-2026-04-07.md](transcripts/xbow-2026-04-07.md).

#### Summary

**32:20 call** with Anna (XBOW recruiter, 1 month tenure, based in Sweden/Brazil, previously at GitHub). One other XBOW voice present at the start.

**Key info from Anna:**
- Founder = Oege de Moor ("Uge de Mor") — sold Semmle to GitHub → CodeQL/GitHub Advanced Security → led Copilot blue-sky at GitHub with a team of ~8
- Team all left GitHub end of 2023/early 2024; launched XBOW April 2024
- Year 1: building the product/benchmark/research
- Spring 2025: started bug bounty work → hit #1 on HackerOne (US first, then global)
- Series B last summer, Series C two weeks ago (~$1B+ valuation)
- "Product is quite robust, still a lot to be built" — most teams currently recruiting
- Confirmed Gemini was running on the call (AI note-taker)
- This was a non-technical screen; technical discussion would follow if it goes well

**What Brian covered:**
- 20+ years software dev, started in Bay Area
- Pivotal Labs — agile consulting, ~15 startups over 4 years (Ruby on Rails, early cloud, full stack)
- honk.com (automotive social/car buying) → acquired by TrueCar
- Independent work: financial sector (Censible/Martin Investment — ESG screener, portfolio rebalancer), menu.com
- Presence AI (last role) — real-time video avatars, LiveKit/WebRTC, lip-sync pipeline, speech-to-speech models, optimized for real-time + jitter reduction; team of 6 → 3; shut down after consumer viability test failed
- Since Presence: building open-source agent harness tooling — Corky (correspondence toolkit), agent-doc (markdown-based LLM sessions synced to tmux), ontology management tools, eval frameworks
- Focus: agentic software development practices, Claude Code harness, human-agent alignment, token efficiency

**Anna's question at end:** "What would you like to do more/less of?" — Brian: improve agentic dev practices, working with Claude Code/Codex, the fundamental shift in knowledge work

**Diarization note:** 6 speaker clusters detected; some are likely voice splits. Anna = Speaker 3, Brian = Speaker 6 (long segment 05:34–32:20). Two unidentified XBOW voices (Speaker 1 at greeting, Speaker 2 in middle sections — possibly the same person or Anna voice splits).

There was a large block of transcription that was not properly split up with diarization. Please have Corky understand that this diarization was not successful...and it should process the audio using progressively smaller chunks until successful diarization is achieved.
> **Brian** (0.88) [00:05:34.720 → 00:32:20.820]

Please look at the recently synced email correcspondence to find Ana's name.
There was a large block of transcription that was not properly split up with diarization. Please have Corky understand that this diarization was not successful...and it should process the audio using progressively smaller chunks until successful diarization is achieved.
> **Brian** (0.88) [00:05:34.720 → 00:32:20.820]

### Re: Diarization fix + re-transcription in progress

**Root cause:** `quality_ok()` passed because speaker count and unknown% looked fine — but it didn't detect that one speaker's block spanned 26 of 32 minutes (81% of audio). The adaptive chunking cascade never triggered.

**Fix applied to corky (`src/corky/src/transcribe/`):**
- `diarize.rs`: Added `max_speaker_span_ratio()` — groups consecutive same-speaker segments, returns the largest block as a fraction of total covered duration
- `engine.rs`: Added span check in `run_adaptive_diarization()` — if any speaker block exceeds **60%** of total audio, cascade to smaller chunks (600s → 120s → 30s)

Now re-transcribing with `--speakers "Brian,Anna"` so speaker names are labeled directly. Running in background — will update transcript when done.
Please look at the recently synced email correcspondence to find Ana's name.

### Re: Anna Pareno / transcript updated

**Anna's full name: Anna Pareno** — confirmed from email subject lines (`new-message-from-anna-pareno-...`).

**Transcript updated:** [tasks/recruit/transcripts/xbow-2026-04-07.md](transcripts/xbow-2026-04-07.md)

Re-transcription completed. Early section (0:00–7:03) now has good alternating speaker attribution. The large block (7:03–32:20) persists but is now labeled with a `⚠️` correction note — the diarizer labeled it "Anna" but the content is clearly Brian's monologue.

**Root cause of remaining issue:** The span check targets undivided pyannote segments, but pyannote's 61 segments alternate correctly. The problem is in the Whisper→speaker mapping layer: when Brian talks for 25 consecutive minutes, the embedding similarity comparison flips for that region. This is a harder fix — requires checking span in the *merged* output layer, not the raw diarized segments.

Is the feature or bug fix added to tasks/software/corky.md?
### Re: Diarization fix — tracking added

Not yet. The fix was in the working tree (`src/corky/src/transcribe/diarize.rs` + `engine.rs`) but hadn't been logged. Added it now to `tasks/software/corky.md` under **Open items** — labeled as uncommitted/unreleased so it doesn't get lost.
<!-- agent:boundary:bf8ae1c3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
