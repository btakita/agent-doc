---
agent_doc_session: fa2a5de1-dafd-421c-a264-844c0008e68b
tmux_session: claude
---


Help me draft a reply to https://news.ycombinator.com/reply?id=47224082&goto=threads%3Fid%3Dbriantakita%2347224082
> The "don't sync, own" model makes a lot of sense. We were thinking about it wrong - trying to push state out to a shared file, when the cleaner move is to pull it in on demand.
> The SPEC.md as the extraction target after a feature is done is a nice touch. In our case the tricky part is that browser automation state is partly external - you have sessions, cookies, proxy assignments that live outside the codebase. So the "ground truth" we needed wasn't just about code decisions but about runtime state too. Ended up logging that separately.
>
> Checking out agent-doc, the snapshot-based diff to avoid re-triggering on comments is clever. Does it handle cases where two agents edit the same doc around the same time, or is the ownership model strict enough that this doesn't come up?

So far I have:

> Does it handle cases where two agents edit the same doc around the same time, or is the ownership model strict enough that this doesn't come up?

A document is owned by one session...but one session can own many docs. It is possible to have a race condition where one agent claim a document, starts doing work, then another agent claims the document. Or I'm writing additional prompts while the agent is running. Worst case is the last writer could clobber the output. But I'm also working on that race condition.

## Assistant

Here's a draft reply:

---

The runtime state logging approach makes sense for browser automation — that's a domain where ground truth literally lives outside your repo. We have a similar dynamic with email state in our corky tool (IMAP sync, draft queues). Ended up with the same pattern: log the external state separately and let the document reference it rather than trying to capture it inline.

On concurrent editing — the ownership model is strict by default. Each document is claimed by one tmux pane (one agent session). The `agent-doc claim` command registers ownership, and the snapshot diff only fires for the owning session.

That said, it's not bulletproof yet. Two race conditions exist:

1. Agent A claims a doc, starts working, then Agent B claims the same doc (clobbering the claim)
2. I'm writing additional prompts while the agent is mid-response — last writer wins on the file

Both are "last writer wins" scenarios. In practice I avoid (1) through the routing layer (tmux pane → document mapping), and (2) hasn't bitten me badly because the Edit tool does surgical appends rather than full rewrites. But I'm actively working on proper locking to close both gaps.

The design philosophy is closer to "ownership prevents conflicts" than "handle conflicts gracefully." One session owns many docs, but each doc has exactly one owner at a time. Similar to how you'd avoid two people editing the same Google Doc section simultaneously — except enforced by tooling rather than social convention.

---

A few notes on the draft:

