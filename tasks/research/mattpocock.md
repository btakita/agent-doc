---
agent_doc_session: 08bbfea7-a9ae-4ef6-b5e4-328df85bc927
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Read the CC transcript for https://www.youtube.com/watch?v=EJyuu6zlQCg
❯ Research his skills in https://www.aihero.dev/5-agent-skills-i-use-every-day

❯ Go into the source of every skill.
❯ How can we add these skills within the context of agent-doc?
### Re: Matt Pocock's 5 Agent Skills

**Video:** "5 Claude Code skills I use every single day"
Timestamps: `/grill-me` (1:18) · `/write-a-prd` (3:55) · `/prd-to-issues` (6:00) · `/tdd` (8:29) · `/improve-codebase-architecture` (12:04)

CC transcript unavailable via web fetch (YouTube serves JS-only). Key info captured from description + skill sources.

---

## Skill Sources

All from https://github.com/mattpocock/skills/

### 1. `/grill-me`

> "Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer. Ask questions one at a time. If a question can be answered by exploring the codebase, explore the codebase instead."

Minimal skill. Pure interview loop. No output format prescribed.

---

### 2. `/write-a-prd`

Five steps:
1. Get a long description of the problem + potential solutions
2. Explore repo to verify assertions
3. Interview relentlessly (decision tree)
4. Sketch major modules — emphasizes **deep modules** (large impl, small stable interface)
5. Write PRD in structured template → submit as GitHub issue

PRD template sections: Problem Statement · Solution · User Stories · Implementation Decisions · Testing Decisions · Out of Scope · Further Notes

---

### 3. `/prd-to-issues`

Tracer-bullet vertical slices — each issue cuts through ALL layers end-to-end (schema + API + UI + tests), not horizontal slices of one layer.

Classifies slices as **HITL** (needs human) or **AFK** (can merge without human). Creates GitHub issues in dependency order.

---

### 4. `/tdd`

Key distinctions:
- **Anti-pattern: horizontal slicing** — "write all tests first, then all implementation" produces tests of imagined behavior that test shape (data structures, signatures) not behavior
- **Correct: vertical slices** — one test → one implementation → repeat. Each test responds to what you learned from the previous cycle
- Tests verify behavior through public interfaces only — survive internal refactors
- References external docs: `tests.md`, `mocking.md`, `deep-modules.md`, `interface-design.md`, `refactoring.md`

---

### 5. `/improve-codebase-architecture`

Explores codebase organically (no rigid heuristics). Friction signals:
- Concepts that require bouncing between many small files (shallow modules)
- Interface nearly as complex as implementation
- Pure functions extracted for testability but real bugs hide in how they're called
- Tightly-coupled modules with integration risk in seams

Then spawns 3+ parallel sub-agents to design radically different interfaces:
- Agent 1: minimize interface (1–3 entry points)
- Agent 2: maximize flexibility
- Agent 3: optimize for most common caller
- Agent 4: ports & adapters pattern

Outputs recommendation + GitHub RFC issue.

---

## Comparison to Existing agent-loop Skills

| Matt's skill | Our equivalent | Delta |
|---|---|---|
| `/grill-me` | `superpowers:brainstorming` | Grill-me is tighter — pure decision-tree interview, no output format; brainstorming is broader |
| `/write-a-prd` | `superpowers:writing-plans` | PRD outputs GitHub issue; writing-plans writes `plan.md` locally |
| `/prd-to-issues` | `superpowers:executing-plans` + `dispatching-parallel-agents` | His focuses on GitHub issue creation specifically |
| `/tdd` | `superpowers:test-driven-development` | Similar; his has crisper anti-pattern articulation (horizontal vs vertical slicing) |
| `/improve-codebase-architecture` | No direct equivalent | Most distinctive — parallel sub-agents for interface design |

---

## Integration Opportunities with agent-doc

The key structural difference: Matt's skills are **stateless per session** — each invocation is a fresh context. Agent-doc provides **multi-session persistence via document+git history**. This changes what's possible.

### 1. Grill-me in exchange components

Natural fit: user writes a proposal in `&lt;!-- agent:input --&gt;`, then invokes `/grill-me` as a slash command. The interview happens in `&lt;!-- agent:exchange --&gt;` across multiple sessions. When grilling completes, agent writes synthesized understanding to an `&lt;!-- agent:output --&gt;` or `&lt;!-- agent:summary --&gt;` component.

