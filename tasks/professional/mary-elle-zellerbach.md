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
Please create a contact for mary-ellen-zellerbach:
- MEMZ@whitneylake.net
- c.m.zellerbach@martin-investments.com
- cmz2970@sbcglobal.net

I did client services for her at Martin Investments.
<!-- agent:boundary:25453ba3 -->
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
