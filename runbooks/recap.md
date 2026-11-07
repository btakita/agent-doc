# Recap format

Every turn ends with one recap in the same shape, whichever harness wrote it:
Claude Code, Codex, OpenCode, Grok, or Cursor. This runbook owns that shape, and
every harness copy of it is generated from this one file.

## Where it goes

Write the recap in a `patch:recap` block beside `patch:exchange`, in the same
closeout:

```markdown
<!-- patch:exchange -->
### Re: topic — model · timestamp

The full response.
<!-- /patch:exchange -->
<!-- patch:recap -->
The recap.
<!-- /patch:recap -->
```

The block is this turn's recap. While the turn is in progress, `agent:recap`
shows it beside the previous turn's recap. When the turn commits, the previous
turn's recap is removed, so the completed document keeps only this turn's
recap. Do not repeat earlier recaps or carry their text forward yourself.
Use it only in template documents, the ones whose preflight publishes a
`response_contract`.

The recap is a summary for the operator, not input. Nothing in it starts a turn,
and a `do [#id]` in it is not a queue row. Change work through the queue and
tracked-work commands, never by naming it in the recap.

## Shape

Write plain prose for someone who was not watching the turn. Keep the order
below. Omit any section that has nothing to say, except the first.

1. **Outcome.** The opening paragraph is required. State the outcome and the
   current state: what committed, what is running, and what the reader does not
   need to read.
2. **Main open thing.** One short paragraph on what the main unfinished item is
   waiting on. Say what you checked and what you did not check.
3. **`Queue rows this cycle:`** A list of the rows that changed, grouped by state
   with the existing queue markers, such as `Closed`, `Waiting on you (🙋)`, and
   `Blocked (⛔)`.
4. **`Still can't close:`** A list of what stays open, each with the reason.
5. **`Your decisions, most urgent first:`** A numbered list of what only the
   operator can decide. When you recommend one choice that item names, nest the
   `#recommendationinrow` marker under that item:

   `- **Recommendation:** **<label>** — <reason>`

   The label is that choice, with no `(Recommended)` suffix. The reason states
   current evidence or a tradeoff, not a tautology such as "it is recommended".
   Omit the line when you recommend nothing. Do not write `I recommend …` or
   `My recommendation: …`; that prose is not the marker. The line is the same
   row a queue renders above `**Choices:**`, and it is display only: the recap
   is still not a packet, a choice, or a queue row, and nothing here is
   projected into `**Choices:**` (`#recaprecommendationrow`).

Use the three labels exactly as written, each on its own line and directly above
its list. Use no Markdown headings.

## Example

```markdown
The cycle is committed at abc1234, and no workers are running. Nothing you need
to read changed since the last response.

The release is waiting on one approval. I checked the review state on the main
repository, but not on the mirror.

Queue rows this cycle:
- Closed: the docs row.
- Waiting on you (🙋): the release row.
- Blocked (⛔): [#example-b], behind [#example-a].

Still can't close:
- do [#example-a]: the merge is blocked until the release lands.

Your decisions, most urgent first:
1. Release: ship it or hold it.
   - **Recommendation:** **hold** — the mirror is unchecked.
2. Docs: publish them now or with the release.
```

## What the binary checks

The binary checks only what it can check cheaply and deterministically, and each
format finding is an advisory on stderr. A format nit never fails a closeout:

- The recap opens with a paragraph, not a label, list, quote, or heading.
- It contains no Markdown heading outside a code fence.
- The three labels keep their order, and each one introduces a list.
- The items under `Your decisions, most urgent first:` are numbered.
- A recommendation under one of those items is one nested
  `- **Recommendation:** **<label>** — <reason>` line, and the reason is not a
  tautology. `I recommend` and `My recommendation` there are findings. The check
  does not guess which choice the prose meant, and it does not read the line as
  a `**Choices:**` row.

Everything else in this runbook is your responsibility. An agent-authored
template closeout with no recap, or with a blank one, is refused before capture
and leaves the previous recap in place. Say that in `patch:recap`, or, on
`agent_doc_finalize`, in `recap`. Binary-owned replay of an already captured
response still lands without a recap and prints an advisory.