Current agent-doc: slash commands in exchange are already handled via `builtin_commands`/`slash_commands` from preflight. A `/grill-me` slash command within the exchange would be detected and dispatched to the grill-me skill, passing the document content as context.

**Gap:** The grill-me skill needs the document content as context when invoked from within agent-doc. Right now skills invoked via `slash_commands` from preflight don't automatically receive the session document context — they'd need it passed explicitly.

### 2. PRD template-mode document

A `prd.md` session with template components:
```
&lt;!-- agent:problem --&gt;
&lt;!-- agent:solution --&gt;
&lt;!-- agent:user-stories --&gt;
&lt;!-- agent:implementation-decisions --&gt;
&lt;!-- agent:testing-decisions --&gt;
&lt;!-- agent:out-of-scope --&gt;
```

The write-a-prd workflow populates these incrementally. Persistent across sessions — the interview can span multiple sittings. `agent-doc commit` after each exchange gives full git history of the PRD evolution.

**New idea:** `agent-doc new --template prd` command that scaffolds a pre-structured PRD document.

### 3. TDD session document

```
&lt;!-- agent:interface --&gt;         (public API being designed)
&lt;!-- agent:test-plan --&gt;         (behaviors to test, approved by user)  
&lt;!-- agent:current-cycle --&gt;     (RED: test written; GREEN: impl; REFACTOR: candidates)
&lt;!-- agent:completed-cycles --&gt;  (append mode — accumulates passing test+impl pairs)
```

The vertical slice approach maps well to agent-doc cycles: each `/agent-doc` invocation IS a RED→GREEN cycle. The document captures the state machine.

**New idea:** `current-cycle` component with `patch=replace` that shows the live RED/GREEN/REFACTOR state, plus `completed-cycles` with `patch=append` as the accumulating log.

### 4. Architecture review document

```
&lt;!-- agent:candidates --&gt;        (architectural friction points found)
&lt;!-- agent:selected --&gt;          (which candidate the user picked)
&lt;!-- agent:interface-designs --&gt; (3+ parallel sub-agent outputs)
&lt;!-- agent:recommendation --&gt;    (agent's synthesis)
&lt;!-- agent:rfc --&gt;               (GitHub issue body, ready to create)
```

The parallel sub-agent interface design step is already how `superpowers:dispatching-parallel-agents` works — agent-doc could capture these in the document so the user can compare designs across sessions.

### 5. Adopt "horizontal vs vertical slicing" anti-pattern framing

Matt's TDD section has a crisper articulation of the horizontal/vertical slicing anti-pattern than our `superpowers:test-driven-development`. Candidate for importing into our TDD skill.

### 6. Deep modules concept for agent-doc itself

The `improve-codebase-architecture` skill is about finding shallow modules in a codebase. Candidate: apply this analysis to agent-doc's own architecture — specifically the exchange handling and route.rs single-exit-point area (which has already caused bugs per memory).

---

## Suggested Actions

1. **Create `/grill-me` skill** — adapt from source, add context injection for agent-doc sessions
2. **Create PRD template** — `agent-doc new --template prd` or a starter document with components
3. **Improve `superpowers:test-driven-development`** — import horizontal/vertical slicing framing
4. **Create `improve-codebase-architecture` equivalent** — currently no direct analog; highest unique value
5. **Consider `agent-doc new --template <name>`** — scaffolding command for structured session types

Should I fork the mattpocock/skills repo?
### Re: Fork the mattpocock/skills repo?

No. His skills are 5–20 line stubs — the moment you add agent-doc context injection, superpowers frontmatter, and your workflow conventions, you diverge immediately. The fork relationship creates overhead without tracking value.

Better: use his repo as a reference bookmark. Read source when needed, adapt ideas directly into agent-loop's skill system. The reference is already in this document.
### Re: Fork the mattpocock/skills repo?

No. His skills are 5–20 line stubs — the moment you add agent-doc context injection, superpowers frontmatter, and your workflow conventions, you diverge immediately. The fork relationship creates overhead without tracking value.

Better: use his repo as a reference bookmark. Read source when needed, adapt ideas directly into agent-loop's skill system. The reference is already in this document.
<!-- agent:boundary:c25deeed -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
