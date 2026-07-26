# Haiven Decline + Bounded Offer — SUPERSEDED (never sent)

Status: **superseded 2026-07-26 — Brian decided to pursue the Haiven role**; the active
letter is `compensation-reply-draft.md`. Kept as history. Original status: draft for
Brian's review. Reshaped per steering: decline the FTE role in good
faith, be open about industry participation + ally intent, and proactively offer the
1-2 week SDK prototype as a very-low-risk option for Craig. Riza is deliberately NOT
mentioned (intro contingent on prototype reception + Riza's opt-in).

Framing decisions (why the letter reads the way it does):
- **"Unpaid" stays private.** Brian is willing to do the prototype for free (interesting
  problem, financial needs met, wants feedback)... but saying so in the letter devalues
  the work and creates awkward obligation dynamics. The letter instead makes the fee
  "nominal if you hand it off"... same near-zero risk for Craig, without pricing the
  work at zero.
- **The good-faith reason is stated plainly**: a founding role deserves more than Brian
  can give while keeping his own distributed-systems commitment. This converts the
  decline from rejection into integrity... and it inoculates against any later
  "why is he available for a prototype but not the job?" confusion: bounded vs.
  open-ended is exactly the distinction the letter draws.
- **Ally framing is explicit** per steering: Brian plans to participate in this
  industry and wants allies in Craig + Luke.
- **Lazily is pitched as method + public evidence, not as a dependency.** Whether
  lazily-the-library belongs in the SDK is a technical decision for later; the letter
  sells the spec + formal + conformance discipline and the 8-binding track record.

---

Subject: Re: [Craig's email] — My decision, and an offer

Hi Craig,

Thank you for the transparency on the funding position, and for how you and Luke have
engaged through this whole process... it genuinely says good things about the company
you're building. And apologies for the slow reply: your email deserved a considered
answer, and I wanted to be certain of mine before giving it to you.

I've made my decision, and I want to give it to you straight rather than negotiate
toward an answer I'd already reached: I'm going to accept the other offer. It's not
about the compensation... I ran the numbers carefully, and I believe you would have
worked with me on the package. What settled it was stepping back from the spreadsheets
and honestly weighing what my day-to-day life would look like at each company. I'm deep
into my own distributed-systems work, and a founding role at Haiven deserves more focus
than I could give while keeping that commitment. I'd rather tell you that now, in good
faith, than have us discover it together in month three. The other seat is a
lighter-weight contract that lets me keep that commitment with a clear conscience.

That said: I believe in what you're building, I plan to stay active in this industry
for the long run, and I'd like to count you and Luke as allies in it. So instead of
just a "no," here's a bounded offer.

**I'll build you an SDK prototype in 1-2 weeks.** Concretely: a working first-engine
integration (I'd suggest Unity) against your API, plus the scaffolding that makes the
rest of the SDK matrix cheap to build... with or without me: an API spec, a conformance
test suite, and docs. This is the method I use for my own cross-language work... one
API surface maintained across 8 languages with spec, formal model, and conformance
suites; the repos are public if you want to kick the tires: the spec lives at
github.com/lazily-hub/lazily-spec, the formal model at
github.com/lazily-hub/lazily-formal, and the bindings are all under
github.com/lazily-hub. I'm currently implementing the C# binding
(github.com/lazily-hub/lazily-cs)... proving the same conformance technique works in
C#, which is exactly what a Unity SDK would be written in. And the patterns already
hold across the divide that matters for your matrix... systems languages (C++, Rust,
Zig) and GC runtimes alike... so the Unreal half is familiar ground, not a new bet.
It's designed for clean
handoff: if the prototype lands and you want me to continue milestone-by-milestone, I'm
available. If you'd rather hand it to a hire or take a different approach, you keep
everything... spec, tests, working code, docs... and we keep the fee nominal. Your
repos, your review, either way. Worst case, Haiven gets a validated head start on the
SDK for close to nothing.

If that's interesting, say the word and we can discuss the shape of the api.
I'll sketch the scope & can begin implementing the prototype this week.
Either way, I'm rooting for the August beta and the October launch, and my door stays open... for
an intro that's useful or a conversation whenever you want one.

With real appreciation,
Brian

---

## Notes (not part of the email)

- **Riza sequencing** (steering 7): intro is contingent on (a) the prototype being
  well-received AND (b) Riza's double opt-in. Riza mentioned on LinkedIn he may need
  work to make ends meet... which makes the eventual ask *kind* rather than
  presumptuous, but do not relay his financial situation to Craig. When the moment
  comes, the pitch to Riza is one message: "Haiven may need SDK/DX help after a
  prototype I built for them; you came to mind... Austin, realtime background,
  analytics-driving-product at PresenceAI. Interested in a conversation?" Pitch to
  Craig: quick learner, great attitude, founding-engineer analytics-to-product
  experience, and local to Austin.
- **If Craig asks "why available for this but not the job":** the letter already
  answers it (bounded vs. open-ended), repeat it plainly.
- **Prototype scope guard**: 1-2 weeks means ONE engine + spec + conformance scaffold +
  sample, not the matrix. Resist scope growth until the milestone conversation.
  The "sample" is a minimal demo scene... join a room, presence indicators, a chat
  panel (a day of uGUI), optionally grafted onto a stock Unity starter template so it
  demos like a game... NOT a reference game with real gameplay/assets/UI.
- Prior negotiation-path drafts (compensation ask) are superseded by this letter;
  `compensation-reply-draft.md` stays in the repo as history only. Full contract
  structure if it continues: `sdk-contract-structure.md`.
