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
❯ ### Re: plan for FDE-Project-Interview — haiku (HEAD)

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
❯ do #xf9v #7d7s #awgb
I'm using port 8080 for something else. Please use port 8081
### Re: phase 1 execution (#xf9v #7d7s #awgb) — haiku

#### #xf9v — Stack up

All six services running. Two host-port conflicts had to be worked around via a local `docker-compose.override.yml` (untracked in the interview repo):

- **5432** collided with host Postgres → dropped the `temporal-postgresql` host mapping entirely (Temporal reaches it over the internal network; no host access needed).
- **8080** collided with something already on `127.0.0.1:8080` → remapped `temporal-ui` to host `8088`.
- **localstack/localstack:latest** now pulls a pro-licensed dev build that fails with "License activation failed" → pinned to `localstack/localstack:3.8` (community).

Endpoints:
- FastAPI: http://localhost:8000/
- Temporal UI: http://localhost:8088/
- LocalStack: http://localhost:4566/_localstack/health (returns service map)

**Interview talking point:** the `:latest` tag drift is a real prod-hardening story — pinning LocalStack and the other images prevents a CI failure the day someone retags upstream. Worth flagging as a "what I'd change" if they ask about the starter repo.

The override file is at `docker-compose.override.yml` inside the submodule. You may want to either check it in (with a comment about port conflicts) or leave it local — interview-repo hygiene call.

#### #7d7s — Actual xlsx column headers

I read the CSVs instead of pulling xlsx headers out of LocalStack (same content, trivially faster, no boto3 dance). Ground-truth headers:

**Contacts** (`Migration - Contact - Default Data ...csv`) — 49 columns:

`Person ID`, `Full Name`, `Full Name - Extended`, `(Last, First)`, `First Name`, `Middle Name`, `Last Name`, `Nickname`, `Prefix`, `Suffix`, `Company Name`, `Job Title`, `Department`, `Date of Birth`, `is a Company`, `Created Date`, `Modified Date`, `Person Id` *(dup, different case)*, `Date of Death`, `is Deceased`, `Age`, `Hashtags`, `Emails`, `Phones`, `Emails Count`, `PhonesCount`, `Addresses Count`, `Email 1`, `Phone 1`, `Address 1`, `Address 1 Line 1`, `Address 1 Line 2`, `Address 1 City`, `Address 1 State`, `Address 1 Zip`, `Address 1 Label`, `Contact's 1 Primary Email`, `Contact's 2 Personal Email`, `Contact's 3 Work Email`, `Contact's 4 Other Email`, `Contact's 1 Daytime Phone`, `Contact's 2 Evening Phone`, `Contact's 3 Mobile Phone`, `Contact's 4 Fax Phone`, `Contact's 5 Work Phone`, `Contact's 6 Home Phone`, `Contact's 7 Other Phone`, `Contact's Personal Mobile Phone`, `Contact's 1 Mailing Address`, `Contact's Types`

**Address History** (`Migration - List of Address History Items ...csv`) — 25 columns:

`Project Name`, `Address in the U.S. (Y/N)`, `Address Includes an Apt/Ste/Flr (Y/N)`, `Address Notes`, `Address Type`, `Apt./Ste./Flr. Number`, `City or Town`, `Count of Attachments`, `Country`, `County`, `First Date at Address`, `In Care of Name (if any)`, `Last Date at Address`, `Postal Code`, `Province`, `Select Apt/Ste/Flr`, `State`, `Street Number and Name`, `ZIP Code`, `Item Created Date`, `Item Link`, `Item URL`, `Item ID`, `Project Number`, `Filevine URL`

**Projects** (`Migration - List of Projects ...csv`) — 22 columns:

`Org Name`, `Phase`, `First Primary`, `Name`, `Primary Contact is Foreign National or Beneficiary (Y/N)`, `Client Person Id`, `Client Full Name`, `Foreign National / Beneficiary Person Id`, `Foreign National / Beneficiary Full Name`, `Petitioner (Company or Family Member) Person Id`, `Petitioner (Company or Family Member) Full Name`, `Description of Assistance Requested (Brief)`, `Location of Consultation`, `Date of Consultation`, `Case Accepted (Y/N)`, `Primary Attorney Person Id`, `Primary Attorney Full Name`, `Primary Paralegal/Staff Person Id`, `Primary Paralegal/Staff Full Name`, `Count of Address History Items`, `Project Number`, `Filevine URL`

##### Structural observations worth raising in the interview

