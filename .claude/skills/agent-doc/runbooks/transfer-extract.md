# Transfer & Extract

Move content between agent-doc session documents.

## Transfer (move entire component)

Moves all content from a component in the source document to the same component in the target document. Source component is cleared.

```bash
agent-doc transfer <SOURCE> <TARGET> <COMPONENT>
```

**Example:** Move exchange content from one session to another:
```bash
agent-doc transfer tasks/briantakita.me.md tasks/software/corky.md exchange
```

**What happens:**
1. Reads the component content from the source
2. Clears the source component
3. Appends to the target component with a `*Transfer from <source>*` annotation
4. Saves snapshots for both files atomically

## Extract (move last exchange entry)

Extracts the last `### Re:` entry from the source exchange to the target document.

```bash
agent-doc extract <SOURCE> <TARGET> [--component <NAME>]
```

**Example:** Extract the last response to a new session:
```bash
agent-doc extract tasks/software/agent-doc.md tasks/software/new-feature.md
```

**What happens:**
1. Splits the last `### Re:` block from the source exchange
2. Removes it from the source
3. Appends it to the target's exchange component with a `*Extract from <source>*` annotation
4. Saves snapshots for both files

## When to use

- **Transfer:** When moving an entire topic/task to a different session document (e.g., a task outgrew its original session)
- **Extract:** When splitting off the last discussion point into its own session

## Important

- Both commands write directly to the target document -- no manual copy-paste needed
- Both update snapshots for both files, so the next `/agent-doc` cycle sees a clean baseline
- The source session's skill does NOT need to write to the target -- the binary handles cross-file writes
