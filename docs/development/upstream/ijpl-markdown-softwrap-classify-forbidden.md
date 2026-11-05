# IJPL report: Markdown soft wrap moves wraps to an already-closed `[`

Status: **filed 2026-11-05 as [IJPL-260670](https://youtrack.jetbrains.com/issue/IJPL-260670)**
(IntelliJ Platform, Type Bug, Subsystem Lang. Markdown, affected 2026.3 Beta (263.7307.44)).
The text below is what was filed; check the issue for its current state before retiring the
agent-doc workarounds (`MarkdownBracketWrapStrategy`, GH #391, and the `<url>` rule in
`agent:active` summaries).

Attached to the issue:

- [`ijpl-markdown-softwrap/repro.md`](ijpl-markdown-softwrap/repro.md): the reproduction file.
- [`ijpl-markdown-softwrap/WrapSim.java`](ijpl-markdown-softwrap/WrapSim.java): the soft-wrap
  simulator that produced the visual lines below (optional).

---

## Title

Markdown soft wrap: `MarkdownLineWrapPositionStrategy.classifyForbidden` moves the wrap to an
already-closed `[`, stranding list markers and splitting bare URLs mid-word

## Environment

- IntelliJ IDEA 2026.3, build **263.7307.44**, bundled Markdown plugin
  (`org.intellij.plugins.markdown.editor.MarkdownLineWrapPositionStrategy`, which has
  `classifyForbidden` / `hasClosingParen`).
- Linux x86_64, monospace editor font, soft wraps on for `*.md`.
- Not present in 2024.2 (242.20224.300): that strategy has no `classifyForbidden`, and these
  lines wrap normally there. Our plugin notes (GH #391) also attribute the token-scan guard to
  later 2026.2 (262) builds but not 262 GA; that was not re-checked for this report.

## Steps to reproduce

1. Settings | Editor | General | Soft Wraps: enable *Soft-wrap these files* for `*.md`.
2. Open the attached `repro.md` (contents below) and narrow the editor until a visual line holds
   about 80 columns (or about 100 columns).
3. Look at how lines 1 to 3 wrap.

```markdown
- `[#q-1]` — Since we changed direction to https://git.example.com/group/project/-/merge_requests/1234
- [ ] [#bl-1] see the notes in the tracker before you start, then https://git.example.com/group/project/-/issues/42
- See [the notes](#notes) before you start working on the item, then https://git.example.com/group/project/-/issues/42
- See (the notes) before you start working on this item today, then https://git.example.com/group/project/-/issues/42 (ok)
- `[#q-1]` — Since we changed direction to <https://git.example.com/group/project/-/merge_requests/1234>
- Since we changed direction to the plan, see https://git.example.com/group/project/-/merge_requests/1234
```

Lines 4 to 6 are controls: no `[` before the URL, or the URL written as `<url>`. They wrap
correctly.

## Expected

Each line wraps once, at the space in front of the URL, the same way control line 6 does:

```text
|- `[#q-1]` — Since we changed direction to |
|https://git.example.com/group/project/-/merge_requests/1234|

|- [ ] [#bl-1] see the notes in the tracker before you start, then |
|https://git.example.com/group/project/-/issues/42|

|- See [the notes](#notes) before you start working on the item, then |
|https://git.example.com/group/project/-/issues/42|
```

## Actual (80 columns)

The list marker is left alone on the first visual line, the next visual line carries one
token, and the URL is then split mid-word:

```text
|- |
|`|
|[#q-1]` — Since we changed direction to https://git.example.com/group/project/|
|-/merge_requests/1234|

|- [ |
|] |
|[#bl-1] see the notes in the tracker before you start, then https://git.exampl|
|e.com/group/project/-/issues/42|

|- |
|See |
|[the notes](#notes) before you start working on the item, then https://git.exa|
|mple.com/group/project/-/issues/42|
```

At 100 columns, line 1 becomes `|- |` `` |`| `` and then the rest, and lines 2 and 3 again break
inside the URL (`…/group/project/|-/issues/42`, `…/group/proje|ct/-/issues/42`).

The same thing happens without a URL whenever the wrap lands on a `(` or `)` and any `[` comes
earlier on the line, for example `  - [#id] Run the smoke (procedure in the runbook): install
(a), register (b), …`. The first wrap goes in front of `[#id]` or inside the indent, and every
continuation visual line gets `NEVER_WRAP`.

## Analysis

`MarkdownLineWrapPositionStrategy.calculateWrapPosition` asks `classifyForbidden` whether the
generic whitespace wrap position is inside a link. Four things go wrong:

1. **The walk back takes the nearest `[` on the line without checking that it is still open.**
   `classifyForbidden` walks back from `position` to the line start and records the first
   `LBRACKET` it meets as `linkStart`. It never checks whether a `]` closed that bracket before
   `position`, whether the `]` is followed by `(` or `[`, or whether the bracket is inside a
   code span. In `` `[#q-1]` `` (a code span), in a task-list `[ ]`, or in a closed
   `[the notes](#notes)`, that `[` cannot own a later URL or parenthesis, but it becomes
   `linkStart` anyway.
2. **A bare autolink is treated as a link destination.** For `GFM_AUTOLINK` (and `AUTOLINK` /
   `EMAIL_AUTOLINK`) the token is never inside a `[…](…)` destination. `FORBIDDEN_INSIDE_LINK`
   still returns `linkStart` whenever any `[` came earlier on the line, instead of the
   autolink's own start (`iterator.start`). The `LINK_BRACKETS` branch has the same problem:
   `hasClosingParen` only asks whether some `)` comes later on the line, not whether it closes
   a destination that starts right after this `[`'s `]`.
3. **Continuation lines then refuse to wrap at all.** On the next visual line the same `[` lies
   at or before `startOffset`, so `forbiddenStart <= startOffset` returns `-1`. `SoftWrapEngine`
   falls back to the preferred maximum offset, which splits the URL (or any word) mid-token.
4. **The retry can never wrap directly in front of the link (minor).** The retry calls the
   generic strategy with `endOffset = forbiddenStart` and
   `maxPreferredOffset = min(maxPreferredOffset, forbiddenStart)`.
   `GenericLineWrapPositionStrategy` clamps `maxPreferredOffset >= endOffset` to
   `endOffset - 1`, so the space right before the `[` (target `forbiddenStart`) is never
   accepted, and the wrap moves one more word back. That is why line 3 wraps after `- ` and
   then after `See `, not once in front of `[the notes]`.

Suggested direction (a sketch, not a patch):

```kotlin
// While walking back, skip brackets already closed before `position`, and ignore `[` in a code span.
var closed = 0
while (!tokens.atEnd() && tokens.start >= lineStart) {
  when (tokens.tokenType) {
    MarkdownTokenTypes.ATX_HEADER -> return NEVER_WRAP
    MarkdownTokenTypes.RBRACKET -> closed++
    MarkdownTokenTypes.LBRACKET -> if (closed > 0) closed-- else if (!inCodeSpan(tokens)) { linkStart = tokens.start; break }
  }
  if (tokens.start == 0) break
  tokens.retreat()
}
// An autolink is never inside a destination: wrap in front of the autolink itself.
if (type in AUTOLINKS) return iterator.start
// For URL / LINK_TITLE / brackets: keep `linkStart` only when its `]` is followed by `(` and the
// matching `)` lies at or after `position`.
```

Two more changes would help. A `forbiddenStart <= startOffset` result on a continuation line
should fall back to the generic position or the end of the forbidden construct, not
`NEVER_WRAP`. And the retry should accept the offset `forbiddenStart` itself.

## Evidence

- **In the IDE.** A remote IntelliJ instance (soft wraps on; exact build not recorded) showed exactly the
  *Actual* shape for line 1: a lone `-`, a lone backtick, then the rest of the row, with the URL
  split near the margin. The screenshot is not attached because it shows a private host name.
  `repro.md` is the same text with the host anonymized.
- **Platform test against the bundled strategy.** A `BasePlatformTestCase` in the agent-doc
  JetBrains plugin
  (`editors/jetbrains-262/frontend/src/test/kotlin/.../MarkdownBracketWrapStrategyPlatformTest.kt`,
  <https://github.com/btakita/agent-doc>) gets the bundled strategy from
  `LanguageLineWrapPositionStrategy.forLanguage(MarkdownLanguage)`. It opens a real editor
  (`LexerEditorHighlighter`) and asks for a wrap at every right margin of
  `  - [#vscodecodersmoke] Run the smoke (procedure in the runbook): install (a), …`. On 263 it
  asserts that the bundled answer lands in `0..indexOf('[')` (the marker is stranded) and that a
  continuation line gets `NEVER_WRAP`.
- **Soft-wrap simulator.** `WrapSim.java` ports `SoftWrapEngine`'s wrap-position search and
  `MarkdownLineWrapPositionStrategy` (`calculateWrapPosition`, `classifyForbidden`,
  `hasClosingParen`) and runs them on the leaf tokens of the IDE's own
  `lib/intellij.libraries.markdown.jar` GFM parser (monospace, no folds or inlays). The
  *Actual* blocks above are its output for `repro.md` at widths 80 and 100. It also reproduces
  the IDE screenshot's shape for line 1.

## Workaround we ship

The agent-doc JetBrains plugin registers a `lang.lineWrapStrategy` for `Markdown` with
`order="first"` (`MarkdownBracketWrapStrategy`, GH #391). It asks the bundled strategy first.
When every `[` before the whitespace wrap closes before it and opens no link whose destination
or reference is still open at the wrap (and the line is not a link reference definition), it uses
the plain whitespace wrap instead. agent-doc also writes bare URLs in
its generated rows as `<url>`, which the bundled strategy wraps correctly. We would like to
retire both once this is fixed.
