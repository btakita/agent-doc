---
name: feedback-discuss-naming-before-applying
description: "Discuss API/type naming decisions to convergence before editing files; don't apply each candidate as it is proposed"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 5c0004fa-7f81-4b38-93d3-42eeb0b59a44
---

When Brian raises a naming or API-design question, **discuss it to convergence
first**. Do not edit files on each candidate as it comes up.

During the `#lzspecedgeindex` teardown-group work (2026-07-19) I renamed the
same method five times mid-discussion — `leak` → `detach` → `release_nodes` →
`cancel_teardown` → `disarm` — editing `lazily-rs` and `lazily-spec` on each
turn, and also messaged a running subagent about a rename that then changed
again. Brian: "I want to discuss before you unilaterally decide on the name."

**Why:** Applying each candidate turns a discussion into a sequence of faits
accomplis, and it creates real churn — subagent briefs, fixture filenames, test
names, and cross-repo references all get rewritten per candidate. It also
signals the decision is made when it isn't.

**How to apply:** Lay out the candidates with tradeoffs, give a recommendation
and the reasoning, then *wait* for the decision. Ask what he wants to weigh if
it's unclear. Once he picks, apply it everywhere in one pass. Offering
"want me to apply X?" at the end of a message is not sufficient if I have
already been editing on the previous candidates — the pattern is what he is
objecting to, not the individual ask. Related: [[feedback_no_confirmation_iterate]]
(routine iteration needs no confirmation) does **not** extend to naming and
API-shape decisions, which are his call.
