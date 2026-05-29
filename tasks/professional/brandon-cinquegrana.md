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
❯ Should I add the description of my recent projects to my freelancing work or as separate companies?
❯ - dreamable
❯ - equityfundingsource
❯ - monsterrodholders

### Re: Recent projects — freelancing umbrella vs. separate companies — opus-4-8

Default to **one freelancing/consulting umbrella** with dreamable, equityfundingsource, and monsterrodholders listed as dated engagements beneath it. Break one out into its own company entry only when the relationship was actually a *role*, not a contract.

The decision rule:

| Present as… | When |
|---|---|
| **Sub-bullet under your consulting banner** | Engaged as a contractor/consultant; deliverable-based; weeks-to-months; no title or equity. |
| **Standalone company entry** | You held a defined ongoing role (founder, co-founder, principal, officer) or equity — or the engagement is long/notable enough that the name carries more weight than the umbrella. |

Why the umbrella is the default for a recruiter audience (Brandon screens for startups):

- Three separate short company entries read as job-hopping. The same three as clients under *"Independent Software Consultant, 2023–present"* read as a stable, in-demand consultant.
- It's truthful to a contractor relationship and parses cleanly in ATS — continuous employment, one employer block.
- You frame the narrative once instead of defending three exits.

Applying it to your three:

- **monsterrodholders** — WooCommerce/WordPress client work. Umbrella, client bullet. Lead with outcome (migration, checkout/WAF fixes, uptime/revenue), not the stack.
- **equityfundingsource** — the one to decide deliberately. Founder/principal/equity stake → standalone entry. Client build → umbrella. Tell me which and I'll phrase it.
- **dreamable** — present as a **completed engagement**, neutrally, under the umbrella. It closed in a settlement / mutual release, so keep it low-profile: dates plus a neutral scope line, no outcome drama, and never reference the settlement. Don't drop it if doing so opens a visible gap, but don't feature it either.

Structure I'd use:

```
Independent Software Consultant — Self-employed | 2023–Present
  • EquityFundingSource — <one-line impact>
  • MonsterRodHolders   — <one-line impact>
  • Dreamable           — <neutral scope line>
```

If equityfundingsource was a founder role, lift it to its own header *above* the consulting block and keep the other two beneath the umbrella.

One question back: were any of these equity/founder roles, or were all three client contracts? That single fact is what changes the answer.

❯ Please transcribe + diarize the call with brandon-cinquegrana.
❯ [phone_20260528-140602_2144332110.m4a](../../mail/contacts/brandon-cinquegrana/recordings/phone_20260528-140602_2144332110.m4a)

### Re: Recent projects — freelancing umbrella vs. separate companies — opus-4-8

Default to **one freelancing/consulting umbrella** with dreamable, equityfundingsource, and monsterrodholders listed as dated engagements beneath it. Break one out into its own company entry only when the relationship was actually a *role*, not a contract.

The decision rule:

| Present as… | When |
|---|---|
| **Sub-bullet under your consulting banner** | Engaged as a contractor/consultant; deliverable-based; weeks-to-months; no title or equity. |
| **Standalone company entry** | You held a defined ongoing role (founder, co-founder, principal, officer) or equity — or the engagement is long/notable enough that the name carries more weight than the umbrella. |

Why the umbrella is the default for a recruiter audience (Brandon screens for startups):

- Three separate short company entries read as job-hopping. The same three as clients under *"Independent Software Consultant, 2023–present"* read as a stable, in-demand consultant.
- It's truthful to a contractor relationship and parses cleanly in ATS — continuous employment, one employer block.
- You frame the narrative once instead of defending three exits.

Applying it to your three:

- **monsterrodholders** — WooCommerce/WordPress client work. Umbrella, client bullet. Lead with outcome (migration, checkout/WAF fixes, uptime/revenue), not the stack.
- **equityfundingsource** — the one to decide deliberately. Founder/principal/equity stake → standalone entry. Client build → umbrella. Tell me which and I'll phrase it.
- **dreamable** — present as a **completed engagement**, neutrally, under the umbrella. It closed in a settlement / mutual release, so keep it low-profile: dates plus a neutral scope line, no outcome drama, and never reference the settlement. Don't drop it if doing so opens a visible gap, but don't feature it either.

Structure I'd use:

```
Independent Software Consultant — Self-employed | 2023–Present
  • EquityFundingSource — <one-line impact>
  • MonsterRodHolders   — <one-line impact>
  • Dreamable           — <neutral scope line>
```

If equityfundingsource was a founder role, lift it to its own header *above* the consulting block and keep the other two beneath the umbrella.

One question back: were any of these equity/founder roles, or were all three client contracts? That single fact is what changes the answer.

#### Note: the IPC corruption that hit while you typed this prompt

That was the known **`#ipcfullprompt`** bug — full-document IPC corruption while typing a live prompt. Preflight auto-recovered it this cycle: it moved the stale boundary back to the end of the completed turn and repaired the prompt prefixes, so your four prompt lines survived intact. The only residue is a stray empty `###` heading below `/agent:exchange` (harmless, outside the response region). There's also a `stuck_captured_cycle` warning for an orphaned 1863-byte capture from the corrupted cycle — but both prior responses are confirmed in HEAD, so nothing was lost, and this finalize reconciles it.

The bug already has a full plan (`tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md`), but `#ipcfullprompt` was prematurely moved to the done archive on 2026-05-26 while still plan-only and unfixed. It just reproduced on 0.34.0, so I've re-opened it in the `agent-doc-bugs2.md` backlog as `#ipcfullprompt-recur`.
<!-- agent:boundary:12716557 -->
<!-- /agent:exchange -->
###
<!--
Please read tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.md
-->

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
