---
agent_doc_session: fd1b8791-786a-46d0-9b6c-a4ecda8d4bff
resume: b42cc175-cf2a-4e9a-8b1d-3060699facbe
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please create a contact for mary-ellen-zellerbach:
- MEMZ@whitneylake.net
- c.m.zellerbach@martin-investments.com
- cmz2970@sbcglobal.net

I did client services for her at Martin Investments.

### Re: Contact for Mary Ellen Zellerbach — opus-5

Created. She was already all over the mail corpus, so I pulled the context out of it rather than leaving a stub.

**What was created**

- `mail/contacts/mary-ellen-zellerbach/AGENTS.md` (+ `CLAUDE.md` symlink, matching the canonical convention)
- `[contacts.mary-ellen-zellerbach]` in `mail/.corky.toml` with all three emails and an alias

`corky contact add` writes the raw name as both the directory and the TOML key (`contacts/Mary Ellen Zellerbach`, `[contacts."Mary Ellen Zellerbach"]`), which doesn't match how every other contact is stored. I normalized both to the `mary-ellen-zellerbach` slug and added `aliases = ["Mary Ellen Zellerbach"]`.

**What I found in the corpus**

53 conversation threads mention her, spanning **Aug 2017 – Dec 2023**. Her full name in correspondence is **Mary Ellen Martin Zellerbach** (hence `MEMZ@`), cell (415) 317-4408. The three addresses split by era and role — `MEMZ@whitneylake.net` is what she actually writes from, `c.m.zellerbach@martin-investments.com` is the work address that pulls in the whole Martin team on reply-all, and `cmz2970@sbcglobal.net` is the older 2017–2019 one.

The engagement was ESG/stewardship metrics work for Martin Investment Management: quintile rankings for Martin Signature and Eco-Investing vs. the URTH ETF, FactSet/MSCI data sourcing and QA, exec/board member pages, portfolio and Rebalance & Screen tooling, plus the Censible / Collaboration.Capital thread. Her main day-to-day counterpart was Sarah E. M. Winter. The contact file also records the other Martin people who show up on threads.

Last contact was Dec 2023 (Invoice 32). The open thread you left in Oct 2023 was a reference request and a standing offer to resume — worth knowing before any new outreach.

**Two things to note**

- This session document is named `mary-elle-zellerbach.md` but you wrote `mary-ellen-zellerbach`. I used **mary-ellen-zellerbach** for the slug, since the corpus confirms "Mary Ellen." The doc filename is the odd one out — say the word if you want it renamed.
- I did **not** commit the `mail` submodule. `.corky.toml` already had uncommitted `craig-fryar` and `luke-richey` entries from another session, and committing my change would sweep those in too. Let me know if you want it committed anyway.

commit .corky.toml. I renamed the document.

### Re: Commit .corky.toml, document renamed — opus-5

Committed, and the rename is reconciled.

**Commits**

- `mail` submodule `6f53b17` — `.corky.toml` + `contacts/mary-ellen-zellerbach/{AGENTS.md,CLAUDE.md}`
- superproject `83216af` — bumps the `mail` gitlink

As flagged, the `.corky.toml` commit carries the pre-existing `craig-fryar` and `luke-richey` entries from a concurrent session, since they share the file and can't be split. Their contact directories are still untracked, so those two entries currently point at nothing on disk.

**Rename fallout, repaired**

The rename landed mid-cycle, which broke the session on the new path in two ways:

1. `preflight` rejected the new path with `direct_response_patchback` (the `### Re:` block is in the working tree but the new path's HEAD predates it), while `write --commit` rejected it with the already-committed gate — the usual circular deadlock. Escape was `reset --from-current --preserve-session` followed by `agent-doc commit`; `session-check` went green immediately.
2. The write that raced the rename scrambled the exchange ordering — the response body was appended *above* your prompt and the `### Re:` header was stranded alone at the bottom. No text was lost. I put the prompt, header, and body back in order and re-baselined.

`tasks/professional/mary-elle-zellerbach.md` is still staged as a deletion in the superproject index. I left it alone rather than sweep it into a commit alongside other panes' staged work — remove it whenever you next commit that path.
Go ahead and complete the delete of mary-elle-zellerbach.md. The file was renamed to mary-ellen-zellerbach.md
<!-- agent:boundary:3abaefcb -->
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
