# Proposal: PR, MR and Ticket Sync Components (`agent:github`, `agent:gitlab`, `agent:jira`)

> **Status:** proposal, non-normative, needs operator review. Nothing here is
> implemented. Normative behavior stays in [`SPEC.md`](../../SPEC.md) and the
> numbered `specs/` files. A phase becomes normative only when its queue item
> lands spec + tests. The open questions are listed under
> [Decisions for the operator](#decisions-for-the-operator).

## Request

> I would like an agent-doc centric workflow to see all PR states + JIRA ticket
> states related to me or some other query synced in a document or element in
> an agent-doc. Note we need integrations with github + gitlab + Jira. Perhaps
> we can start with a dedicated tag to each integration? `agent:github` +
> `agent:gitlab` + `agent:jira`. Ideally it will be bidirectional. We can start
> with read-only syncing. But I would also like a way to send diff commands for
> each ticket. Perhaps we can have a nested list to send independent commands
> to avoid appending to a long line of text?

The nested-list part of the request is already implemented, in general form,
as nested sub-prompts (`#nestedsubprompts`, `specs/02-document-format.md`). This
proposal builds the tracker components on top of it.

## Goals

1. A document element shows the live state of the PRs, MRs and tickets that
   match a query ("assigned to me", "review requested from me", a JQL string),
   refreshed in the background.
2. The first phase is read-only. Nothing is written to a remote tracker.
3. In a later phase, the operator steers one ticket with nested per-ticket
   command rows. Each row is reviewed and sent as one explicit remote write.
4. Secrets never appear in the document, in logs or in the ops log.

Non-goals for the first phase: creating PRs or tickets, merging PRs, webhooks,
and a full mirror of comments or diffs.

## Component shape

One built-in element per provider, all with one shared shape:

```markdown
## PRs <!-- agent:github query="is:open is:pr involves:@me" interval=10m -->
- 🟢 [gh:btakita/agent-doc#412] Fold nested sub-prompts into their parent — review requested · CI passing · 2 approvals
- 🟡 [gh:btakita/agent-doc#409] Retire legacy child markers — changes requested · CI failing (lint)
<!-- /agent:github -->

## Tickets <!-- agent:jira query="assignee = currentUser() AND resolution = Unresolved" interval=15m -->
- [jira:PROJ-123] In Progress · High · Sync tracker state into documents
<!-- /agent:jira -->
```

- **Authority.** The rows are a `DerivedProjection`: the remote tracker is the
  source of truth and the binary owns every row byte. This is the same contract
  `agent:status` has. Operator edits to a row are not authority. The next
  refresh replaces the row, except for its nested command rows (see Phase 3).
- **Row identity.** Each row leads with a provider-qualified key
  (`gh:owner/repo#N`, `gl:group/project!N`, `jira:KEY-N`). The key is the row's
  stable identity across refreshes, reorders and state changes. It follows the
  `[#id]` bracket convention, but in a separate namespace, so it can never
  collide with a backlog id or be read as a `do [#id]` directive.
- **Rendering.** One line per item: a state glyph, the key, the title, then a
  short state summary (review decision, CI rollup, assignee, status, priority).
  The glyph palette goes through `[queue_icons]`-style configuration so it stays
  configurable. Rows sort by "needs my action" first, then by last update.
- **Attributes.** `query=` (provider syntax, verbatim), `interval=` (reuses the
  `#queueschedule` duration grammar), `limit=` (default 50), and optional
  `fields=` to pick summary columns.
- **Where it lives.** It is a new element crate per provider
  (`agent-doc-element-github`, ...), registered in `agent-doc-element-registry`,
  plus one shared IO crate (`agent-doc-tracker-sync-io`) that owns fetching,
  rate limiting and the projection write. The pure crates render and diff rows
  from fetched JSON. Only the IO crate touches the network.

## Auth and secret model

| Provider | Phase 1 credential | Never |
|---|---|---|
| GitHub | The `gh` CLI's existing auth (`gh api`, `gh pr list --json ...`). No token passes through agent-doc at all. | Read `GH_TOKEN` into the document, logs or ops log. |
| GitLab | `glab` CLI auth when present. Otherwise a personal access token fetched at call time from the operator's secret store (`passage show <path>`), named by config, never by the document. | Store the token in frontmatter or `.agent-doc/config.toml`. |
| Jira | API token + account email fetched at call time from the secret store. The site URL and the secret's store path live in project config. | Put the token, the email, or a Basic-auth header in any log line. |

- The document names only a credential *profile* (`auth="work"`). Project
  config maps the profile to a CLI or to a secret-store path. A document copied
  to another machine therefore carries no secret, and no path to one.
- The fetcher runs the secret command in a child process and passes the token
  to the HTTP client by environment or stdin, never on argv (`ps` shows argv).
- Every fetch error is redacted with `agent-doc-secret-redact` before it reaches
  the ops log or an `agent:status` line.
- A missing or expired credential is fail-closed and visible: the component
  keeps its last rows and shows one status row (`⚠️ github auth: run gh auth
  login`). It never retries hot.

## Refresh cadence

- The project controller's single scheduler loop (the one that already owns
  `interval=` queue passes, `#queueschedule`) ticks each tracker component. No
  new timer thread and no per-document daemon.
