---
name: feedback_passage_substitution_in_env_is_not_a_secret
description: "A dollar-paren passage/pass substitution in a .env scores as a high-entropy secret to a naive length/charclass scan — check the VALUE SHAPE before declaring an exposure, and compare hashes to decide whether a historical credential is still live."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e02d13b6-2abb-4ec9-baeb-f98355152c7a
  modified: 2026-08-03T03:27:12.795Z
---

Burned 2026-08-02 auditing `censible-core` for committed credentials.

`apps/finegar-etl/.env` held:

```
FACTSET_FTP_LOGIN="$(passage finegar/FACTSET_FTP_LOGIN)"
FACTSET_FTP_PASSWORD="$(passage finegar/FACTSET_FTP_PASSWORD)"
```

A redaction-safe inventory that only prints `key`, `len`, and a charclass reports
those as 36 and 39 characters of "mixed" — indistinguishable from a real
credential. I filed a backlog item, wrote two commit messages, and answered in the
session document all claiming the file "carries a real FactSet FTP login and
password." It never did: a commit five months earlier had already replaced the
literals with passage references.

The detector bug was specific: the placeholder pattern recognised `${...}` but
not `$(...)`, so command substitutions fell through to VALUE-PRESENT.

**How to apply.** Before calling anything an exposure, classify the value's
*shape*, not its length:

- `$(...)` / `${...}` / backtick substitution -> indirection, NOT a secret
- `scheme://user:pass@host` -> parse it; the password may be `__PASSWORD__` or a
  localhost dev value
- opaque high-entropy string -> candidate secret

Length and charclass alone cannot tell a 39-char secret from a 39-char reference
to one.

**The decisive follow-up question is not "is it committed" but "is it still
live."** You can answer that without ever printing a secret: SHA-256 the value
found in git history and compare against what the secret store serves now.

```
git show <old-commit>:<file>            # hash the literal, do not print it
passage <entry>                         # hash it, do not print it
identical -> never rotated, historical copy is LIVE
different -> already rotated, historical copy is worthless
```

Here they were identical across four years, which turned a vague "maybe worth
worrying about" into a concrete "rotate this." That comparison is what makes a
credential-exposure finding actionable — do it before recommending a history
rewrite, since rotation supersedes rewriting (rewriting cannot un-disclose
anything and invalidates every clone).

Related: [[feedback_no_cleartext_secrets]], [[feedback_nested_json_redaction]],
[[feedback_no_hallucinate_correct_errors]].
