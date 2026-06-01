---
agent_doc_session: 3af2ebc5-cc8e-487f-b92c-f5bf3f7e9e00
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Create a corky contact for sam-akkouche. https://www.linkedin.com/in/sam-akkouche/

### Re: Create a corky contact for sam-akkouche — opus-4-8

Created the contact at `mail/contacts/sam-akkouche/` (`corky contact add` + canonical `AGENTS.md`, with `CLAUDE.md` symlinked to it).

**Profile (from public web search — LinkedIn itself is auth-walled):**
- **Sam Akkouche** — Recruiter / Staffing Specialist at **Signify Technology**, London.
- Signify Technology is a global functional-programming staffing agency (Rust, Scala, Go, plus Software Engineering / Data / Cloud / Leadership). The Rust niche overlaps directly with your work.
- LinkedIn: https://www.linkedin.com/in/sam-akkouche/

**Couldn't capture:** exact job title, tenure, and an email address — LinkedIn returned 404/auth-wall to the fetcher. The `AGENTS.md` flags these as unknown to confirm on first real contact, and `corky contact info` shows `Emails: (none)`.

The contact file is structured the same way as the other professional contacts (role / company / LinkedIn / location header, then Relationship / Formality / Tone / Topics / Research / Notes), so it's ready for email drafting once an address comes in.

Sources: [Sam Akkouche — Signify Technology (LinkedIn)](https://www.linkedin.com/in/sam-akkouche/), [Signify Technology](https://www.signifytechnology.com/)
❯ Create a resume for my call with sam-akkouche tomorrow morning at 11am.

### Re: Create a resume for the sam-akkouche call (tomorrow 11am) — opus-4-8

Built a tailored resume: **`resume/BrianTakita-sam-akkouche.pdf`**.

**Tailoring rationale** — Sam recruits for **Signify Technology**, a functional-programming staffing agency (Rust / Scala / Go + Software Engineering / Data / Cloud / Leadership). So the profile (`resume/profiles/sam-akkouche.toml`) leads with your **Rust + functional/reactive + systems** signal:

- **Skills** surface Rust, Go, Functional/Reactive Programming, TypeScript, Python, ZeroMQ, Docker, AWS.
- **Experience order:** Independent Consulting (recent Rust tooling — corky, agent-doc, tagpath) → Presence AI (real-time multimodal pipeline) → Open Source → Milyoni → TrueCar → Censible → Pivotal Labs.
- Auto-bolds Rust, functional programming, reactive contexts, CRDT/3-way-merge/streaming-merge, event-driven, cloud, plus leadership/career signals (20+ years, 20+ startups, RailsConf 2008) for Signify's Leadership vertical.

**Verification:**
- `make PROFILE=sam-akkouche` built cleanly.
- `pdftotext` confirms **10 Rust mentions** and all targeted companies present.
- **Confidentiality:** Dreamable leak check = **0** — it renders as "Confidential AI Startup" (skills-only), per the confidentiality rule.

**Left uncommitted** (no `#commit-push` invoked): the new `resume/profiles/sam-akkouche.toml` and `resume/BrianTakita-sam-akkouche.pdf` are in the `resume/` submodule working tree, plus the earlier `mail/` contact. Say the word and I'll commit + push.

One gap: the profile leans on public web data for Sam's role since LinkedIn was auth-walled — if you learn specifics before the call (exact role/team, what they're hiring for), I can re-tailor in minutes.
<!-- agent:boundary:f778b2ed -->
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
