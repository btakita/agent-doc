---
agent_doc_session: 52f61771-848a-448e-964c-2e27d6e31aa5
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

❯ This will be for a technical interview with:
❯ - https://www.linkedin.com/in/chris-smith-252697341/
❯ - https://www.linkedin.com/in/stuart-hiller-6a35a1a6/
❯ ### Re: technical interview with Chris Smith + Stuart Hiller — haiku (HEAD)

Noted — technical round with two interviewers. I can't fetch LinkedIn directly, so I don't know their roles/backgrounds from the URLs alone.

To prep effectively, a few things would help:

- **Their roles** — engineering managers? staff/principal ICs? architects? Tells me whether to expect systems design, coding, or leadership-angle questions.
- **Format** — live coding, systems design, architecture discussion, behavioral, or mixed? How long?
- **Stack focus** — Filevine is .NET/C# + React on the public side; is this role backend, full-stack, infra, or AI/ML?
- **Scheduled date** — so I can sequence prep tasks.

If you paste the LinkedIn bios (or a summary) into the doc, I can tailor talking points, likely questions, and a short technical review checklist against your strengths (Rust/TS/distributed systems/AI tooling).

❯ Please add as a submodule git@github.com:mixmasterc85/FDE-Project-Interview.git
This is in a docker container.
### Re: FDE-Project-Interview submodule — haiku

Added at `tasks/recruit/filevine/FDE-Project-Interview` and read the README. Summary of what they gave you:

**Stack:** FastAPI + Temporal (worker + workflows boilerplate in `app/`) + LocalStack S3 + Postgres, all via `docker compose`. Python, not .NET — so this is an FDE (Forward Deployed Engineer) migration-engineering role, not backend product.

**The task:**

1. **Source:** three `.xlsx` files in LocalStack S3 bucket `migration-data` — Contacts, Address History, Projects.
2. **Target:** Filevine REST API — `Create Project` (v2) and `Create Contact` legacy (addresses are nested inside contact payloads, not a separate endpoint).
3. **Mapping:** xlsx columns ≠ Filevine field names. You need to inspect columns and define the mapping.
4. **No real Filevine keys** — code must still build + send requests with env-var config, and write the payloads to JSON files for review. Handle 401/403 gracefully.
5. **`contactId` on projects** — mock/placeholder, or thread it from the contact create step.
6. **Temporal** — orchestrate the pull-transform-send flow as a workflow + activities.
7. **Gemini LLM** — required integration (key is in `.env`). Open-ended: could do column-to-field mapping assistance, project-type classification, enrichment, etc.

**What I'd prep before the interview:**

- Skim the three xlsx files locally (`docker compose up -d` then `aws --endpoint-url=http://localhost:4566 s3 cp ... --recursive`) so you know the real column shapes, not just the file names.
- Read the two Filevine endpoint docs and sketch the target payload shapes on paper.
- Draft the mapping table (xlsx col → Filevine field) for Contacts + Projects + Address History. This is the core of the exercise.
- Decide the Temporal shape: one workflow per entity type, or one orchestrator workflow fanning out to activities? I'd go one-orchestrator-per-migration-run with activities for `extract_from_s3`, `map_with_gemini`, `post_to_filevine`, `write_payload_json`.
- Pick where Gemini earns its keep — I'd use it for **ambiguous column mapping** (ask it to propose field mappings given xlsx headers + Filevine schema) and **project-type inference** from row content. Both are genuinely hard without an LLM and easy to defend in the interview.
- Have an answer for: *order of operations* (contacts first so projects can reference real contactIds, then projects, then addresses — but since contacts own addresses in Filevine, addresses merge into the contact create call).

**Interview strategy:** they want to see you reason about mapping under ambiguity, not memorized Filevine trivia. Narrate tradeoffs out loud. The Gemini call is the signal — show you can pick a high-value LLM use case, not just sprinkle it in.

Want me to draft the mapping tables and a workflow skeleton you can walk in with, or keep it hands-off so you can work through it cold in the interview?
<!-- agent:boundary:19e230f7 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
