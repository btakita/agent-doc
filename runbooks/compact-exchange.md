# Compact Exchange

Steps to compact an agent-doc exchange component when it grows too large.

## When to compact

- User explicitly requests "compact exchange"
- Never auto-compact without user approval
- Auto-compact is off by default. The only way it becomes "active" is an explicit `agent_doc_auto_compact` opt-in in document frontmatter or project `.agent-doc/config.toml`. Without that opt-in, `session_accretion.guidance` emits a gated reminder ("Exchange is large; ask the user before compacting...") instead of an imperative `Run agent-doc compact <FILE>` directive — agents must follow that gate and ask the user before invoking compact.

## Steps

1. **Read the full exchange content** from the document

2. **Read the canonical state surfaces** that must survive compaction:
   - `agent:backlog` / `agent:pending`
   - `agent:queue`
   - `agent:icebox`
   - Treat those components as the source of truth for unresolved work. Do not rely on old exchange prose alone when deciding what context must survive.

3. **Summarize** — preserve:
   - Decisions made (with rationale)
   - Key facts and discoveries
   - Open items and pending work
   - The top active backlog items, including gated items that still matter
   - The queue head and any remaining queued prompts when present
   - Iceboxed work that still affects future routing or context
   - **Unanswered user input** — if the exchange contains uncommitted questions or instructions that haven't been responded to, note them as open items in the summary (don't silently drop them)
   - Use frontmatter `prompt_presets` only as optional policy knobs for how much state to mention (for example "include the top 3 backlog items"), not as the source of substantive context
   - Discard verbose back-and-forth, code snippets already committed, exploratory dead-ends

4. **Run `agent-doc compact <FILE> --component exchange --commit`**
   - Archives the original content to `.agent-doc/archives/`
   - Replaces exchange content with the supplied summary, or when no custom `--message` is provided, with a default session summary that includes archive pointer plus live backlog/queue/icebox context
   - Updates snapshot atomically
   - Closes out via the binary-owned `agent-doc commit` path and verifies the VCS refresh signal when available
   - If a live template/CRDT session already has a bare `compact exchange` user directive in the diff, later write-back also forces `agent:exchange` replacement semantics so the checkpoint summary cannot silently append over old exchange content
   - Pass `--message "summary text"` to include a custom summary instead of the binary default session summary
