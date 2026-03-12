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

<!-- agent:exchange -->
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

### LinkedIn Comment (with GitHub links)

```
Really impressed with the architecture — the BLAKE3 content-addressable cache and SIMD-optimized vector scoring are exactly the kind of design choices that make local-first tooling practical.

I'm building several Rust CLI tools that could benefit from Sift's search pipeline:

- tagpath — structural code search via tag-based identifiers (syntactic). Sift's semantic layer would complement it perfectly
https://github.com/btakita/tagpath

- existence-lang — ontology term search (currently keyword-based). Semantic search would unlock cross-ring relationship discovery
https://github.com/existence-lang/existence

- agent-doc — interactive document sessions. Cross-session retrieval over markdown files is a gap right now
https://github.com/btakita/agent-doc

Would love to explore integrating Sift as a library dependency or coordinating on a shared search interface. Happy to jump on a call if you're interested.
```

### Networking Guide: Public Comment → Call

**The escalation ladder:**
1. **Public comment** (you're here) — shows genuine interest, gives Alex social proof
2. **Alex replies** — even a brief "thanks, interesting!" opens the door
3. **You DM** — "Hey, loved your post on Sift. I'm working on some Rust CLI tools where hybrid search would fit perfectly. Would you be up for a quick 20-min call to explore integration ideas?"
4. **Call** — keep it short (20-30 min), focused on concrete integration

**Why it's not bold:** Alex just shipped a new open-source project with 2 stars. He *wants* early adopters and integration partners. Someone reaching out with specific use cases (not just "cool project") is exactly what he's hoping for. You're bringing real value — working Rust tools that could become early consumers of his library.

**The key phrase** "Happy to jump on a call if you're interested" is soft, no-pressure, and lets Alex opt in. Very standard in developer circles.

**Shortcut option:** Skip the ladder — DM him directly. His post is fresh, you already commented asking about hooks/Tree Sitter, he knows who you are. A DM saying "I have some Rust tools that could use Sift, want to chat?" is perfectly appropriate.

Convert this document to stream mode.
<!-- /agent:exchange -->
