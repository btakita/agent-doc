---
plan: response-heading-missing-blank-line
status: backlog
---

## Bug

When a response is appended to `agent:exchange` and the previous response's last line did not end with two newlines, the new `### Re: …` heading lands on the line immediately after the prior paragraph with no blank line between them. Many markdown renderers (and the GitHub diff viewer in particular) then render the heading as plain text inside the previous paragraph, which makes the response look "missing" to the user even though it is in the document.

## Concrete repro

Observed in `/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` at commit `c32e5b4` / boost-client pointer at the same SHA. The previous response ("Direct URLs for manual retrieval") ended its final paragraph without a trailing blank line; the next finalize cycle inserted `### Re: Where the Developer token field is — opus-4-7` directly underneath:

```
…the real value of this stack is for any *future* regression.
### Re: Where the Developer token field is — opus-4-7
```

Result: in the live document, the heading appears as the visible H3 (renderer keeps it functional), but the user reported "Response not written into document" because their renderer / scroll position glued the heading text to the previous paragraph.

## Root cause hypothesis

`agent-doc finalize`'s exchange-append path (likely in the patch-apply / boundary-insertion code) trims trailing whitespace from the previous tail before inserting the new boundary + heading, but does not insert the required blank line between prior content and the new heading. Markdown ATX headings require either a leading blank line or `^` to render as a heading inside many renderers (and to render correctly in the patch-mode "live response" UI).

## Proposed fix (do NOT implement here)

In `agent-doc/src/exchange.rs` (or wherever the template `exchange` append happens — search for `### Re:` heading insertion and the boundary repair path):

1. Before writing the new response block, normalize the prior tail to end with exactly one `\n` (single trailing newline, no trailing blank lines).
2. Insert exactly one literal blank line between the prior tail and the new heading. Total separator therefore becomes `\n\n` between final char of prior content and the `#` of the new heading.
3. Apply the same normalization in the patch-mode `<!-- patch:exchange -->` consumer so that direct heredoc patches don't reintroduce the join.
4. Add a regression test alongside the existing exchange-append tests: feed two `<!-- patch:exchange -->` blocks back-to-back where the first ends without a trailing newline, assert that the rendered exchange contains `…regression.\n\n### Re:` and not `…regression.\n### Re:`.

## Related plans

- [[plan-crdt-merge-prompt-prefix-leaks-into-response-body]]
- [[plan-duplicate-exchange-block-patchback]]
- [[plan-tail-patch-outside-exchange]]

## Acceptance

- The repro shape above (final paragraph without trailing newline + immediate `### Re:` heading) is no longer reachable from `agent-doc finalize` or `agent-doc write --commit`.
- Existing exchange-append + boundary-repair regression suite still passes.
- Renderer check: a GitHub-flavored markdown render of the test exchange shows the second response heading as an H3 with its own paragraph break, not as text continuing the previous paragraph.
