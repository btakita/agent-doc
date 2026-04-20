# Transfer & Extract

Move content between agent-doc session documents.

## Transfer (move entire component)

Moves all content from a component in the source document to the same component in the target document. Source component is cleared.

```bash
agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ID1,ID2]
```

**Example:** Move exchange content from one session to another:
```bash
agent-doc transfer tasks/briantakita.me.md tasks/software/corky.md exchange --bypass-claim
```

**Example:** Move specific pending items by ID:
```bash
agent-doc transfer tasks/software/tsift.md tasks/software/tagpath.md pending --bypass-claim --items "#ast4,#0m97"
```

**`--bypass-claim`:** Required when the target document is owned by a different tmux pane. Without it, transfer checks pane ownership and refuses to write to another pane's document. Always use `--bypass-claim` for cross-session transfers — it signals deliberate intent.

**`--items`:** Selective pending transfer. Only moves lines containing `[#id]` for each specified ID. Remaining items stay in the source. Only valid with `component=pending`. IDs can include or omit the `#` prefix.

**What happens (full transfer):**
1. Checks pane ownership of target (unless `--bypass-claim`)
2. Reads the component content from the source
3. Clears the source component
4. Appends to the target component with a `*Transfer from <source>*` annotation
5. Also merges pending items if the transferred component is not `pending`
6. Commits the target to prevent `(HEAD)` marking on next cycle
7. Saves snapshots for both files

**What happens (--items selective):**
1. Checks pane ownership (unless `--bypass-claim`)
2. Scans source pending for lines matching `[#id]` patterns
3. Moves matched lines to target pending; leaves unmatched in source
4. Commits target; saves both snapshots
5. Warns about any IDs that didn't match

## Referral (pointer instead of copy)

Inserts a structured referral tag in the target, leaving content in the source. The target session can read the source for context without duplicating history.

```bash
agent-doc transfer <SOURCE> <TARGET> <COMPONENT> --referral [--bypass-claim]
```

**Example:** Reference tsift's exchange from tagpath without copying:
```bash
agent-doc transfer tasks/software/tsift.md tasks/software/tagpath.md exchange --referral --bypass-claim
```

**What gets inserted in target:**
```html
<!-- agent:referral src="../tsift.md" component="exchange" created="2026-04-20T..." -->
*Context from [../tsift.md](../tsift.md) — read source exchange for full history.*
<!-- /agent:referral -->
```

**What happens:**
1. Checks pane ownership (unless `--bypass-claim`)
2. Computes relative path from target to source
3. Inserts referral block into target's matching component
4. Source content is NOT modified
5. Commits target; saves snapshot

**When preflight sees referrals:** Future enhancement — preflight can resolve `<!-- agent:referral -->` tags and inject referenced content as context for the responding agent.

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
- **Cross-pane transfers:** Use `--bypass-claim` when the target is owned by a different pane. Transfer is a deliberate user action, not a concurrent write, so pane ownership should not block it.
