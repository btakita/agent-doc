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
❯ ### Re: FDE-Project-Interview submodule — haiku (HEAD)

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
❯ ### Re: docker container note — haiku (HEAD)

Got it — the whole stack (FastAPI, Temporal worker, LocalStack, Postgres) runs via `docker compose up -d` from inside `tasks/recruit/filevine/FDE-Project-Interview`. I won't try to run Python/uvicorn on the host.

Practical implications:

- **Host tooling for inspection:** to read the xlsx files without starting the full stack, I'd either `docker compose run` a one-off python container or just copy the files out of `data/` (they ship in the repo too, per the README — same files as in the S3 bucket). The `data/` dir is the easy path.
- **LocalStack endpoint:** `http://localstack:4566` from inside the compose network, `http://localhost:4566` from the host. Code should read from an env var so it works in both.
- **Temporal worker lives in the compose stack** — any workflow/activity code edits need a container rebuild or a bind mount (the README says FastAPI hot-reloads under `app/`; worth checking if the worker does too).
- **Gemini `.env`** — it's mounted into the containers, so the key is only reachable from inside the stack. Don't print it.

Want me to spin up the stack now and poke at the xlsx column headers + verify the Temporal boilerplate, or hold off until you give the go-ahead?

❯ Please treat the README.md as the spec for this interview. Create a plan to fulfill the objectives.
### Re: plan for FDE-Project-Interview — haiku

**Important reframe:** the README says "boilerplate in `workflows.py`/`activities.py`/`worker.py`", but the starter repo actually ships a fairly thick implementation already — there's a 471-line `filevine_mapper.py`, a wired `migration_pipeline.py`, a `filevine_client.py` with 401/403 handling, and a `gemini_service.py` with two call sites. The Temporal workflow (`MigrationWorkflow`) is end-to-end: `migration_load_from_s3` → `migration_write_json` → `migration_send_to_filevine`, and activities are registered in `worker.py`.

So this is **not a green-field build** — it's closer to "understand this, run it, identify gaps, harden it, defend the design." Different interview shape than I initially framed.

**Signals that there's work left:**

1. `app/services/mockPersenIDFetch.py` exists, is **empty**, and has a typo (`Persen` → `Person`). Almost certainly a placeholder for the "mock contactId for projects" requirement — currently satisfied inline via `mock_contact_id_prefix` in `run_pipeline`. Wiring this file is probably an expected deliverable.
2. Mapper correctness is where the interview actually tests you — 471 lines of xlsx → Filevine mapping is guaranteed to have judgment calls worth defending or flagging.
3. Gemini is only wired for **description generation** and **project-type inference from name**, both gated behind `use_gemini_for_description=False` by default. A stronger use case — *LLM-assisted column-to-field mapping* — isn't used and would be a high-signal addition.
4. No one-shot entry point beyond the Temporal UI. A `make migrate` or FastAPI endpoint is polish-level signal.

### Phased plan

**Phase 1 — Ground truth (30–60 min)**

- Start the stack: `docker compose up -d` from inside the submodule. Confirm FastAPI (8000), Temporal UI (8080), LocalStack (4566) are green.
- Read `data/*.xlsx` headers directly on host (files ship in the repo, no LocalStack needed yet).
- Skim `filevine_mapper.py` top-to-bottom with the Filevine Create Contact + Create Project docs open. Note every judgment call you disagree with — these become interview talking points.

**Phase 2 — Run it clean (15 min)**

- Trigger `MigrationWorkflow` via `temporal_router.py` or the Temporal UI with `{"write_json": true, "send_to_filevine": false}`.
- Inspect generated JSON under `output/`. Diff against what the Filevine docs say each create endpoint expects. This is your ground truth for "is the mapping correct."

**Phase 3 — Prove the 401/403 path (10 min)**

- Re-run with `send_to_filevine=true`, keys unset. Verify the workflow **completes** (not fails) and the `errors` array cleanly captures each failure.

**Phase 4 — Fill the gaps (60–90 min)**

- Implement `mockPersenIDFetch.py` (rename → `mock_person_id_fetch.py`). Contract: `get_or_create_mock_contact_id(contact_payload) -> str`, called when threading contactIds into project payloads. Removes the magic `mock_contact_id_prefix` sprinkling.
- Verify the order-of-operations invariant: contacts (with addresses merged) → mock contactIds captured → projects reference them. Refactor if the mapper doesn't already do this cleanly.

**Phase 5 — Gemini that actually earns its keep (45 min)**

- Current Gemini calls are weak (describe by name, guess type from name). Add an **LLM-assisted column mapper**: given xlsx headers + the target Filevine schema, ask Gemini to propose a mapping JSON. Cache by header signature so it runs once, not per row. This is the use case to lead the interview with — ambiguous without an LLM, genuinely hard, and shows you pick high-value LLM integrations rather than sprinkling AI for show.

**Phase 6 — Polish + talking points (30 min)**

- Add `make migrate` or a `/migrate` FastAPI endpoint that kicks off `MigrationWorkflow` with sane defaults.
- Prep notes: *why Temporal vs a script* (retries, idempotency, observability, activity boundaries), *why Gemini here* (mapping under ambiguity, not content generation), *prod-hardening* (secrets rotation, Filevine rate limiting, idempotency keys on creates, structured logging).

### Pending items added

I've populated `agent:pending` with the actionable items from above. Say "go" (or "go phase 1") and I'll start executing.
<!-- agent:boundary:1ec2b035 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#xf9v] Run `docker compose up -d` inside FDE-Project-Interview; confirm FastAPI / Temporal UI / LocalStack come up clean
- [ ] [#7d7s] Inspect the three xlsx files (from `data/` on host) and capture actual column headers for Contacts, Address History, Projects
- [ ] [#awgb] Read `filevine_mapper.py` end-to-end; compare field mappings against Filevine Create Contact + Create Project docs; note any gaps or incorrect assumptions
- [ ] [#kkpf] Exercise the `MigrationWorkflow` via the Temporal UI or `temporal_router.py` endpoint with `write_json=true`, `send_to_filevine=false`; verify output JSON payloads look right
- [ ] [#8fah] Re-run the workflow with `send_to_filevine=true` and unset keys; confirm 401/403 path is handled without crashing the workflow
- [ ] [#ca60] Fill in the empty `mockPersenIDFetch.py` (rename to `mock_person_id_fetch.py`) — define how mock contactIds are generated and threaded into project payloads (currently inline in pipeline)
- [ ] [#rjqg] Audit Gemini usage: confirm `generate_project_description` + `generate_project_type_from_name` are called where they add real value; consider adding Gemini-assisted column mapping as a stronger use case
- [ ] [#z98w] Verify order of operations: contacts (with addresses merged) created first, project contactIds threaded from those contact payloads, then projects — check `filevine_mapper.py` actually does this
- [ ] [#apx7] Draft talking points: why Temporal vs a script, why Gemini here, how you'd harden for real keys
- [ ] [#rxs3] Optional polish: add a `make migrate` or FastAPI endpoint that kicks off the full migration in one command
<!-- /agent:pending -->
