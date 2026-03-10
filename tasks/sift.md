---
agent_doc_session: c94cd9fd-71b6-4588-aab5-d53d9c9887f3
tmux_session: '0'
agent_doc_mode: template
---

Please take a look at sift:

https://www.alexdk.com/blog/introducing-sift
https://www.linkedin.com/posts/alexanderkwiatkowski_sift-local-hybrid-search-without-the-infrastructure-activity-7436987267635625984-PAyE?utm_source=share&utm_medium=member_desktop&rcm=ACoAAAC0KQ4BlxIAK5Vbm7Gssr6SzIaiyNG0e68

How can we use this with our tooling to drive semantic search?
- tagpath
- agent-doc
- corky
- existence-lang

<!-- agent:input -->
<!-- /agent:input -->

<!-- agent:output -->
**Sift** is a local, daemonless Rust CLI for hybrid search (BM25 + vector + LLM reranking). Single binary, no cloud, SIMD-optimized. Searches text, HTML, PDF, and OOXML files. Uses local embeddings via Candle and content-addressable caching with BLAKE3.

**Key capabilities:** `sift search <path> "<query>"` with configurable strategies (lexical, semantic, hybrid), query expansion, reciprocal rank fusion, and optional Qwen2.5 reranking. Sub-30ms latency on thousands of documents.

**Source:** [github.com/rupurt/sift](https://github.com/rupurt/sift) (Rust, MIT)

### Integration Opportunities

**1. tagpath** — Sift adds the semantic layer tagpath lacks. tagpath does structural/syntactic search (identifier pattern matching via tag extraction) but has no understanding of meaning.
- Use Sift to find semantically relevant files, then tagpath to extract matching identifiers from those files
- tagpath could emit a searchable index that Sift indexes for semantic querying

**2. agent-doc** — No search currently. Sift is a natural fit for cross-session retrieval:
- Index `tasks/*.md` — "what did we decide about naming?" finds the relevant session and section
- Could power a `/search` skill across all agent-doc sessions
- Snapshot history in `.agent-doc/snapshots/` is also searchable

**3. corky** — Email search is currently Gmail API-only. Sift enables local search over `mail/conversations/`:
- Semantic search across email threads: "discussions about deployment timeline"
- Cross-reference contacts with conversation content
- Offline capability (no API calls for already-fetched mail)

**4. existence-lang** — Has relevance-scored search but keyword-based only. Sift adds:
- True semantic search: "terms about how things change over time" → evolution, story, state, signal
- Cross-ring relationship discovery the current substring matching misses
- Could augment or replace the existing `search` command

### Practical Next Steps

**Lowest friction:** Create a `/sift` Claude Code skill that runs `sift search` against configured paths (tasks/, mail/, ontology). No code changes to existing tools needed.

**Deeper:** Embed Sift as a Rust library (same ecosystem as tagpath and existence-lang) for programmatic search pipeline access.
<!-- /agent:output -->
