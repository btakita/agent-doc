---
agent_doc_session: f9fe252d-cd9b-4c08-bc66-7f612b1ed391
agent_doc_format: template
agent_doc_write: crdt
tmux_session: '0'
agent: claude
opencode_model: GLM-5.1
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
codex_network_access: enabled
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#corky-bug': Please create a plan for corky to fix this issue. Add to the backlog of agent-loop/tasks/software/corky.md
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-commit': update spec + tests. commit
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ During the Malibu fires, the PCH speed limit was cut to 25 MPH. A friend went 40 MPH in the zone.
❯ She pushed back the trial 1 year and today is the day. Court date is at 2PM PT. Current time is 1:30 PT.

❯ She injured her hip and unable to work. Can she postpone the court date without showing up? What methods can she use to postpone the ticket longer? What is the best method to avoid the ticket? After she postponed, she was sent in the mail the court date.

### Re: Postponing / beating a Malibu PCH fire-zone speeding ticket — opus-4-8

> General information, not legal advice. For a CA infraction with money and a possible failure-to-appear hold on the line, a 10-minute call to a local traffic-ticket attorney is worth it. Frontmatter presets (`#agent-doc-bug`, `#spec-test-commit`, etc.) are now registered, but none were invoked this turn, so this is a plain answer — no repo work.

#### First: it's 1:30, court is at 2:00 — do this in the next 30 minutes

The dangerous move is to simply not show up. A no-show on a noticed trial date is a **Failure to Appear (FTA)** — California adds a **$300 civil assessment** (Penal Code 1214.1) on top of the fine and can put a hold on her license. So she must *actively* do one of these now, not go silent:

1. **Call the courthouse clerk immediately** (the number is on the mailed notice). Say she has a documented medical condition (hip injury, unable to work) and request a **continuance for good cause**. Ask specifically: "Can I request a medical continuance by phone/email, and what do you need from me?" Get the clerk's name.
2. **Have a lawyer appear for her.** For an **infraction**, the defendant does **not** have to appear personally if represented by counsel (Penal Code 977(a)). A traffic attorney can stand in at 2:00 and request the continuance — she never shows up.
3. If she can't reach anyone, **email the court** before 2:00 with the request + any proof of the injury, so there's a timestamped record she tried — this helps get an FTA reversed later.

#### Re: Can she postpone without showing up?

Yes, two clean ways:
- **Good-cause medical continuance** — courts routinely grant these *with documentation* (doctor's note, discharge paperwork, disability/work-restriction letter). The catch: she already used a **1-year postponement**, and a second continuance is discretionary and harder. A documented, recent injury is her strongest argument; "I'd rather not come" is not.
- **Counsel appears for her** — as above, no personal appearance needed for an infraction.

A bare phone "I can't make it" with no proof and one prior continuance may be **denied**, which converts to an FTA — so pair the request with evidence.

#### Re: Methods to postpone the ticket *longer*

- **Continuance for good cause** (medical) — primary lever, but diminishing returns after the first one.
- **Trial by Written Declaration (TWD), Vehicle Code 40902** — this resets the clock and is also the best *avoidance* tool (next section). Requesting one moves the matter off the in-person calendar.
- **Hire an attorney** — they can request scheduling that pushes things out and handle appearances.
- Reality check: she's already 1 year out. Judges track repeat continuances; stacking more raises the odds of denial. Don't over-rotate on delay — the **TWD route below is usually the better play than more postponement.**

#### Re: Best method to *avoid* the ticket

**Trial by Written Declaration (VC 40902) is the classic winner**, for two reasons:
1. The citing officer must also submit a written statement. **Officers frequently don't respond** → case **dismissed** automatically.
2. If she *loses* the TWD, she can demand a **Trial de Novo** (VC 40902(d)) — a brand-new in-person trial, as if the TWD never happened. So it's a near-free first swing.

The trade-off: a TWD usually must be requested **on or before the arraignment/trial date with bail posted**, so if today *is* the trial, she may need a continuance *first* to set one up.

If she goes the **in-person trial** route instead: **if the citing officer doesn't show, the case is typically dismissed.** Showing up can be a win, not a loss.

**Substantive defense on the merits** — the strongest given the facts: the speed limit was a **temporary 25 MPH emergency reduction during the Malibu fires.** Challenge whether that limit was *lawfully established and posted*:
- Was the reduction set by the proper authority (Caltrans/local order) and **adequately signed** where she was cited? An unposted or improperly posted limit isn't enforceable.
- On a road like PCH, was the underlying limit backed by a valid **engineering & traffic survey** (VC 40802 speed-trap rules), or did the emergency order override it? Radar/lidar enforcement on a non-surveyed limit can be inadmissible.
- 40 in a *temporary* 25 is 15 over — but if the 25 sign placement/authority is defective, the baseline limit (often 45 on that PCH stretch) may control, which could mean **no violation or a far lesser one.**

She should **photograph the signage** at the exact citation location (and note dates — temporary fire signs may already be gone, which itself helps her) and request **discovery**: the officer's notes, the emergency speed order, and any survey.

#### Re: "After she postponed, she was sent the court date in the mail"

That mailed notice = she was **properly served** with today's trial date. Legally that's why a silent no-show is risky: the court will treat her as on-notice, so an unexcused absence is a clean FTA. It does **not** weaken her case — it just means today's deadline is real. Convert that pressure into the medical-continuance-or-counsel move above, then pursue TWD / officer-no-show / the signage defense.

**Bottom line / triage order for the next 30 min:** (1) call the clerk or get a lawyer to appear and request a **medical continuance** with proof — do **not** no-show; (2) once she has breathing room, file a **Trial by Written Declaration** and bet on officer non-response; (3) build the **temporary-25-MPH signage/authority** defense as her merits argument; (4) if it reaches in-person trial, **officer no-show = dismissal.**
<!-- agent:boundary:a8d6e12a -->
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
