---
agent_doc_session: f180b79b-8203-4b12-b7c5-5b38768fa541
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Session started. Awaiting input.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
I signed up for a ragie account.
*Session initialized.* What would you like to work on with Ragie?
See tasks/professional/bob-remekia.md

How can we use Ragie with Corky? What would Ragie add to my document analysis?

### Ragie + Corky Integration Analysis

**Ragie fills Corky's biggest gap: semantic search and retrieval.**

Corky stores all correspondence as flat Markdown files — great for git, agents, and human readability. But its search is limited to keyword substring matching on subjects. No full-text search, no embeddings, no semantic similarity.

Ragie is a managed RAG-as-a-Service pipeline (founded by Bob Remeika) that handles document ingestion, intelligent chunking, multi-layer indexing (vector + keyword + summary), and context-optimized retrieval.

**Concrete integration opportunities:**

1. **Correspondence search** — Index `mail/conversations/*.md` into Ragie. Query like "what did Bob say about the funding timeline?" instead of grepping subjects.: I would want to index these files locally for RAG. I can see Ragie being more for collaborative setups where the users don't have a developer setup. I can see Ragie being a great fit with judith-krug's project...where she and her staff won't be using an agent-doc workflow or git repos with the Markdown files. Is it fair to say that Ragie is a cloud database dedicated to context files...effectively a cloud file system. Even if I prefer the git + a local database approach, I do want practice for my work with Judith.
2. **Contact intelligence** — Index `mail/contacts/*/CLAUDE.md` files. Before drafting outreach, retrieve all context about a person across threads, notes, and prior exchanges.
3. **Cross-source retrieval** — Corky merges Gmail, Telegram, Slack, SMS into unified Markdown. Ragie indexes that unified corpus → semantic search across all channels.: I presume that I would have to install or build the integration between the different data sources into either the file system or Ragie.
4. **Agent-doc context injection** — During `/agent-doc` sessions, pull relevant correspondence via Ragie retrieval API to ground responses in email history.
5. **Draft assistance** — Before composing a reply, retrieve full conversation context + related threads via Ragie for complete situational awareness.

**What Ragie adds beyond file reading:**
- Semantic similarity (find related threads even with different terminology)
- Multi-format ingestion (PDFs, attachments Corky doesn't parse)
- Automatic chunking tuned to content type
- Reranking and context compression for LLM windows

**Integration path:** Ragie has Python SDK + REST API. Potential `corky index` command to push conversations to Ragie, `corky search` to query retrieval. Manifest metadata (labels, contacts, dates) → Ragie document metadata for filtered retrieval.: Would it make sense to have a `corky ragie` command tree?

### Re: Local vs. cloud RAG positioning

Yes, that's a fair characterization. Ragie is essentially a **managed cloud document database with built-in RAG pipeline** — upload documents, it handles chunking/indexing/retrieval. It's the "don't run your own infra" option.

For your personal workflow (git + local DB), something like a local vector store (SQLite + embeddings, or `tantivy` for full-text search in Rust) would be lighter weight and keep everything local.

For Judith's project — exactly right. Her staff won't touch git or Markdown. Ragie gives them a clean upload-and-query interface without developer tooling. **Practicing the Ragie API now on your own corpus is smart prep for building her integration later.**

### Re: Data source integration

Correct. Ragie has built-in connectors (Google Drive, Notion, Confluence, Slack) but for your custom sources (Corky's unified Markdown, Telegram imports, SMS), you build the push side. Flow: **Corky syncs sources → Markdown files → your integration pushes to Ragie via Documents API → Ragie handles chunking/indexing/retrieval.**

### Re: `corky ragie` command tree

Yes, makes sense as the integration surface:
- `corky ragie push` — push conversations/contacts to Ragie (incremental via manifest dates)
- `corky ragie search` — semantic query across indexed corpus
- `corky ragie sync` — bidirectional state check (what's indexed, what's stale)
- `corky ragie status` — account info, document counts, index health

This keeps Ragie as an optional module behind a feature flag rather than baking it into core sync. For Judith's project, a separate config profile could use Ragie as the primary store instead of local files.
<!-- agent:boundary:34aca0d3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Explore Ragie Python SDK / REST API — test document upload and retrieval
- [ ] Prototype `corky ragie push` with sample conversation files
- [ ] Design `corky ragie` command tree (push/search/sync/status)
- [ ] Evaluate Ragie pricing/limits for correspondence corpus size
- [ ] Plan Judith Krug project integration using Ragie as primary store
- [ ] Research local RAG alternatives (tantivy, SQLite+embeddings) for personal workflow
<!-- /agent:pending -->
