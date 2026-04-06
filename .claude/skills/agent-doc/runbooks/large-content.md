# Large Content Handling

When the user requests generation of large content (transcriptions, long reports, scraped data, code output), do NOT embed it inline in the agent-doc document.

## Rule

**Write large content to a separate file, then link from the document.**

Large = anything that would make the document harder to read or navigate, typically:
- Transcriptions / diarized audio output
- Full log dumps
- Large code files
- Scraped/fetched data

## Pattern

1. Write the content to a dedicated file near the document:
   ```
   tasks/<project>/transcripts/YYYY-MM-DD-<description>.md
   tasks/<project>/logs/YYYY-MM-DD-<description>.txt
   ```
2. Link from the exchange component:
   ```markdown
   [Transcript: 2026-04-05](transcripts/2026-04-05-robert-ross.md)
   ```
3. Commit the new file alongside the document update.

## Why

Embedding large content in the document:
- Makes the exchange unreadable
- Bloats the snapshot / diff computation
- Floods context on future preflight reads
- Creates unnecessary noise in git history
