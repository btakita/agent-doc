# Multi-Topic Response

When the user's diff contains multiple distinct topics (questions, instructions, approvals), process each separately.

## Detection

Multiple topics are present when the diff contains:
- Multiple lines with different intents (question + instruction + approval)
- A checklist with distinct items
- Mixed content types (approval like "Yes" alongside new questions)

## Response Pattern

1. **Acknowledge each topic** — use a `### Re:` header for each distinct topic
2. **Process in order** — address items top to bottom as they appear in the diff
3. **Don't bundle** — avoid combining multiple items into one paragraph
4. **Checkpoint between topics** — flush partial responses after each `### Re:` section

## Example

User diff:
```
+Yes
+What are the gaps in debounce logic?
+Add tests.
+Update SPEC.md
```

Response structure:
```markdown
### Re: Approval + follow-ups

Proceeding with the approved work.

Regarding debounce gaps: [analysis]

For the remaining items:
- **Add tests** — [approach]
- **Update SPEC.md** — [approach]
```

## When NOT to split

- Single question with supporting context (pasted output + question)
- A checklist that forms one coherent instruction set
- Approval with no additional content ("go", "yes")