- A tick fetches, diffs the rendered rows against the current component body,
  and writes only when something changed, through the binary-owned projection
  write path. An unchanged fetch is an `empty_poll` with no document write and
  no commit.
- A manual refresh: `agent-doc tracker refresh <FILE> [--component <id>]`.
- An open editor sees the update like any other projection write. A refresh
  never runs inside an agent turn's write window, so it cannot race closeout.

## Rate limits and conflicts

- **Budget per provider and credential.** GitHub GraphQL and REST, GitLab, and
  Jira each publish limit headers. The IO crate records the remaining budget
  and the reset time, and backs off before it reaches zero (default: skip a
  tick when less than 10% remains).
- **Conditional requests.** Use ETag / `If-None-Match` (GitHub REST) and
  `updatedAt` cursors (GraphQL, JQL `updated >=`), so an idle tick costs close
  to nothing.
- **Backoff.** Exponential with jitter on 429 / 5xx and on network errors, and
  capped. The state is shown in `agent-doc tracker status`, mirroring
  `queue schedule`.
- **Read conflicts.** None: the remote is authoritative and rows are derived.
  A row the operator edited is overwritten on the next change of that item. The
  edit is recorded in the ops log so it is never silently lost.
- **Write conflicts (Phase 3).** Each write carries the remote's last-seen
  version (Jira `updated`, GitHub `updatedAt` / head SHA). If the remote moved,
  the write is refused and the command row is marked `⚠️ stale: refreshed`. The
  binary never applies a write over state it has not shown the operator.

## Phase 3: bidirectional writes through nested command rows

This builds on `#nestedsubprompts`. A command is a nested row under a tracker
row:

```markdown
- 🟡 [gh:btakita/agent-doc#409] Retire legacy child markers — changes requested
  - comment: rebased on main, PTAL
  - request-review: @reviewer
  - label: +needs-release-note
```

- A nested row under a tracker row is a **command sub-prompt** for that item.
  It is never rendered by the binary and never overwritten by a refresh.
- The binary parses a closed verb set per provider (`comment:`, `label: +x/-x`,
  `assign:`, `request-review:`, `transition: In Review`, `close`, `reopen`).
  Free text that matches no verb is handed to the agent as a normal sub-prompt
  for that item, the same as a sub-prompt under an `agent:run` row.
- **Nothing is sent on edit.** A command is sent when the operator authorizes it
  explicitly, either with a `go` on the tracker component or with a per-row
  send marker. The result is projected back as the row's lifecycle marker
  (`☑️` sent, `⚠️` refused, with the reason) and the row is struck once the next
  refresh shows the effect.
- Each sent command is one remote write with an idempotency key. The key is
  derived from the item key plus the command row's identity, so a retry never
  posts a comment twice.
- An audit line in the ops log records each write: provider, item key, verb,
  and outcome. It never records a secret.

## MVP order

1. **GitHub, read-only** (`agent:github`), via `gh api graphql` with the
   existing `gh` auth: open PRs involving me, review-requested, and authored.
   It needs no new secret handling and is the fastest way to validate the row
   shape, the refresh cadence and the projection path.
2. **Jira, read-only** (`agent:jira`), with a JQL query and the token from the
   secret store. It validates the secret model.
3. **GitLab, read-only** (`agent:gitlab`), with `glab` auth or a token.
4. **Phase 3 writes**, GitHub first (`comment:`, `label:`, `request-review:`),
   then Jira transitions. Phase 3 is not started without a separate operator go.

## Decisions for the operator

1. **One element per provider, or one `agent:tracker provider=github`
   element?** The request suggests one per provider. One element with a
   `provider=` attribute shares more code and lets one component mix providers
   ("everything assigned to me"). Which do you prefer?
2. **Where do credential profiles live?** The proposal says project config
   (`.agent-doc/config.toml`) maps `auth="<profile>"` to a CLI or a
   secret-store path. Should it be user-global config instead, so one profile
   serves every project?
3. **Default refresh interval and quiet hours.** Is 10 minutes for GitHub and
   15 for Jira acceptable? Should refreshes pause while no editor has the
   document open?
4. **Does a refresh commit?** Derived rows change often. Should tracker
   refreshes be committed like other projection writes, or kept out of git (for
   example by rendering into a component excluded from autocommit)?
5. **Operator edits to a derived row.** The proposal overwrites them and logs
   the edit. Should an edit instead be read as a command (a title edit becomes a
   rename), or be refused visibly?
6. **The Phase 3 send gesture.** Is it component-level `go`, a per-row send
   marker, or both? Should a destructive verb (`close`, `transition: Done`)
   always need a per-row confirmation?
7. **Tracker rows as queue work.** Should a tracker row be able to feed
   `agent:run` (for example, "a PR with CI failing becomes a `#gh-fix` head"),
   or does that stay a manual step?
8. **Scope of "related to me".** For GitHub, is it `involves:@me` (authored,
   assigned, mentioned, review-requested), or only review-requested and
   assigned?
