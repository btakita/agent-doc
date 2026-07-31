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

### Re: Complete the delete of mary-elle-zellerbach.md — opus-5

Done — superproject `c995d3a`.

Before committing I diffed `HEAD:tasks/professional/mary-elle-zellerbach.md` against the current `mary-ellen-zellerbach.md`: the old path's content is entirely superseded, with the only differences being the scrambled exchange ordering I already repaired and the turns that landed after the rename. Nothing unique was lost.

`tasks/professional/` is now clean of the old path — gone from both the index and HEAD, with only `mary-ellen-zellerbach.md` remaining on disk.

Please create a contact for ehsin-naeem: ehsin.naeem@apollo-solutions.com

Please help me draft an email to mary-ellen-zellerbach. So far I have the following. Should I apologize for the Apollo  recruiter Ehsin reaching out to Mary Ellen before I reached out to her?

```
Hi Mary Ellen,

I hope everything is well with you and your family.

I'm reaching out because I received an offer from Acadian Financial Services for a contractor position through Apollo Solutions. They asked for a reference and since my work with Martin Investments aligned, I thought of you.

My other reference let me know that Ehsin reached out to him...so I apologize if the email came from him before I had a chance to reach out to you first.

--
Thank you,
Brian Takita
```

### Re: Ehsin Naeem contact + reference email to Mary Ellen — opus-5

**Contact created** — `mail/contacts/ehsin-naeem/` + `[contacts.ehsin-naeem]` in `.corky.toml`, slug-normalized the same way as the last one. I filled in what the repo already knows: Apollo Solutions is the contracting counterparty, Acadian Financial Services the end client, and the open written-confirmation items from `tasks/recruit/acadian/recruiter-questions-draft.md`. Committed as `a00a5b8` in `mail`, gitlink `f2e26a3`.

### Should you apologize? No.

Nothing happened that warrants one. You gave a recruiter your references; he contacted them. That is the process working, not a breach of it. Three reasons the apology actively hurts you:

1. **It invents a transgression.** Mary Ellen has no reason to think the sequencing was wrong until your email tells her it was. An apology is the first thing that makes it look like a problem.
2. **It's the wrong posture for an ask.** You're requesting a favor after two and a half years of silence. Opening apologetic and closing with a request reads as smaller than you are — and she knew you as the contractor who caught MSCI's errors on live client reports.
3. **You don't actually know it happened to her.** Your draft says your *other* reference heard from Ehsin. Apologizing to Mary Ellen for something that may not have reached her yet is confusing.

What the situation does call for is a **heads-up**, which is a different thing: give her the context so that if Ehsin's email is already sitting in her inbox, it makes sense. Courteous, not contrite.

### The bigger gap in your draft

It never asks. "I thought of you" implies the request without making it, which leaves her to infer what she's agreeing to. A reference request needs four things your draft is missing: the explicit ask, what it will cost her (a short call or a few emailed questions), when, and an easy way to say no.

I also added a bridge for the gap. Your last message to her, Oct 2023, was itself a reference request that went unanswered when the project went on hold — so this isn't cold, it's a continuation. Worth naming.

### Draft

Committed at `mail/drafts/2026-07-31-mary-ellen-zellerbach-reference-request.md`, in your voice per `mail/voice.md` (`Hi`, contractions, `&`, no "Best,"). To `MEMZ@whitneylake.net` — the address she actually writes from — cc'ing the Martin address so it lands either way.

> **A reference request — & hello after too long**
>
> Hi Mary Ellen,
>
> I hope you & your family are doing well. It's been a while since our work together wrapped up, & I've thought about it often — the stewardship metrics work was some of the most interesting analysis I've done.
>
> I'm reaching out with a favor. I've received a contractor offer from Acadian Financial Services, through Apollo Solutions, & they've asked for references. The work is close to what we did at Martin — data-heavy financial analysis & the tooling around it — so you were the first person I thought of.
>
> Would you be willing to speak to my work? It would likely be a short call or a few questions by email, sometime in the next week or two.
>
> One heads-up: Ehsin Naeem, the recruiter at Apollo, has been moving quickly & has already contacted another of my references. If you've already heard from him, that's why — I'd wanted to reach you first & give you the context myself.
>
> No pressure at all if the timing doesn't work. Either way, it's good to be back in touch.
>
> --
> Thank you,
> Brian Takita

The heads-up paragraph carries the same information your apology did, minus the concession. If you want it warmer, "I'd wanted to reach you first" can become "I'd hoped to reach you first" — that's as close to an apology as this needs to get.

Not sent — draft only, per your standing rule. Say the word and I'll push it to Gmail Drafts.

One unrelated flag from preflight: `stale_install` on agent-doc — the installed binary and cdylib predate local source edits in `src/agent-doc`. That's another session's work, so I left it; `make install` there when convenient.
<!-- agent:boundary:7eb62eda -->
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
