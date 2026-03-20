---
agent_doc_session: 04a402a5-4cb5-46b2-906d-6a010216134a
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange patch=append -->
I want to delete controversial tweets from my x.com profile.

**Options for bulk tweet management:**

1. **X's built-in tools** — Go to Settings > Your Account > Download an archive of your data. This gives you a `tweets.js` file with all your tweets. We can parse it, filter by keywords/topics, and generate a list of tweet IDs to delete.

2. **Manual browser approach** — Use X's Advanced Search (`x.com/search-advanced`) to find your own tweets containing specific keywords, date ranges, or engagement patterns, then delete them one by one.

3. **Automated deletion via API** — If you have X API access (developer account), we can write a script that:
   - Loads your tweet archive
   - Filters tweets by keywords, date range, or sentiment
   - Deletes matching tweets via the API

**Recommended workflow:**
1. Download your tweet archive from X settings
2. Define what "controversial" means for your case (keywords, topics, date ranges)
3. Review the filtered list before any deletion
4. Delete in batches with confirmation

Which approach would you like to take? And what criteria should we use to identify "controversial" tweets — specific keywords, topics, date ranges, or something else?
Please help me filter and selectively delete.
<!-- /agent:exchange -->
