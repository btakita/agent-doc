---
name: reference_resume_build_source
description: "resume/ submodule build reads data.toml via agent-resume, NOT BrianTakita.md (despite AGENTS.md); how to tailor a resume for a job"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 8cf89bf0-5549-4eb4-9e70-bce4539f12fb
---

The `resume/` submodule generates tailored PDFs: `make PROFILE=<name>` → `BrianTakita-<name>.pdf`.

**Source-of-truth gotcha:** `resume/AGENTS.md` says "Markdown is the source of truth," but `build.py` actually reads **`resume/data.toml` through the `agent-resume` tool**; `BrianTakita.md` is only a fallback used when agent-resume isn't installed (it prints "agent-resume not found, using BrianTakita.md fallback" to stderr). On 2026-05-29 the two had drifted — `data.toml` was missing the entire **Independent Consulting** experience block (Equity Funding Source, MonsterRodHolders, Confidential AI Startup) that `BrianTakita.md` already had, so a section filter naming "Independent Consulting" silently dropped it with no warning. Tracked as `#resume-data-toml-sync` in `tasks/professional/brandon-cinquegrana.md`.

**To tailor for a job:**
1. Add the experience to `data.toml` if missing (`[[experience]]` with `company`, `title`, `start`/`end`/`duration`, `skills`, and `[[experience.accomplishments]]` blocks). The `[sections].experience` profile filter matches the `company` field.
2. Write `profiles/<job>.toml` — `[highlight].phrases` / `.skills` auto-bold; `[sections].include` filters h2 sections; `[sections].experience` filters/reorders by company.
3. `make PROFILE=<job>`, then verify with `pdftotext`.

**Dreamable confidentiality:** present Dreamable only as "Confidential AI Startup", skills-only (Rust/Python/TS/LiveKit/WebRTC/CopilotKit), never the name or product architecture — see [[feedback_no_cleartext_secrets]] and the §5 review in the brandon-cinquegrana session. Verify `pdftotext … | grep -ic dreamable` is 0.

Note: editing the session doc's backlog via CLI `--pending-add` can be blocked by a live JB IPC listener ("refusing to absorb out-of-band pending mutation"); direct `git commit` of the one session file + `agent-doc reset --from-current` is the fallback. See [[feedback_agent_doc_nonconsecutive_dup_recovery]].
