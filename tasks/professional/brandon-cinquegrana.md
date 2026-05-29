---
agent_doc_session: e48b3024-e796-496f-85c5-7c37c346b046
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
❯ Please add a contact for brandon-cinquegrana
❯ - https://www.linkedin.com/in/brandoncinquegrana/

❯ use markitdown to convert /home/brian/work/btakita/agent-loop/tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.pdf to a md file
### Re: Add a contact for brandon-cinquegrana — done — opus-4-8

Created the corky contact at `mail/contacts/brandon-cinquegrana/` and enriched it from the LinkedIn profile you linked.

- **Created:** `corky contact add` scaffolded it, then I **normalized to the repo's kebab-case convention** — corky's `add` made a space-named dir (`Brandon Cinquegrana/`), the lone outlier among 56 kebab dirs, so I renamed the directory and the `[contacts.brandon-cinquegrana]` key in `mail/.corky.toml` to match. `CLAUDE.md → AGENTS.md` symlink intact; `corky contact info brandon-cinquegrana` resolves cleanly.
- **Enriched `AGENTS.md`** from the profile (1st-degree connection): headline *"Helping Startups Scale with On-Demand HR & Recruiting Support,"* Dallas–Fort Worth Metroplex, Rutgers University, 500+ connections, mutuals Alex & Saylor, and your recent connection note.
- **No email/phone yet** — only the LinkedIn URL was available (the Contact-info overlay wouldn't load reliably). Flagged in the contact's Notes and added `#brandon-email` to the backlog so it's not forgotten before any outreach.

### Re: Convert the settlement PDF with markitdown — done — opus-4-8

Converted via `markitdown[pdf]` (base markitdown lacks PDF support, so I ran the PDF extra):

- **Output:** `tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.md` — 137 lines, clean text (verified it's real legal prose, not OCR noise).
- Sensitive legal content, so I'm not quoting terms here — the file is local and uncommitted.

Both the contact files and the converted `.md` are left **uncommitted** in the working tree (you didn't ask to commit, and `#commit-push` is only defined as a preset, not invoked). Say the word and I'll commit + push.
<!-- agent:boundary:4b8d0a9b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#brandon-email] [Contact] Get Brandon Cinquegrana's email/phone before drafting any email - only LinkedIn is on file. Pull from the LinkedIn 'Contact info' panel or ask Brian directly. [recommended]
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