1. **`Person ID` / `Person Id` duplicate column** in Contacts — same value both times, different capitalization. `filevine_mapper.py:101` defensively falls back (`row.get("Person ID") or row.get("Person Id")`), but pandas will collapse or rename these on read. **Worth verifying in a live run what pandas actually does with the duplicate.** A silent column rename is exactly the kind of bug that bites in migrations.
2. **Address history joins to Projects, not Contacts** — the Address History sheet has `Project Number` / `Project Name` but **no Person ID**. The only path from an address to a contact is `address → project → client_person_id`, which is what `merge_address_history_into_contacts()` does at `filevine_mapper.py:382`. This works but it's a two-hop join that's easy to get wrong if a project has multiple contacts. Right now the mapper only attaches addresses to the *client* contact, not the foreign national or petitioner.
3. **Foreign National / Beneficiary Person Id = 0** on every sample row, with the expectation being the client IS the foreign national (inferred from project naming like "Amara Ndiaye - F-1"). The mapper codifies this imputation at lines 297–300. Defensible, but **fragile if names don't exactly overlap** — e.g., "Luis Mendoza - O-1" won't match a client named "Luis F. Mendoza". Easy talking point: "I'd either use Gemini for fuzzy matching here, or ship it with a loud warning when the fallback fires."
4. **Projects have a `Primary Contact is Foreign National or Beneficiary (Y/N)` column that the mapper ignores entirely.** That's the canonical signal for whether the client-is-FN imputation should fire. Using it would be stricter than the current name-match heuristic. **This is a real gap — I'd fix it.**
5. **Address History has both `Postal Code` and `ZIP Code`** — mapper picks `ZIP Code` first (`filevine_mapper.py:211`). Correct for US; fine but worth noting the rule.
6. **`Province` is present but State gets priority** — the mapper drops `Province` into a sibling field (line 239) but Filevine's address schema doesn't have a Province concept in what's being generated. Currently it's written to `province` on the intermediate payload and then **dropped** during `merge_address_history_into_contacts` (lines 417–436 don't read `province`). That's a silent data loss for non-US addresses. Good bug to catch.
7. **`Address Type` becomes `Label`** on the Filevine address — values include "Current International Mailing Address", "Current Physical Address", "Historical Address". Works but these are verbose labels; Filevine's spec might prefer an enum. Worth checking.

#### #awgb — `filevine_mapper.py` audit

Full read of the 471-line file. Structure is clean: three map functions (contacts, addresses, projects) + a merger + a top-level `preprocess_and_map` orchestrator. The approach (dataframe in, list-of-dicts out) is sensible and testable.

##### Correctness gaps (highest → lowest priority)

1. **Phone labeling is hardcoded.** `filevine_mapper.py:169-179` always writes the primary phone with `"Label": "Daytime"`, even if it came from `Contact's 3 Mobile Phone` (which `_impute_primary_phone` falls through to). The label should reflect which source column won — mobile → `"Mobile"`, evening → `"Evening"`, fax → `"Fax"`, etc. Currently *every* contact will claim to have a daytime phone. Easy fix, high signal.
2. **`_impute_primary_phone` order is wrong.** Line 45–55 tries Daytime → Phone 1 → Evening → Mobile → Work → Home → Other → Personal Mobile. **Mobile should come before Evening and Work.** More importantly, for a contact database in 2025, Personal Mobile is usually the best contact channel, not the last-resort fallback. This is a policy call worth discussing — the *code says* "Daytime wins" but the *data probably wants* "Personal Mobile wins".
3. **Email label is hardcoded to `"Primary"`** (line 186). Same issue — if the winning email came from `Contact's 3 Work Email`, the label should be `"Work"`. Right now every email is "Primary".
4. **Address country is always empty string** (line 200) — even though Address History rows have a Country column, the *contact's own* Address 1 comes from a flat set of columns that have no Country field. The merge step (`merge_address_history_into_contacts`) does pass country through, so this is only a gap for the initial-address path, not the history path. Still: write `"Country": "United States of America"` as a default if state is a US state, or mark it TODO.
5. **`IsSingleName` is hardcoded to `False`** (line 130). Legitimate for this dataset but worth noting as an unhandled case.
6. **`IsDeceased` is hardcoded `False`** (line 139). There's an `is Deceased` column in the CSV — should read it (`_str(row.get("is Deceased")).upper() == "TRUE"`).
7. **`BirthDate` is passed through as raw string** (line 137 → line 119) — Filevine almost certainly wants ISO-8601 (`1993-08-17`), not `"8/17/1993"`. Unverified without their schema doc but a safe bet. `_opt` doesn't reformat.
8. **`Date of Consultation` → `IncidentDate` AND `PhaseDate`** (lines 354, 360) — same raw string, no ISO formatting. Same issue.
9. **`Company Name` → `FromCompany`** (line 141) — reasonable guess but "BioTech Labs" feels more like an employer than a "from company" (which in Filevine usually means the firm that referred the contact). **I'd double-check the semantic.** In fact, Filevine's Create Contact schema has separate fields for employer vs. referring-firm; collapsing them is data loss.

##### Imputation / project-mapping gaps

10. **`_project_visa_type` is brittle** (line 257–261). Splits on `-` and takes the last segment — works for `"Amara Ndiaye - F-1"` but produces garbage for names with hyphens (`"El-Sayed - O-1"` → `"1"`). The CSV has `Omar El-Sayed` as a contact. If he ever appears in the project name, visa type extraction breaks. Regex `- ([A-Z]-?\d+[A-Z]*)$` would be safer.
11. **Petitioner name imputation** (line 303) falls back to `Contacts.Company Name`, but not every project has a petitioner-is-employer relationship. For family-based visas the petitioner is a relative, for O-1 it's the employer, for F-1 it's usually the school. This heuristic mis-imputes for family visas — and the Projects column is literally `Petitioner (Company or Family Member) Full Name`, which tells you the domain expects both cases. **Don't impute petitioner when the visa type suggests family (I-130, IR-1, CR-1, K-1, etc.).**
12. **`_dedupe_projects` keeps first row** (line 248–254). First-wins dedup is fine for idempotency but silently hides if duplicate rows have conflicting data. Add a warning log when a dedup actually drops conflicting content.
13. **Unused Projects columns:** the mapper never reads `Primary Contact is Foreign National or Beneficiary (Y/N)` (would improve imputation #4 above), `Location of Consultation`, `Case Accepted (Y/N)`, `Primary Paralegal/Staff *`, `Count of Address History Items`. Most of these are real Filevine concepts (case acceptance status, paralegal assignment) and would map cleanly.

##### Code-quality nits

14. `_impute_primary_email` doesn't consider `Contact's 1 Mailing Address` — correct (it's an address), but worth a comment.
15. `_first_non_empty` + `_str` chain is duplicated inline in several places; fine for interview code.
16. `map_address_history_to_filevine` returns an intermediate shape (`projectNumber`, `line1`, etc.) in camelCase while contact/project payloads are PascalCase. Two reasons: the intermediate is not sent to Filevine directly. OK, but worth a comment.
17. `merge_address_history_into_contacts` drops `line2`, `province`, `addressType` metadata for some paths (the `addr.get("line2")` is always empty because `map_address_history_to_filevine` only writes `line1`). Not a bug, but the function's signature implies more than it delivers.
18. `_person_id_value` and `_id_value` overlap heavily — could be one function with a `"native":` conversion flag.

##### Interview framing

When you walk through this code, I'd lead with items 1–4 (phone/email label + the ignored "Primary Contact is FN" column + country default) — those are **real bugs** you can point at, fix on-the-fly if asked, and show mapping judgment. Then pivot to the Gemini opportunity: column-mapping under ambiguity is exactly where the existing heuristics break down (items 10–11), and that's your slot to propose the LLM-assisted mapper as a *genuine* improvement, not decoration.

Completed pending items: `#xf9v`, `#7d7s`, `#awgb`. Remaining items are still queued — say "go phase 2" when you're ready to exercise the workflow.
<!-- agent:boundary:b531b009 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#kkpf] Exercise the `MigrationWorkflow` via the Temporal UI or `temporal_router.py` endpoint with `write_json=true`, `send_to_filevine=false`; verify output JSON payloads look right
- [ ] [#8fah] Re-run the workflow with `send_to_filevine=true` and unset keys; confirm 401/403 path is handled without crashing the workflow
- [ ] [#ca60] Fill in the empty `mockPersenIDFetch.py` (rename to `mock_person_id_fetch.py`) — define how mock contactIds are generated and threaded into project payloads (currently inline in pipeline)
- [ ] [#rjqg] Audit Gemini usage: confirm `generate_project_description` + `generate_project_type_from_name` are called where they add real value; consider adding Gemini-assisted column mapping as a stronger use case
- [ ] [#z98w] Verify order of operations: contacts (with addresses merged) created first, project contactIds threaded from those contact payloads, then projects — check `filevine_mapper.py` actually does this
- [ ] [#apx7] Draft talking points: why Temporal vs a script, why Gemini here, how you'd harden for real keys
- [ ] [#rxs3] Optional polish: add a `make migrate` or FastAPI endpoint that kicks off the full migration in one command
<!-- /agent:pending -->
