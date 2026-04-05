---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
agent_doc_auto_compact: 250
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260405-190537.md`*

I don't think I'm going to use aggressive auto compact too much. I would want more control.
However, a partial auto compact may be good. Where the current active topics are kept while anything before the current active topics are compacted.
### Re: Auto-compact control + partial compaction + eval tuning (HEAD)

**Auto-compact feedback noted.** The current implementation is all-or-nothing (compacts entire exchange component). A partial compact that preserves active topics while archiving older content would be more useful. That's a feature for `compact.rs` -- detect topic boundaries (e.g. `### Re:` headers with their preceding user prompts) and only archive exchanges before the last N topics. Not implemented yet, added to pending.

I set `agent_doc_auto_compact: 250` on this document as a safety net rather than an aggressive threshold. It'll only trigger when the exchange grows past ~250 lines.

**Eval tuning results (all 3 tasks done):**

1. **System prompt added** -- `evals/general/token-efficiency/system-prompt.md` explains the "do" convention, conciseness expectation, and `### Re:` headers. The `approval-minimal-response` case went from 0.0 to 0.85 score.

2. **Budgets tuned:**
   - `simple-question-concise`: 300 -> 400 (model uses ~340-376)
   - `approval-minimal-response`: 500 -> 600 (model uses ~541)
   - `multi-topic-structured`: 800 -> 1200 (3 topics at ~330 each)
   - `single-line-edit-response`: kept at 200 (model uses ~31)
   - `no-changes-fast-exit`: kept at 100 (model uses ~50-61)

3. **Remaining issue:** `simple-question-concise` scores low (0.3) because the model adds `### Re:` headers and subsections even for single questions. The system prompt encourages this. The eval's `expected_behavior` and system prompt conflict -- need to add "for single questions, omit the `### Re:` header" to system prompt or adjust the eval expectations.
<!-- agent:boundary:17a77b25 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] Test auto-compact threshold on a real session document
- [x] Add system prompt to token-efficiency evals for approval case
- [x] Tune token budgets based on baseline
- [ ] Implement partial compact (keep last N topics, archive older)
- [ ] Fix simple-question-concise eval (system prompt vs expected_behavior conflict)
<!-- /agent:pending -->
