# Handoff: #queuefreetextsplit — distinct multi-line free-text queue items

Handed off from Claude Code (opus-5.5) to Codex on 2026-10-16. Owning session document: `tasks/agent-doc/agent-doc-bugs.md` (backlog `#queuefreetextsplit` and its queue line; the coordinator closes them). Queue preset: `#spec-test-build-install-commit-push` ("update spec + tests. build + install for local testing. commit + push"). Under coordinator rules, workers do not run install; the coordinator runs one `make install`.

## Operator's question
"Does agent-doc recognize distinct queue items of multi-line free text between unordered lists without the `---` line? How is the next queue item, which is 2 lines of free text, interpreted?"

The operator wrote this `agent:queue` body, meaning **two** items: the question bullet, and a free-text note (a paragraph plus a blockquote):

```
- Does agent-doc recognize distinct queue items ... without the `---` line? How is the next queue item, which is 2 lines of free text, interpreted?
My X11 windows crashed due to tmux not getting cleaned up. agent-doc should be a good steward of tmux-spawn scopes and regularly clean up:
> I found a persistent amplifier: ... (one long line)
- do [#tmuxscopesteward]
```

## Observed today (0.35.512 + 46bc541bd8). The answer is "no" for both layouts.
1. **Without `---`:** preflight's `queue_prompts` returns ONE merged item: the bullet, the X11 paragraph, and the blockquote. Unindented lines after a bullet are treated as lazy continuations of that bullet.
2. **With `---`:** the layout was `- ~~do [#tmuxpanesreversed]...~~` / `---` / `My X11 ...` / `> I found ...`. Here the X11 text was **not a queue item at all**:
   - preflight listed only the bullet;
   - `agent-doc queue claim --item 'My X11 windows crashed...'` failed with "no live queue head matches";
   - nothing warned the operator, even though the diff classifier reported `multi_topic, 2 topics separated by ---`.

   Silently dropping operator-written queue text is a defect.
3. **Related false warning:** `agent-doc session-check` warned that a **claimed** free-text head "may have been silently lost". A claimed head is in flight, not lost, so this guard should honor active claims.

## State
- Worktree: `/home/brian/work/btakita/agent-doc-wt/queuefreetextsplit`
- Branch: `fix/queue-free-text-item-boundaries`, from `origin/main` `46bc541bd8`
- **Nothing done yet.** No commits and no changes.

## Work
1. **Find the code.** Locate the queue item parser that produces `queue_prompts` and queue heads. Then find every consumer that must agree on item boundaries: claim matching, `#ftstrike` echo or strike matching (`strike_answered_free_text_queue_heads`), queue sync and mirror, and the session-check free-text guard. Cite file:line for each.
2. **Proposed rule** (refine it if the spec or code supports a better-grounded one, and explain). Inside `agent:queue`:
   - **Item starts:** a new item starts at a list marker in column 0, or at an unindented non-list paragraph that follows a list item.
   - **Item continues:** lines indented under a bullet (2 or more spaces) stay in that item, as does a blockquote directly continuing a free-text paragraph.
   - **`---`:** a thematic break (`---`, `***` or `___`) separates items and is never part of one. Prose after it is an item.
   - **Nothing dropped:** queue text that cannot be assigned to any item produces a preflight warning that quotes the text.
3. **Check compatibility first.** Search `tasks/**/*.md` and the repo fixtures for bullets wrapped with unindented continuation lines. If that pattern is common, ask the operator about the trade-off instead of guessing.
4. **Update the SPEC** section that defines queue items and free-text heads.
5. **Tests:**
   - both layouts above (exactly two heads; the blockquote inside the X11 item);
   - indented continuation staying attached;
   - thematic-break separation;
   - the no-silent-drop warning;
   - claim and `#ftstrike` matching on a multi-line free-text item;
   - the session-check guard honoring claims.

   Mutation-check the key tests: break each fix, confirm red, then restore from a scratch copy inside the worktree and `touch` it.
6. **Verify, commit, push.** Run targeted tests, then `make check`, capturing `$?`. Commit with `git commit -F -` and push the branch, fast-forward only.

## Rules
- Do not work on tmux scopes; `#tmuxscopesteward` covers that.
- Work only in this worktree.
- Format only touched files with `rustfmt --edition 2024 <file>`.
- Never `make install`, release, or write to the session document.

## Report
Include:
- the answer to the operator's two questions, with parser file:line, covering how the 2-line item is read today and after the fix;
- the rule you implemented and why;
- compatibility findings;
- branch and commit SHA;
- exact test results and mutation results.
