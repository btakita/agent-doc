---
agent_doc_session: cdb66ae1-09b7-466b-9979-54ffa571683b
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please create a contact for andreas-kovountzis https://www.linkedin.com/in/andreas-kavountzis-88a87310/

Use corky gpu to transcribe + diarize [2026-06-02 13-00-37-compressed.mp4](../../../../../Videos/2026-06-02%2013-00-37-compressed.mp4)

### Re: Create contact for andreas-kovountzis — sonnet-4-6

Created `mail/contacts/andreas-kovountzis/` (canonical `AGENTS.md` + `CLAUDE.md` symlink, per convention). Populated from the call transcript, not just the LinkedIn URL.

- **Who:** Andreas **Kavountzis** — CTO of **Redium** (loyalty + social platform). Your task file spells it `kovountzis`; his LinkedIn vanity URL and self-intro are `kavountzis`. I kept the directory name as `andreas-kovountzis` to match the task file but use **Kavountzis** in the contact body.
- **Context:** The 2026-06-02 call was a first-round CTO interview / mutual intro for an engineering role on Redium's onshore team. Captured: his background (ex-Quick Left XP, co-founded a consultancy with Jeff Dean), Redium's product/funding/stack ($1M+ ARR, TS+Mongo on AWS, Shopify-embedded, EB→ECS/OpenTofu migration), the founders (CEO Sarah, president Regan), and his 4-step interview process (intro → loyalty system-design → light coding → two culture finals).
- **Action item recorded:** decide whether to continue; if yes, email/LinkedIn Andreas to schedule the system-design round (he targeted next week).

LinkedIn: https://www.linkedin.com/in/andreas-kavountzis-88a87310/

### Re: corky gpu transcribe + diarize — sonnet-4-6

Done — transcript at `mail/contacts/andreas-kovountzis/recordings/20260602-andreas-kovountzis.txt`.

The first attempt was running on **CPU**, which is why it was slow: the installed `.bin/corky` binary had no CUDA support compiled in (built with default `["transcribe","diarize"]`, not `transcribe-cuda`). I rebuilt corky with `--features transcribe-cuda` (`CMAKE_CUDA_ARCHITECTURES=120` for your RTX 5090 / CUDA 13.3), confirmed the new binary links `libcublas`/`libcudart`/`libcuda`, and re-ran on the GPU:

- **23.8s, 149.8× realtime** (vs the CPU run still going after 25+ min)
- 895 transcription segments; diarization found 2 speakers (Brian, Andreas)
- **Diarization caveat:** speaker labels are imperfect/swapped in spots — e.g. Andreas's "I'm the CTO of Redium" self-intro is mis-tagged as Brian. Read by content, not label.

Note: the `.bin/corky` symlink now points at the CUDA-enabled release build, so future `corky transcribe` runs will use the GPU automatically. The `mail` repo changes (new contact + transcript) are left uncommitted — say the word if you want them committed there.
<!-- agent:boundary:9d17c829 -->
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
