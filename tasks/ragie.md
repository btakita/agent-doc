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

1. **Correspondence search** — Index `mail/conversations/*.md` into Ragie. Query like "what did Bob say about the funding timeline?" instead of grepping subjects.
2. **Contact intelligence** — Index `mail/contacts/*/CLAUDE.md` files. Before drafting outreach, retrieve all context about a person across threads, notes, and prior exchanges.
3. **Cross-source retrieval** — Corky merges Gmail, Telegram, Slack, SMS into unified Markdown. Ragie indexes that unified corpus → semantic search across all channels.
4. **Agent-doc context injection** — During `/agent-doc` sessions, pull relevant correspondence via Ragie retrieval API to ground responses in email history.
5. **Draft assistance** — Before composing a reply, retrieve full conversation context + related threads via Ragie for complete situational awareness.

**What Ragie adds beyond file reading:**
- Semantic similarity (find related threads even with different terminology)
- Multi-format ingestion (PDFs, attachments Corky doesn't parse)
- Automatic chunking tuned to content type
- Reranking and context compression for LLM windows

**Integration path:** Ragie has Python SDK + REST API. Potential `corky index` command to push conversations to Ragie, `corky search` to query retrieval. Manifest metadata (labels, contacts, dates) → Ragie document metadata for filtered retrieval.
<!-- agent:boundary:ef65e099 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Explore Ragie Python SDK / REST API for indexing Corky conversations
- [ ] Design `corky index` command for pushing mail/conversations to Ragie
- [ ] Design `corky search` command for semantic retrieval via Ragie
- [ ] Test Ragie ingestion with sample conversation files
- [ ] Evaluate Ragie pricing/limits for correspondence corpus size
<!-- /agent:pending -->
