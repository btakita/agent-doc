# Proposal: PR, MR and Ticket Sync Components (`agent:github`, `agent:gitlab`, `agent:jira`)

> **Status:** proposal, non-normative. The operator answered the design
> questions on 2026-10-27 (`#pmsyncquestions`); the answers are recorded under
> [Decisions](#decisions), with the two recommendations the operator asked for
> under [Recommendation: refresh cadence](#recommendation-refresh-cadence) and
> [Recommendation: status updates](#recommendation-status-updates). Nothing here
> is implemented. Normative behavior stays in [`SPEC.md`](../../SPEC.md) and the
> numbered `specs/` files. A phase becomes normative only when its queue item
> lands spec + tests. What is still undecided is listed under
> [Still open](#still-open).

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

One built-in element per provider (Decision 1), all with one shared shape:

```markdown
## PRs <!-- agent:github query="is:open is:pr involves:@me" interval=5m -->
- 🟢 [gh:btakita/agent-doc#412] Fold nested sub-prompts into their parent — review requested · CI passing · 2 approvals
- 🟡 [gh:btakita/agent-doc#409] Retire legacy child markers — changes requested · CI failing (lint)
<!-- /agent:github -->

## Tickets <!-- agent:jira query="assignee = currentUser() AND resolution = Unresolved" -->
- [jira:PROJ-123] In Progress · High · Sync tracker state into documents
<!-- /agent:jira -->
```

- **Authority.** The rows are a `DerivedProjection`: the provider (GitHub,
  GitLab, Jira) is the source of truth and the binary owns every rendered row
  byte. This is the same contract `agent:status` has. An operator edit to a
  rendered row is never authority over the provider's state. It is read as a
  command instead (Decision 5).
- **Row identity.** Each row leads with a provider-qualified key
  (`gh:owner/repo#N`, `gl:group/project!N`, `jira:KEY-N`). The key is the row's
  stable identity across refreshes, reorders and state changes. It follows the
  `[#id]` bracket convention, but in a separate namespace, so it can never
  collide with a backlog id or be read as a `do [#id]` directive.
- **Rendering.** One line per item: a state glyph, the key, the title, then a
  short state summary (review decision, CI rollup, assignee, status, priority).
  The glyph palette goes through `[queue_icons]`-style configuration so it stays
  configurable. Rows sort by "needs my action" first, then by last update, with
  the key as the final tie-breaker so the order is deterministic. A row never
  shows a relative or wall-clock time ("3h ago", "refreshed 14:02"): those
  change on every poll and would make an unchanged result look changed. Refresh
  times are reported by `agent-doc tracker status`, not in the document.
- **Attributes.** `query=` (provider syntax, verbatim), `interval=` (reuses the
  `#queueschedule` duration grammar), `limit=` (default 50), optional `fields=`
  to pick summary columns, `auth=` (a credential profile name, Decision 2), the
  queue controls `go` / `start` / `stop` and `subagents` (Decision 6), and the
  feed attributes `feed-run=` / `feed-todo=` (Decision 7).
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
| Jira | API token + account email fetched at call time from the secret store. The site URL and the secret's store path live in config. | Put the token, the email, or a Basic-auth header in any log line. |

- **Profiles by name (Decision 2).** A document selects a credential *profile*
  by name only. Config defines what the name means: which CLI to use, or which
  secret-store path to read, plus non-secret settings such as the Jira site URL
  or a self-hosted GitLab host. A document copied to another machine therefore
  carries no secret and no path to one.
- **Where the selection comes from, most specific first:** the component's
  `auth=` attribute, then frontmatter, then project config
  (`.agent-doc/config.toml`), then global config
  (`~/.config/agent-doc/config.toml`). This is the operator's
  frontmatter > project > global order, with the component attribute as a
  narrower document-level override (see [Still open](#still-open)).
- **Where the definition comes from:** project config, then global config. A
  project profile shadows a global profile with the same name. Frontmatter can
  name a profile and set its non-secret settings, but it never holds a
  secret-store path or a token. A frontmatter value that looks like a secret or
  a store path is refused with a visible warning and is not used.
- The fetcher runs the secret command in a child process and passes the token
  to the HTTP client by environment or stdin, never on argv (`ps` shows argv).
  Secrets are fetched from `passage` or `gh` at call time and are never cached
  on disk by agent-doc.
- Every fetch error is redacted with `agent-doc-secret-redact` before it reaches
  the ops log or an `agent:status` line.
- A missing or expired credential is fail-closed and visible: the component
  keeps its last rows and shows one status row (`⚠️ github auth: run gh auth
  login`). It never retries hot.

## Refresh cadence

- The project controller's single scheduler loop (the one that already owns
  `interval=` queue passes, `#queueschedule`) ticks each tracker component. No
  new timer thread and no per-document daemon.
- The interval is configurable (Decision 3). Resolution, most specific first:
  the component's `interval=`, then frontmatter, then project config, then
  global config, then the built-in default from
  [Recommendation: refresh cadence](#recommendation-refresh-cadence).
- A tick runs the change-detection ladder in that recommendation. An unchanged
  result is an `empty_poll`: no document write and no commit.
- A manual refresh: `agent-doc tracker refresh <FILE> [--component <id>]`. It
  skips the interval but still uses the same ladder, so it writes nothing when
  nothing changed.
- An open editor sees the update like any other projection write. A refresh
  never runs inside an agent turn's write window, so it cannot race closeout.
- **Commits (Decision 4).** A refresh is committed only when it changed the
  rendered rows. A write that changed rows gets its own binary-owned commit
  that touches only the document, and it is never folded into an agent turn's
  commit. Several components that change in one tick of one document share one
  commit. An `empty_poll` creates no commit.

## Rate limits and conflicts

- **Budget per provider and credential.** The IO crate records the remaining
  budget and the reset time from each provider's rate-limit headers, and backs
  off before it reaches zero (default: skip a tick when less than 10% remains).
- **Conditional requests and cursors.** See
  [Recommendation: refresh cadence](#recommendation-refresh-cadence).
- **Backoff.** Exponential with jitter on 429 / 5xx and on network errors, and
  capped. A provider's `Retry-After` is honored when present. The state is shown
  in `agent-doc tracker status`, mirroring `queue schedule`.
- **Row edits.** An operator edit to a rendered row is never silently
  overwritten. It is captured as a command before the row is re-rendered
  (Decision 5).
- **Write conflicts (Phase 3).** See
  [Recommendation: status updates](#recommendation-status-updates).

## Phase 3: bidirectional writes through command sub-items

This builds on `#nestedsubprompts`. A command is a nested sub-item under a
tracker row:

```markdown
- 🟡 [gh:btakita/agent-doc#409] Retire legacy child markers — changes requested
  - comment: rebased on main, PTAL
  - request-review: @reviewer
  - label: +needs-release-note
```

- A nested sub-item under a tracker row is a **command** for that item. The
  binary never renders it and a refresh never overwrites it. Sub-items are the
  preferred way to send a command, so that one row never grows a long line of
  appended text.
- The binary parses a closed, documented verb list per provider (`comment:`,
  `label: +x/-x`, `assign:`, `request-review:`, `rename:`,
  `transition: In Review`, `close`, `reopen`). Free text that matches no verb is handed to the agent as a
  normal sub-prompt for that item, the same as a sub-prompt under an `agent:run`
  row.
- **Commands follow queue-item rules (Decision 6).** The tracker component's
  commands form a list that drains like `agent:run` heads: nothing is sent on
  edit; a `go` or `start` control on the tracker component (marker-side, or
  frontmatter) authorizes sending, and `stop` holds every command. Per-line
  tags behave as on a queue line (`#subagent`, `[inline]`,
  `[operator-verify]`), and the `subagents` / `subagents=N` attribute
  dispatches commands to subagents under the same claim and capacity rules as
  `agent:run subagents`. A destructive verb (`close`, `transition: Done`) needs
  no extra per-row confirmation beyond these rules, but it is subject to the
  stale-state check in
  [Recommendation: status updates](#recommendation-status-updates).
- The result is projected back as the command's lifecycle marker (`☑️` sent,
  `⚠️` refused, with the reason). The command is struck once a refresh shows the
  effect at the provider.
- Each sent command is recorded in a local send ledger keyed by the item key
  plus the command's identity, so a retry never posts a comment twice (see
  [Still open](#still-open) for provider-native idempotency).
- An audit line in the ops log records each write: provider, item key, verb,
  and outcome. It never records a secret.

## Rows as queue and todo work

Tracker rows can feed `agent:run` and `agent:todo` automatically (Decision 7).

- `feed-run="<filter>"` on the tracker component mirrors each matching row into
  the document's `agent:run` as a head that names the key, for example
  `fix [gh:btakita/agent-doc#409]: CI failing (lint)`.
- `feed-todo="<filter>"` mirrors each matching row into `agent:todo`, for
  example `- [ ] [gh:btakita/agent-doc#412] review requested`.
- Filters are a small closed vocabulary over the rendered row state:
  `needs-me`, `review-requested`, `changes-requested`, `ci-failing`,
  `assigned`, and for Jira `status=<name>`. Several filters combine with `,`
  (any of them).
- A feed entry is keyed by item key plus filter reason, so one item produces at
  most one entry per reason, however many refreshes see it.
- A fed queue head is ordinary queue work: it runs only when that queue is
  active (`go` / `start`), so the queue's own controls stay the authorization
  gate.
- When the provider shows the condition cleared (CI green, review submitted),
  the provider is the source of truth: a fed todo is checked off, and a fed
  queue head that has not started is struck as `resolved upstream`. A head that
  a session or subagent has already claimed is left to its owner.

## MVP order

1. **GitHub, read-only** (`agent:github`), via `gh api graphql` with the
   existing `gh` auth, scoped to `involves:@me` by default (Decision 8). It
   needs no new secret handling and is the fastest way to validate the row
   shape, the refresh ladder, the commit rule and the projection path.
2. **Jira, read-only** (`agent:jira`), with a JQL query and the token from the
   secret store. It validates the profile and secret model.
3. **GitLab, read-only** (`agent:gitlab`), with `glab` auth or a token.
4. **Row-edit capture and feeds** (Decisions 5 and 7), still read-only at the
   provider.
5. **Phase 3 writes**, GitHub first (`comment:`, `label:`, `request-review:`),
   then Jira transitions. Phase 3 is not started without a separate operator go.

## Decisions

Recorded from the operator's answers to `#pmsyncquestions` (2026-10-27).

1. **One component per provider.** `agent:github`, `agent:gitlab` and
   `agent:jira` are separate components. There is no `agent:tracker
   provider=...`. They share one IO crate and one row shape, so a provider is
   added without changing the others.
2. **Credential profiles live in frontmatter, project config and global
   config, with precedence frontmatter > project config > global config.** The
   document refers to a profile by name only. Secrets are still fetched at call
   time from `passage` or `gh` and are never written into the document. See
   [Auth and secret model](#auth-and-secret-model).
3. **Refresh intervals are configurable.** The default and the pause rule
   follow the recommendation below, which the operator asked for.
4. **A refresh is committed only when the synced content changed.** An
   unchanged poll writes nothing and commits nothing.
5. **An edit to a synced row is a diffable command.** The binary keeps the last
   rendered bytes of each row. On the next preflight or refresh, it diffs the
   operator's version of the row against them and classifies the change, so the
   agent always sees which kind of edit it was:
   - **appended text** (the operator typed after the rendered row) becomes a
     command, exactly as if it had been written as a sub-item;
   - **a changed rendered field** (title, status, assignee, labels) becomes the
     matching verb, for example `status: In Progress → Done` becomes
     `transition: Done`, and a title edit becomes `rename:`;
   - **a deleted row** is a hide request for that item, never a close (closing
     needs an explicit `close` command).

   The captured command is written as a nested sub-item under the row, tagged
   with what it came from (for example `- transition: Done (from row edit)`),
   and the row is re-rendered from the provider. The operator's intent is never
   lost, the row never shows state the provider does not have, and the next
   edit lands as a sub-item. Nested sub-items stay the preferred, encouraged way
   to add a command. Status always comes from the provider (see
   [Recommendation: status updates](#recommendation-status-updates)).
6. **Commands follow queue-item rules.** Commands are a list, drained by the
   same `go` / `start` / `stop` controls as `agent:run`, with the same per-line
   tags, and with the `subagents` attribute supported. A destructive verb has
   no separate per-row confirmation.
7. **Tracker rows can feed `agent:run` and `agent:todo` automatically,**
   through `feed-run=` and `feed-todo=`. See
   [Rows as queue and todo work](#rows-as-queue-and-todo-work).
8. **"Related to me" means `involves:@me`** on GitHub. GitHub defines
   `involves` as author, assignee, mentioned or commenter. It does not include
   a pending review request on its own (see [Still open](#still-open)). The
   GitLab and Jira defaults use the closest equivalent each provider offers.

## Recommendation: refresh cadence

The operator asked: "If we use deterministic logic to detect changes, then what
do you recommend?"

**Recommended defaults:** `interval=5m` for every provider while the document
is in use, a floor of `1m` (a lower value warns and is raised to `1m`), and a
pause when nothing is using the document.

Change detection is a ladder. Each rung runs only when the rung before it says
something may have changed, so an idle tick costs close to nothing and an
unchanged result never writes:

1. **Conditional request.** Send the validator from the last successful fetch:
   `If-None-Match: <ETag>` (or `If-Modified-Since`) on endpoints that return
   one. On GitHub REST, a `304 Not Modified` to an authorized request does not
   count against the primary rate limit. A 304 ends the tick as an
   `empty_poll`.
2. **Update cursor.** Where conditional requests are not available, ask only
   for items updated since the last cursor: GitHub GraphQL `search` with an
   `updated:>=<cursor>` qualifier, GitLab `updated_after=`, Jira
   `updated >= "<cursor>"` in JQL. Jira's `updated` comparison works at minute
   resolution, so the cursor overlaps the previous tick by one minute and rows
   are de-duplicated by key and update time. No updated items ends the tick as
   an `empty_poll`.
3. **Full reconcile.** A cursor cannot see an item that *left* the query (a PR
   closed or a ticket reassigned no longer matches the open/assigned query). So
   when rung 1 or 2 reports a change, and in any case every 6th tick, the tick
   fetches the full result set up to `limit=`.
4. **Content digest.** Render the rows deterministically (stable sort, no
   relative or wall-clock times, no rate-limit or refresh counters) and take a
   digest of the rendered body. If the digest equals the digest of the last
   write, and the document's current rows still match that write, the tick is
   an `empty_poll`: no write, no commit. Only a different digest writes, and
   that write is the one commit Decision 4 allows.

The digest is the rule that makes Decision 4 hold. It also means that a change
the operator does not see (a provider field that is not rendered) never causes
a write.

**Pause rule.** A tracker component refreshes while its document is in use, and
pauses otherwise. "In use" means at least one of:

- an editor has the document open (a live editor lease);
- the document has an active queue or loop (`go` / `start` / `interval=` pass),
  or a live subagent claim, so that `feed-run=` / `feed-todo=` keep working when
  the queue runs headless;
- an agent session holds the document's claim.

While paused, nothing is fetched. When the document comes back into use (an
editor opens it, a queue starts, a session claims it), the scheduler runs one
catch-up tick at once if the last refresh is older than the interval, then
returns to the normal interval. Preflight reports each tracker component's age
as a non-blocking signal, so an agent turn knows when it is reading old rows.

**Provider hints.** A provider's own poll hint wins when it asks for less
frequent polling than configured, for example GitHub's `X-Poll-Interval`
header on endpoints that send it.

**Provider facts.** Verified: GitHub REST returns `ETag` and supports
`If-None-Match`, and an authorized 304 does not count against the primary rate
limit; the authenticated REST primary limit is 5,000 requests per hour.
Unverified, to confirm before implementation: whether the GitHub REST search
endpoint returns usable ETags; the GitHub GraphQL point cost of one `search`
query of 50 nodes (expected to be about 1 point of 5,000 per hour; GraphQL has
no conditional requests); GitLab.com's current authenticated rate limit and
whether its list endpoints honor `If-None-Match`; and Jira Cloud's current
rate-limit model and search endpoint (Atlassian has been moving from
`/rest/api/3/search` to `/rest/api/3/search/jql`). At `interval=5m`, one
component makes 12 ticks an hour, which is far inside any of these limits even
when every tick reaches rung 3.

## Recommendation: status updates

The operator asked: "What happens when a status is updated?" The provider is
the source of truth for status in every case below. The document only ever
shows status the provider returned.

- **A status changes upstream** (someone moves a ticket, a PR is merged, CI
  goes red). The next refresh sees it through the ladder, re-renders the row
  from the provider, and commits once (Decision 4). Commands under the row are
  kept. Feeds re-evaluate: a cleared condition checks off its fed todo or
  strikes its unstarted fed head (see
  [Rows as queue and todo work](#rows-as-queue-and-todo-work)).
- **An operator command changes a status** (`transition: Done`, `close`, or a
  status edit captured as one). The row's status does not change when the
  operator types. The command waits under the row until the queue rules send
  it. When it is sent, the binary sends it to the provider and re-renders the
  row from the provider's response to that write, or, when the response does
  not carry the new state, from an immediate single-item refresh or the next
  tick. It never renders the operator's text as the new status. If the
  provider rejects the write (an invalid transition, a missing permission), the
  command gets `⚠️` with the provider's reason and the row keeps the provider's
  status.
- **A command conflicts with an upstream change.** When a command is captured,
  it records the item's base version: Jira `updated`, GitHub `updatedAt` (and
  the head SHA for a PR), GitLab `updated_at`. Just before sending, the binary
  reads the item once more and compares:
  - If the item closed or merged upstream, every unsent command on it is
    refused: `⚠️ stale: merged upstream`.
  - A state verb (`transition:`, `close`, `reopen`) is refused when the state
    field moved since the base version: `⚠️ stale: status moved In Progress →
    In Review upstream`. The binary never applies a state change to state the
    operator was not shown.
  - An additive verb (`comment:`, `label:`, `assign:`, `request-review:`) is
    sent when the item is still open, because it does not depend on the
    current status.
  - A refused command stays in the list. The operator re-arms it with the
    queue rules (a fresh `go`), which takes the current provider state as its
    new base version, or deletes it.
- **A write already in flight when the upstream change lands.** A small window
  remains between the pre-send read and the write. The provider orders the two
  writes, and the next refresh renders whatever the provider ended with. The
  ops-log audit line records the base version, so the operator can see that
  both writes happened. Whether a provider offers a server-side
  compare-and-set that closes this window is unverified (see
  [Still open](#still-open)).

## Still open

1. **Review requests outside `involves:@me`.** A PR where the operator's
   review is requested, but who has not commented and is not assigned or
   mentioned, does not match `involves:@me`. GitHub search cannot OR two
   qualifiers in one query, so covering it means a second
   `review-requested:@me` query merged by key into the same component. Confirm
   whether the default should include it.
2. **`auth=` on the component.** The operator's order is frontmatter > project
   config > global config. This proposal adds the component's `auth=` attribute
   above frontmatter, so two components in one document can use different
   profiles. Confirm, or drop the attribute.
3. **Naming of `feed-run=` / `feed-todo=` and the filter vocabulary.** Agree on
   the names before implementation.
4. **GitLab and Jira equivalents of `involves:@me`.** GitLab has no single
   `involves` filter: the likely default is the union of `author`, `assignee`
   and `reviewer` = me. Jira's likely default is
   `assignee = currentUser() OR reporter = currentUser() OR watcher = currentUser()`.
5. **Hiding a deleted row.** Where a hidden key is stored (component attribute,
   frontmatter, or a sidecar), and how the operator un-hides it.
6. **Provider-native idempotency and compare-and-set.** Whether GitHub, GitLab
   or Jira accept an idempotency key or a conditional write. Until verified,
   the plan is the local send ledger plus a reconciling read after an
   ambiguous failure (a timeout), before any retry.
7. **The unverified provider facts** listed under
   [Recommendation: refresh cadence](#recommendation-refresh-cadence).
