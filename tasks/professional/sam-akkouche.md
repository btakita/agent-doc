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
<!-- agent:boundary:2f874885 -->
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
