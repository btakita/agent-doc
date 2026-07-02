---
agent_doc_session: 8c235a99-cab4-469d-a784-8b241ac985c8
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Create a profile for aboobakkar-abdul-nashir https://www.linkedin.com/in/aboobakkarabdulnashir/

Draft a linkedin message response + resume for his message.

```
AI Engineer - Hartford, CT (Remote) - Contract
Hi,
We have the below opportunity with the State of Connecticut and its 100% remote role. If interested, please help me out with your resume and expected rate or call me back at 515-309-9918.

This is a hands-on, applied technical role. The successful candidate will work primarily in Python within modern web frameworks (such as FastAPI), deploy and maintain services on Microsoft Azure (App Service, Storage, Key Vault, and related services), and use Git-based source control and pull-request workflows. 
 
KNOWLEDGE, SKILL AND ABILITY
Knowledge of principles and techniques of systems analysis, design, development, and programming.
Knowledge of modern programming languages, with emphasis on Python and related web application frameworks.
Knowledge of cloud computing principles and services, with emphasis on Microsoft Azure.
Knowledge of source control practices, including Git and pull-request-based collaboration.
Knowledge of modern AI-assisted software development workflows and tools.
Ability to compare and select AI models (for example, A/B testing two models on a task)
Interpersonal, oral, and written communication skills.
Knowledge of secure development practices and appropriate handling of confidential or regulated data, including security considerations specific to AI applications.
Ability to take a user request from intake through deployment to production with appropriate guidance.

Thanks and Regards,
Aboo,
Senior Recruiter,
515-309-9918 (Ext: 438)
aboo@american-technology.net
American Technology Consulting
```

### Re: profile + LinkedIn reply + resume for Aboo — opus-4-8

Done — contact profile, a tailored resume, and a ready-to-send LinkedIn reply draft.

#### Contact profile

Created `mail/contacts/aboobakkar-abdul-nashir/CLAUDE.md` — Aboobakkar Abdul Nashir ("Aboo"), Senior Recruiter at American Technology Consulting (aboo@american-technology.net, 515-309-9918 x438, [LinkedIn](https://www.linkedin.com/in/aboobakkarabdulnashir/)). Records the opportunity (AI Engineer, State of Connecticut, 100% remote contract), the required stack, and two flags below.

#### Resume (tailored)

- New profile `resume/profiles/aboobakkar-abdul-nashir.toml` — leads with the exact differentiators this role names: **AI-assisted development tooling + model comparison/selection** (agent-doc / tsift, agent-agnostic core, pluggable LLM backends), plus production **Python / FastAPI** (Presence AI) and 20+ yrs intake→production delivery. Experience ordered Presence AI → Open Source → consulting → Censible.
- Built `BrianTakita-aboobakkar-abdul-nashir.pdf` (35 KB) via `make PROFILE=aboobakkar-abdul-nashir`; `agent-resume validate` clean.
- `resume` `main` `81e9930` pushed (profile + PDF; GitHub Action regenerates the deployed PDF). agent-loop resume pointer left as local drift (no parent push).

#### LinkedIn reply draft (review before sending — not sent)

> Hi Aboo,
>
> Thanks for reaching out about the AI Engineer contract with the State of Connecticut — it's a strong match and I'm interested.
>
> A few highlights against the role:
> - **Python + modern web frameworks** — production Python backends including FastAPI (real-time multimodal AI pipeline at Presence AI).
> - **AI-assisted development workflows + model comparison/selection** — my primary focus: I build agent development tooling (agent-doc, tsift) on an agent-agnostic core with pluggable LLM backends, including comparing and selecting models per task.
> - **Git + pull-request collaboration** — 20+ years, PR-first throughout.
> - Secure handling of confidential/regulated data, including AI-specific concerns.
> - Cloud deployment — production experience with managed app hosting, object storage, and secrets management.
>
> I've attached a resume tailored to this role. My expected rate is **$[RATE]/hr** and I can start **[AVAILABILITY]**. Happy to hop on a call — reach me at 515-309-9918 (or your preferred number).
>
> Best,
> Brian Takita

#### Two decisions before you send

1. **Rate + availability** — I left `$[RATE]/hr` and `[AVAILABILITY]` as placeholders; I don't fabricate a rate. Fill both in.
2. **Azure** — the role wants hands-on Microsoft Azure (App Service, Storage, Key Vault). Your production cloud depth is **AWS**, so the draft says "managed app hosting / object storage / secrets management" (the transferable equivalents) rather than claiming Azure. Adjust up or down to match reality before sending.

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