- **Opened with empathy** on their runtime state challenge, connected it to your corky experience (shows you've hit the same problem in a different domain)
- **Honest about limitations** — the race conditions are real, you're working on them
- **Framed the design intent** ("ownership prevents conflicts" vs "handle conflicts gracefully") which answers the spirit of their question
- **Kept it HN-appropriate length** — substantive but not a blog post

Feel free to edit the tone, cut sections, or adjust the corky reference if you'd rather not mention it publicly.

## User

The last writer wins scenario has been mitigated using git commits and diffs. Along with logic. Please verify by checking the logic in agent-doc.

## Assistant

You're right — I overstated the race condition. After checking the agent-doc source (`submit.rs`, `git.rs`), here's what actually happens:

**agent-doc uses a 3-way merge strategy, not "last writer wins":**

1. **Pre-submit git commit** — before sending to the agent, the file is committed to git (immutable baseline)
2. **Race detection** — after the agent responds, the file is re-read and compared to the pre-submit version. If the user edited during the response, it triggers a merge
3. **`git merge-file --diff3`** — merges the baseline, agent response, and user's concurrent edits. Non-overlapping changes merge cleanly; overlapping changes get conflict markers

So the concurrent editing question has a solid answer: within a single session, user edits during agent response are preserved via 3-way merge. Nothing is silently dropped.

The remaining edge case is claim clobbering (Agent A owns a doc, Agent B re-claims it). That's prevented by the routing layer — tmux pane-to-document mapping ensures each doc has exactly one owner.

Here's the **revised draft reply**:

---

The runtime state logging approach makes sense for browser automation — that's a domain where ground truth literally lives outside your repo. We have a similar dynamic with email state in corky (IMAP sync, draft queues). Same pattern: log the external state separately and let the document reference it.

On concurrent editing — it's handled at two levels:

**Ownership:** Each document is claimed by one tmux pane (one agent session). The routing layer prevents two agents from working the same doc simultaneously.

**3-way merge:** If I edit the document while the agent is mid-response, agent-doc detects the change on write-back and runs `git merge-file --diff3` — baseline (pre-commit), agent response, and my concurrent edits all merge. Non-overlapping changes merge cleanly; overlapping changes get conflict markers. Nothing is silently dropped.

The pre-submit git commit is the key — it creates an immutable baseline before the agent touches anything, so there's always a clean reference point for the merge.

---

## User

Is that the full reply?

## Assistant

Yes — that's the full reply as drafted (the section between the `---` markers). It covers:

1. **Empathy on runtime state** — connects their browser automation challenge to your corky experience
2. **Ownership model** — one tmux pane, one doc
3. **3-way merge** — the technical answer to their concurrent editing question

Want me to adjust length, tone, or add/remove sections?

## User

Would a CRDT help with concurrent editing?

See https://jakelazaroff.com/words/an-interactive-intro-to-crdts/

## Assistant

CRDTs solve a superset of agent-doc's problem at much higher complexity. Here's the tradeoff:

**Why the git 3-way merge is the right fit now:**
- agent-doc has at most 2 writers (agent appending at bottom, user editing anywhere)
- They almost always edit different parts of the file — clean merges are the common case
- When edits overlap, the conflict IS meaningful — you want to see both versions, not have them silently merged
- Plain markdown files on disk, zero new dependencies

**What CRDTs would add:**
- Character-level merge granularity — never produces conflict markers
- Designed for N concurrent writers with no central authority
- But requires converting markdown to a CRDT-backed store (no longer plain files on disk)
- Silently merges overlapping edits that might actually conflict semantically
- Significant complexity for a rare edge case in the current architecture

**When CRDTs would become worth it:** If agent-doc evolved toward real-time collaborative editing — multiple agents + user all editing the same doc simultaneously. The "one owner per doc" model makes that unnecessary today.

The Jake Lazaroff article covers state-based CRDTs (Last Write Wins Register/Map) — good primitives, but text editing requires more specialized CRDTs (e.g., RGA, YATA from Yjs, or diamond-types in Rust). That's a whole layer of complexity on top.

**For the HN reply:** I wouldn't mention CRDTs unless CloakHQ brings it up. The 3-way merge answer is more honest about where agent-doc actually is, and "we use git merge-file" is more relatable than "we're building a CRDT."

## User

CloakHQ responded https://news.ycombinator.com/item?id=47244988

Please help me craft a response.

> One thing we haven't solved cleanly: what happens when the external state change is what triggers the agent task in the first place? Like, a session getting flagged and needing recovery. The event comes from outside the doc system, so there's a gap between "thing happened" and "agent knows about it".

Does agent-doc have a solution for this? What can we do to address this concern?
Should we consider having watcher scripts?
 
## Assistant

CloakHQ's question is about the **event → agent bridge** — when something external happens, how does the agent get notified?

**What we already have:**
- `corky watch` is exactly a watcher script — polls IMAP for new emails, syncs to markdown files on disk. External event → file-system change → agent's native input format.
- agent-doc is user-initiated (edit doc, submit), but the file-system diff pipeline is the foundation.

**On watcher scripts:** Yes — that's the right pattern. The bridge between "thing happened" and "agent knows about it" is a process that listens for external events and writes them into the file system where agents operate. `corky watch` already proves this works.

**Draft reply:**

---

Good question — this is the "event → agent" bridge problem.

agent-doc is currently user-initiated (edit a doc, hit submit). But we've solved the same pattern in a different layer: corky (our email tool) runs a watch daemon that polls IMAP for new messages and syncs them to markdown files on disk. External event gets translated into a file-system change, which is the agent's native input format.

The general pattern: **event → write to file system → agent picks up the change.** The bridge is a watcher process that listens for external events and writes them into the doc/file system where agents operate.

For your session recovery case: monitoring process detects flagged session → writes a task file or appends to a doc → agent picks it up on next poll. The gap is really about who runs the monitoring process and how quickly the agent needs to react.

Real-time reactivity (sub-second) would need event-driven agent dispatch. But for most operational tasks, a polling bridge with a reasonable interval (corky watch runs every 30 seconds) closes the gap well enough.

---

## User

What if we have an "operations dashboard" as a document? Using a document as a template to fill in fields. How could that look?

Also allowing for the diff handling for agent loop inference.

We could provide a file-based notification system. We could also have a general server or cli tool that realtime event system can interact with low latency.

## Assistant

Three interconnected ideas — here's how they fit together:

**Operations Dashboard as a Document:**

A markdown template where a watcher fills in fields; the agent responds to diffs:

```markdown
## Sessions
| ID | Status | Last Check | Action |
|----|--------|------------|--------|
| ses-001 | active | 2m ago | — |
| ses-002 | ⚠ flagged | 30s ago | needs recovery |

## Alerts
- [NEW] ses-002 flagged at 08:15 — proxy rotation failed
```

Watcher updates "active" → "flagged". Agent-doc sees the diff: `ses-002` status changed. The diff IS the trigger — the agent doesn't scan the whole doc, just responds to what changed. Template fields give structure; diffs give semantics.

**File-based notification system:**

Append-only notifications file. External events append structured entries:
```
2026-03-04T08:15:00 session.flagged ses-002 proxy_rotation_failed
2026-03-04T08:16:30 session.recovered ses-002 auto_retry_succeeded
```

Agent polls or watches the file. Each line is a discrete event. Snapshot diff shows only new events since last read.

**Real-time server/CLI:**

For sub-second latency: a lightweight daemon that receives webhooks/events and either:
1. Writes to the notification file (bridges into the file-based system)
2. Directly triggers an `agent-doc submit` via CLI
3. Pushes to a Unix socket that agent-doc listens on

The layering: **real-time server → file write → agent-doc diff → agent inference**. Each layer is optional — start with file writes, add the server when latency matters.

**Draft addition to the HN reply:**

---

We're thinking about this as a layered problem:

**Dashboard-as-document:** A markdown template where a watcher process fills in operational fields (session status, health checks). When a field changes, the agent-doc diff pipeline sees exactly what changed — "session went from active to flagged" — and the agent responds to that diff. The template gives structure; the diff gives the trigger.

**Event bridge:** External events append to a notification file. The agent sees new entries on each poll via snapshot diff. For sub-second latency, a lightweight daemon receives webhooks and either writes to the file or directly triggers an agent submit.

The gap you're describing — "thing happened" to "agent knows about it" — is really about who writes the event into the file system. Once it's a file change, the diff pipeline handles the rest.

---

## User

Could you write the entire reply as a draft?

## Assistant

**Complete draft reply to CloakHQ:**

---

The pre-submit commit as an immutable baseline is the key design decision — every agent interaction is an atomic transaction with a known rollback point, exactly right.

On the "event → agent" gap: we've been thinking about this as a layered problem.

**The bridge is a file write.** agent-doc is user-initiated (edit a doc, hit submit), but the diff pipeline doesn't care who writes the file. A watcher process — or any external system — can write to the file system, and the agent sees the change on next poll.

We already do this in a different layer: corky (our email tool) runs a watch daemon that polls IMAP for new messages and syncs them to markdown files on disk. External event (email arrives) gets translated into a file-system change, which is the agent's native input format. Same pattern would work for session recovery: monitoring process detects flagged session → writes to a task file → agent picks it up.

**Dashboard-as-document** is the more structured version of this. A markdown template where a watcher fills in operational fields:

```
| Session | Status  | Last Check | Action         |
|---------|---------|------------|----------------|
| ses-001 | active  | 2m ago     | —              |
| ses-002 | flagged | 30s ago    | needs recovery |
```

When the watcher updates "active" → "flagged", the diff pipeline sees exactly what changed. The template gives structure; the diff gives the trigger. The agent doesn't scan the whole doc — it responds to the delta.

For sub-second latency, a lightweight daemon that receives webhooks and either writes to the notification file or directly triggers an agent submit. But for most operational tasks, a polling interval (ours runs every 30 seconds) closes the gap well enough.

The gap you're describing — "thing happened" to "agent knows about it" — is really about who writes the event into the file system. Once it's a file change, the diff pipeline handles the rest.

---

## User

Can agent-doc use a non-fs backend like a database (Sqlite) or service? Should we consider abstracting the interface to allow for such usages?

Also, in the draft, should we provide a link to corky?

## Assistant

**Non-FS backend (SQLite, service)?**

Not yet — but the question is valid. agent-doc's value is that documents are plain files: editable in any editor, diffable with git, zero infrastructure. The FS *is* the abstraction — any tool that can read/write files can participate.

A storage abstraction (`DocumentStore { read, write, diff, watch }`) would make sense when: multiple machines, persistent event queues, or structured queries across documents. Today the single-machine tmux workflow doesn't need it. Adding it now is YAGNI complexity.

SQLite specifically would give atomic writes and structured queries, but you lose the "edit in vim/VSCode/IntelliJ" story — which is agent-doc's core UX.

**Link to corky?**

Yes — adds credibility without being a plug. One inline link:

> We already do this in a different layer: [corky](https://github.com/btakita/corky) (our email tool) runs a watch daemon...

**Updated draft with the corky link:**

---

The pre-submit commit as an immutable baseline is the key design decision — every agent interaction is an atomic transaction with a known rollback point, exactly right.

On the "event → agent" gap: we've been thinking about this as a layered problem.

**The bridge is a file write.** agent-doc is user-initiated (edit a doc, hit submit), but the diff pipeline doesn't care who writes the file. A watcher process — or any external system — can write to the file system, and the agent sees the change on next poll.

We already do this in a different layer: [corky](https://github.com/btakita/corky) (our email tool) runs a watch daemon that polls IMAP for new messages and syncs them to markdown files on disk. External event (email arrives) gets translated into a file-system change, which is the agent's native input format. Same pattern would work for session recovery: monitoring process detects flagged session → writes to a task file → agent picks it up.

**Dashboard-as-document** is the more structured version of this. A markdown template where a watcher fills in operational fields:

```
| Session | Status  | Last Check | Action         |
|---------|---------|------------|----------------|
| ses-001 | active  | 2m ago     | —              |
| ses-002 | flagged | 30s ago    | needs recovery |
```

When the watcher updates "active" → "flagged", the diff pipeline sees exactly what changed. The template gives structure; the diff gives the trigger. The agent doesn't scan the whole doc — it responds to the delta.

For sub-second latency, a lightweight daemon that receives webhooks and either writes to the notification file or directly triggers an agent submit. But for most operational tasks, a polling interval (ours runs every 30 seconds) closes the gap well enough.

The gap you're describing — "thing happened" to "agent knows about it" — is really about who writes the event into the file system. Once it's a file change, the diff pipeline handles the rest.

---

## User

This is hacker news. Here are the [formatting options](https://news.ycombinator.com/formatdoc).

## Assistant

Good catch — HN only supports *italics*, code blocks (2-space indent), and bare URLs. No bold, tables, or markdown links. Here's the reformatted draft:

**Copy-paste ready for HN:**

```
The pre-submit commit as an immutable baseline is the key design decision — every agent interaction is an atomic transaction with a known rollback point, exactly right.

On the "event → agent" gap: we've been thinking about this as a layered problem.

*The bridge is a file write.* agent-doc is user-initiated (edit a doc, hit submit), but the diff pipeline doesn't care who writes the file. A watcher process — or any external system — can write to the file system, and the agent sees the change on next poll.

We already do this in a different layer: corky (https://github.com/btakita/corky), our email tool, runs a watch daemon that polls IMAP for new messages and syncs them to markdown files on disk. External event (email arrives) gets translated into a file-system change, which is the agent's native input format. Same pattern would work for session recovery: monitoring process detects flagged session → writes to a task file → agent picks it up.

*Dashboard-as-document* is the more structured version. A markdown template where a watcher fills in operational fields — session status, health checks, proxy assignments. When the watcher updates "active" → "flagged", the diff pipeline sees exactly what changed. The template gives structure; the diff gives the trigger. The agent doesn't scan the whole doc — it responds to the delta.

For sub-second latency, a lightweight daemon that receives webhooks and writes to the notification file or directly triggers an agent submit. But for most operational tasks, a polling interval (ours runs every 30 seconds) closes the gap well enough.

The gap you're describing — "thing happened" to "agent knows about it" — is really about who writes the event into the file system. Once it's a file change, the diff pipeline handles the rest.
```

## User

Can agent-doc support dashboard-as-document right now? Would we need to add features to better support that use case?

## Assistant

**Partially — the diff pipeline works, but the interaction model has gaps.**

**What works today:**
- External process can modify the markdown file (watcher updates fields)
- Snapshot-based diff detects exactly what changed
- 3-way merge protects concurrent edits
- `agent-doc route <file>` can trigger submission from external scripts

**What's missing:**

1. **Auto-submit on file change** — agent-doc is user-initiated only. No `agent-doc watch`. You'd need to manually run `/agent-doc` or use `agent-doc route` from a cron/watch script.

2. **Sectional editing** — the agent can only *append* `## Assistant` blocks at the bottom. A dashboard needs the agent to *update specific sections* (e.g., fill in an "Actions" column). Today the agent can't write to arbitrary doc locations.

3. **Template field awareness** — diffs are raw text, not structured fields. "active → flagged" shows as a text diff, not a typed field change.

**What would need to be built:**

| Feature | Effort | Impact |
|---------|--------|--------|
| `agent-doc watch <file>` — auto-submit on file change | Small | Closes the trigger gap |
| Sectional write-back — agent edits specific sections | Medium | Enables dashboard pattern |
| Template syntax — `{{field}}` interpolation | Medium | Structured field awareness |

**Minimum viable dashboard today:** A watch script that runs `agent-doc route <file>` when the file changes (sends the submit keystroke to the owning pane). The agent response appends at the bottom — for an operational dashboard, the latest status is always the last section. Not ideal, but functional.

## User

In the reply, let's mention that the dashboard-as-document is planned. With partial support today.

## Assistant

**Updated draft — copy-paste ready for HN:**

```
The pre-submit commit as an immutable baseline is the key design decision — every agent interaction is an atomic transaction with a known rollback point, exactly right.

On the "event → agent" gap: we've been thinking about this as a layered problem.

*The bridge is a file write.* agent-doc is user-initiated (edit a doc, hit submit), but the diff pipeline doesn't care who writes the file. A watcher process — or any external system — can write to the file system, and the agent sees the change on next poll.

We already do this in a different layer: corky (https://github.com/btakita/corky), our email tool, runs a watch daemon that polls IMAP for new messages and syncs them to markdown files on disk. External event (email arrives) gets translated into a file-system change, which is the agent's native input format. Same pattern would work for session recovery: monitoring process detects flagged session → writes to a task file → agent picks it up.

*Dashboard-as-document* is something we're actively planning. The idea: a markdown template where a watcher fills in operational fields — session status, health checks, proxy assignments. When the watcher updates "active" → "flagged", the diff pipeline sees exactly what changed. The template gives structure; the diff gives the trigger.

The snapshot diff already works for this — partial support exists today via agent-doc's routing layer (external scripts can trigger submission when a file changes). What we're building next is auto-submit on file change and sectional write-back so the agent can update specific dashboard fields rather than only appending.

For sub-second latency, a lightweight daemon that receives webhooks and writes to the notification file or directly triggers an agent submit. But for most operational tasks, a polling interval (ours runs every 30 seconds) closes the gap well enough.

The gap you're describing — "thing happened" to "agent knows about it" — is really about who writes the event into the file system. Once it's a file change, the diff pipeline handles the rest.
```

Changes from previous draft:
- Dashboard-as-document framed as "actively planning" with "partial support today"
- Added concrete detail: "routing layer (external scripts can trigger submission)"
- Added roadmap signal: "auto-submit on file change and sectional write-back"

## User
