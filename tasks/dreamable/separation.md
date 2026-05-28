---
agent_doc_session: 892a6a1f-8c9b-4455-b8fe-6299d3191e71
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please evaluate https://signrequest.com/#/document/25061d5c-0117-4650-aa61-2acd582c0252/912765c8-641b-4269-9f8a-eea938973a82/?utm_source=signrequest&utm_medium=email&utm_campaign=signrequest-received&signer_token=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1dWlkIjoiOTEyNzY1YzgtNjQxYi00MjY5LTlmOGEtZWVhOTM4OTczYTgyIiwiZW1haWwiOiJicmlhbi50YWtpdGFAZ21haWwuY29tIiwiaWF0IjoxNzc5OTkwMzcwLjY1Mzk2OSwiZXhwIjoxNzgwNTk1MTcwLjY1Mzk5LCJsaW5rX3R5cGUiOiJzaWduaW5nIn0.NUEUbVZTCyzYRTdZZbyfndfQnBzC2kUW1BqYw6963l4

❯ Create an addendum document with my public github projects. Include projects created after April 6. Include a line for my private projects.
### Re: SignRequest — Settlement and Mutual Release Agreement — opus-4-7

Loaded the document via chromium-bridge ("052826 Dreamable - Brian Takita Settlement and Mutual Release Agreement.pdf", sent by rob@dreamable.com, dated May 28, 2026, already counter-signed by Rob).

**Structure**

- Parties: Dreamable, Inc. (846A North Point St., SF) and Brian Takita (PO Box 571 / 1 Park St., North Grosvenordale, CT).
- Recitals: Consulting Agreement + Addendum effective April 5, 2026. Services rendered through May 26, 2026. April 6, 2026 email terminated the Consulting Agreement; engagement continued under the same terms except for a payment-amount modification.
- 12 terms + signatures.

**Terms summary**

1. **Engagement Period** — April 6, 2026 → May 26, 2026.
2. **Settlement Payment** — $5,000 as "full and final settlement … all compensation obligations satisfied in full."
3. **Intellectual Property** — Broad assignment: all inventions, code, documentation, and work product developed *for* Company *or in connection with the Services* during the engagement is Company property. Catch-all assignment at the end of the section sweeps anything not already covered by a prior written assignment.
4. **Prior Inventions** — Confirms Consultant retains exclusive ownership of all Prior Inventions listed *in the Addendum*. Company gets a nonexclusive, royalty-free, perpetual license to anything Consultant's OSS pulled into Company products under the published OSS license terms.
5. **Confidentiality** — Standard, survives termination.
6. **Company Property** — All documents/code/materials created or received in connection with Services are Company property; no use or disclosure post-engagement.
7. **Mutual Release** — Full mutual release of all claims arising out of the engagement, with explicit California Civil Code §1542 waiver (you give up unknown claims too).
8. **Non-Disparagement** — Symmetric; truthful legal testimony preserved.
9. **Patent/Copyright Assistance** — Consultant assists Company in securing rights to the inventions described in Section 3. Survives termination.
10. **Governing Law / Arbitration** — California law; JAMS arbitration in San Francisco County; each side bears own fees + equal arbitration costs.
11. **Entire Agreement** — Supersedes prior agreements *except* the April 5, 2026 Consulting Agreement, the Addendum thereto, and any subsequent agreements/termination notices including the April 6, 2026 termination notice.
12. **Severability** — Standard.

**Things to weigh before signing**

1. **$5,000 vs. expected fees.** At the original $4,000/week rate, April 6 → May 26 is ~7 weeks ≈ $28k. If you've already been paid the bulk of that and $5k is a true tail-end settlement, fine. If $5k is the *total* you'll have received for the engagement period, the gap is large and worth resolving before signing — the §7 mutual release with §1542 waiver extinguishes any future unpaid-wages claim.
2. **Section 3 is broad.** "In connection with the Services" can be read aggressively. The Addendum's §2(b) open-source carve-out survives via §11, but Section 3 here does not re-cite or reaffirm it. Two clean ways to firm this up:
   - Add a sentence to Section 3: *"For avoidance of doubt, Section 3 does not modify the carve-outs in §2 of the Addendum, including the open-source carve-out at §2(b)."*
   - Or attach the engagement-period-projects addendum (see below) as Exhibit A-1 so the parties have a contemporaneous record of which works fall under §2(b).
3. **Section 4 only references Prior Inventions** as of the Addendum's Effective Date. The Addendum's §2(b) categorically covers *future* OSS too, but the Settlement does not echo that. The supplemental addendum I drafted below makes the application explicit for the April 6 – May 26 window.
4. **§1542 waiver in §7** is standard but irreversible — it covers claims you don't know about yet (e.g., later-surfacing wage-and-hour, retaliation, or IP-misappropriation claims). Only sign once the §5,000 number, the IP boundary, and any other known issues are resolved to your satisfaction.
5. **JAMS in San Francisco County** is a venue burden for a CT-based consultant. Probably not worth re-negotiating, but it's a meaningful cost if a dispute ever arose.
6. **Non-disparagement (§8)** is symmetric — fine. Just be aware it constrains what you can say publicly about the engagement.

**Bottom line.** The structure is reasonable for a closeout, and §11 preserves the Addendum (including the open-source carve-out). The two items I'd want resolved before signing: (a) the $5k settlement amount versus actual outstanding fees, and (b) explicit reaffirmation of the §2(b) open-source carve-out so Section 3's broad assignment can't be re-read against it later.

### Re: Engagement-period projects addendum — opus-4-7

Drafted at [`tasks/dreamable/addendum-engagement-period-projects.md`](addendum-engagement-period-projects.md). It supplements the original Addendum's Exhibit A by enumerating what was created during the April 6 → May 26 engagement window, and points to Addendum §2(b) and Settlement §4 as the controlling carve-outs.

**Public repositories (github.com/btakita), created after April 6, 2026:**

| Repository | Created | License | Description |
|---|---|---|---|
| [tsift](https://github.com/btakita/tsift) | 2026-04-17 | MIT | Token-efficient CLI plugin for Claude Code — Sift-powered search + native tool routing |
| [homebrew-tap](https://github.com/btakita/homebrew-tap) | 2026-05-05 | tap metadata (no proprietary content) | Homebrew tap for your CLI tools |

**Private repositories:** the document includes a single summary line covering the 3 private repos created during the window (`session-share`, `boost-client`, `monsterrodholders-dev` — described categorically, not enumerated, consistent with the original Exhibit A's blanket clause for private repos).

**Suggested use.** Two options:

1. **Attach to the Settlement** as "Exhibit A-1" before signing. Email it to Rob with a one-line note: "Adding a supplement to the Addendum's Exhibit A to cover work created during the engagement window. Identical legal effect under Addendum §2(b)."
2. **Keep on file** as your own contemporaneous record if you'd rather not re-open the document with Rob. The Addendum's §2(b) carve-out already protects the public OSS projects categorically; this just removes ambiguity if it's ever questioned.

I'd lean toward option 1 — five minutes of friction now prevents an IP-claim gray area later, and it's consistent with how the original Addendum framed Prior Inventions.
<!-- agent:boundary:0c2c947b -->
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
