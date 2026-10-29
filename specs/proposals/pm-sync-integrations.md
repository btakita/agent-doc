# Proposal: PR, MR and Ticket Sync Components (`agent:github`, `agent:gitlab`, `agent:jira`)

> **Status:** proposal, non-normative. The operator answered the design
> questions on 2026-10-27 (`#pmsyncquestions`) and the follow-up points on
> 2026-10-28 (`#pmsyncstillopen`); the answers are recorded under
> [Decisions](#decisions), with the recommendations the operator asked for
> under [Recommendation: refresh cadence](#recommendation-refresh-cadence),
> [Recommendation: status updates](#recommendation-status-updates) and
> [Research: "related to me" on each provider](#research-related-to-me-on-each-provider),
> and the hide design under [Hidden rows](#hidden-rows). Nothing here
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

A nested child under `agent:github`, `agent:gitlab`, or `agent:jira` that is
its own leaf is submitted with the same parent lineage as a nested `agent:run`
leaf (`#nestsubmit`, `specs/pending-system.md`). The shared path also attaches
that body's prompt tree as context. That is the existing nest vector, not this
proposal's fetch, auth, or write product. Nothing in those phases is
implemented here.

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
## PRs <!-- agent:github interval=5m -->
- 🟢 [gh:btakita/agent-doc#412] Fold nested sub-prompts into their parent — review requested · CI passing · 2 approvals
- 🟡 [gh:btakita/agent-doc#409] Retire legacy child markers — changes requested · CI failing (lint)
- Hidden
  - [gh:btakita/agent-doc#398] Bump the release checklist
<!-- /agent:github -->

## Tickets <!-- agent:jira query="assignee = currentUser() AND resolution = Unresolved" -->
- [jira:PROJ-123] In Progress · High · Sync tracker state into documents
<!-- /agent:jira -->
```

With no `query=`, the GitHub component uses the "related to me" default
(Decisions 8 and 9). The `Hidden` item holds rows the operator hid
([Hidden rows](#hidden-rows)).

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
  queue controls `go` / `start` / `stop` and `subagents` (Decision 6), the
  feed attributes `feed-run=` / `feed-todo=` (Decisions 7 and 11), and
  `hidden=` (the title of the hidden-rows item, default `Hidden`, Decision 13).
  `query=` is used verbatim: the provider's "related to me" default
  (Decisions 8, 9 and 12) applies only when `query=` is absent.
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
  narrower document-level override above frontmatter (Decision 10), so two
  components in one document can use different profiles.
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
  plus the command's identity, so a retry never posts a comment twice. Until
  provider-native idempotency keys are confirmed, this ledger plus a provider
  re-read before any retry is the accepted guard (Decision 14): after an
  ambiguous failure (a timeout, a dropped connection), the binary reads the
  item back and retries only when the read shows the write did not land.
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
- A hidden item ([Hidden rows](#hidden-rows)) produces no new feed entries.
  Entries fed before it was hidden keep their own lifecycle.
- A fed queue head is ordinary queue work: it runs only when that queue is
  active (`go` / `start`), so the queue's own controls stay the authorization
  gate.
- When the provider shows the condition cleared (CI green, review submitted),
  the provider is the source of truth: a fed todo is checked off, and a fed
  queue head that has not started is struck as `resolved upstream`. A head that
  a session or subagent has already claimed is left to its owner.

## Hidden rows

Deleting a rendered row hides that item (Decision 5). The hide is kept in the
document itself, as one ordinary list item whose nested sub-items are the
hidden rows (Decision 13):

```markdown
## PRs <!-- agent:github interval=5m -->
- 🟢 [gh:btakita/agent-doc#412] Fold nested sub-prompts into their parent — review requested · CI passing
- Hidden
  - [gh:btakita/agent-doc#398] Bump the release checklist
  - [gh:btakita/agent-doc#371] Draft: editor parity notes
<!-- /agent:github -->
```

- **Storage.** No frontmatter key, component attribute or sidecar records a
  hide. The hidden-rows item is plain Markdown, so it survives copy, diff and
  merge like any other text, and the operator folds it with the editor's own
  list folding. agent-doc never writes fold state.
- **The parent item and its title.** agent-doc titles the parent `Hidden`. The
  component's `hidden="<title>"` attribute picks another title. The title is
  the parent's only identity: it has no id token and no marker comment. A
  list item counts as the hidden parent when it carries no item key and its
  text, trimmed and compared case-insensitively, equals the title.
- **Full walk.** Each refresh, and each preflight row-edit capture, parses the
  component body as a Markdown block AST (the tree-sitter-md AST that
  `#statussyncnest` uses) and walks every list item at every depth, in
  document order. Nothing depends on a fixed position:
  - a list item that leads with an item key (after an optional state glyph)
    is that item's row: **hidden** when it sits anywhere under a hidden
    parent, **visible** anywhere else;
  - the nested sub-items of a row are that item's commands (Phase 3), not
    rows. A key-led line inside a command subtree is command text;
  - a list item that matches the title is a hidden parent, at any depth and
    position, so the operator can move it;
  - when one key appears twice, the first copy in document order is the row.
    Later copies are left untouched and reported by `agent-doc tracker
    status`; sync never deletes them.
- **Adding the parent.** agent-doc never creates an empty parent in advance.
  When a hide needs one and the walk found none, agent-doc appends it as the
  last top-level item of the component body. When the walk finds several,
  every one counts as hidden, a new hide goes into the first, and `tracker
  status` reports the duplicates. A parent left with no children is removed by
  the next write.
- **Classifying each key.** The walk is combined with the last-write record
  that Decision 5 already keeps (the last rendered bytes of each row, which
  also note whether the row was visible or hidden):

  | Item in the provider result | Where the walk found its key | State at the last write | Result |
  |---|---|---|---|
  | yes | outside every hidden parent | any | visible: re-rendered in place |
  | yes | under a hidden parent | any | hidden: kept in hidden form |
  | yes | nowhere | visible | the operator deleted the row: hide it, by writing it under the hidden parent |
  | yes | nowhere | hidden | the operator deleted the hidden entry: un-hide it, re-added as a visible row |
  | yes | nowhere | no record | a new item: added as a visible row |
  | no | anywhere | any | the item left the query: its row or hidden entry is removed on the full reconcile that no longer returns it |

  So there are two ways to hide (delete the row, or move it under the hidden
  parent) and two ways to un-hide (move the entry out of the hidden parent,
  or delete the entry). Deleting the whole hidden parent un-hides every item
  in it; nothing is lost, and each one can be deleted again.
- **Hidden form.** A hidden entry is `- [<key>] <title>`: no state glyph and no
  state summary. It is rewritten only when the provider's title changes, so a
  hidden item's CI, review or status changes cause no write and no commit.
  Hidden entries keep the order the operator left them in, and a new hide is
  appended at the end of the hidden list.
- **How an un-hidden row re-renders.** A row moved out of the hidden parent is
  matched by key on the next refresh or preflight and rendered in full form
  (glyph, title, state summary) from the provider's current state. The move is
  classified before the Decision 5 diff runs, so the hidden-form text is never
  read as an edit of the full row and produces no command. A row moved to the
  top level takes its place in the deterministic sort. A row the operator
  moved under some other list item of their own, which carries no key and is
  not a hidden parent, is matched and re-rendered where it is; sync re-orders
  only top-level visible rows.
- **Edits and commands under a hidden entry.** They follow the same rules as
  on a visible row: appended text or a changed title is captured as a command
  (Decision 5), and nested sub-items are commands under the queue rules
  (Decision 6). An item can be hidden and still receive a `comment:`.
- **Feeds.** A hidden item produces no new `feed-run=` / `feed-todo=` entries
  ([Rows as queue and todo work](#rows-as-queue-and-todo-work)).
- **How this fits Decision 5.** "A deleted row hides the item, never closes
  it" is unchanged. The decision here only settles where the hide is stored:
  as the row's new place in the document, not in frontmatter, an attribute or
  a sidecar. Un-hiding is therefore an ordinary edit, and the only binary-side state is the
  last-write record Decision 5 already needs.
- **Trade-off.** Once an item leaves the query, its hidden entry is dropped. If
  the item later matches again (a closed PR is reopened), it comes back
  visible.

## MVP order

1. **GitHub, read-only** (`agent:github`), via `gh api graphql` with the
   existing `gh` auth, scoped to `involves:@me` plus `review-requested:@me` by
   default (Decisions 8 and 9). It needs no new secret handling and is the fastest way to validate the row
   shape, the refresh ladder, the commit rule and the projection path.
2. **Jira, read-only** (`agent:jira`), with a JQL query and the token from the
   secret store. It validates the profile and secret model.
3. **GitLab, read-only** (`agent:gitlab`), with `glab` auth or a token.
4. **Row-edit capture, hidden rows and feeds** (Decisions 5, 7 and 13), still
   read-only at the provider.
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
   time from `passage` or `gh` and are never written into the document.
   Decision 10 adds the component's `auth=` attribute above frontmatter. See
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
     needs an explicit `close` command). Decision 13 says where the hide is
     kept.

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
   a pending review request on its own, so Decision 9 adds one. The GitLab and
   Jira defaults use the closest equivalent each provider offers (Decision 12).

Recorded from the operator's answers to `#pmsyncstillopen` (2026-10-28).

9. **The GitHub default also covers `review-requested:@me`.** The default
   result is `involves:@me` merged with `review-requested:@me`, de-duplicated
   by item key, in one component. Where GitHub's advanced search accepts it,
   the fetcher sends one query,
   `is:open is:pr (involves:@me OR review-requested:@me)`; otherwise it sends
   the two queries and merges them by key. The rendered rows are the same
   either way. See
   [Research: "related to me" on each provider](#research-related-to-me-on-each-provider).
10. **A component `auth=` attribute ranks above frontmatter.** Selection order
    is component `auth=` > frontmatter > project config > global config. See
    [Auth and secret model](#auth-and-secret-model).
11. **The feed names and filters stay as proposed.** `feed-run=` and
    `feed-todo=`, with the filters `needs-me`, `review-requested`,
    `changes-requested`, `ci-failing`, `assigned`, and Jira `status=<name>`.
12. **GitLab and Jira "related to me" defaults follow the research below.**
    GitLab: the union, by key, of MRs I authored, am assigned to, or am a
    reviewer on, issues I authored or am assigned to, and open items behind
    my pending GitLab to-dos for a mention. Jira:
    `(assignee = currentUser() OR reporter = currentUser() OR creator = currentUser() OR watcher = currentUser()) AND resolution = Unresolved`.
    Neither provider can match "I commented" exactly; what each default misses
    is listed in the research section.
13. **A hidden row is kept in the document, as a sub-item of one ordinary list
    item.** Deleting a row hides it (Decision 5); agent-doc moves it under a
    list item it titles `Hidden` (or the `hidden=` title), adding that item
    when it is missing. The operator folds that item in the editor. Sync finds
    rows by a full walk of the component's list tree, so a row is matched
    wherever it sits. Un-hiding is moving the row back out. No frontmatter,
    attribute or sidecar stores a hide. See [Hidden rows](#hidden-rows).
14. **Retries use the local send ledger plus a provider re-read** until
    provider idempotency keys are confirmed. See
    [Phase 3](#phase-3-bidirectional-writes-through-command-sub-items).

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

## Research: "related to me" on each provider

The operator asked for research into the GitLab and Jira equivalents of
`involves:@me`. Checked against the providers' official documentation on
2026-10-28. The GitHub advanced-search behavior for PRs was also checked with
read-only probes the same day.

**GitHub (the reference point).** `involves:USER` is "a logical OR between the
`author`, `assignee`, `mentions`, and `commenter` qualifiers for a single
user". `review-requested:USER` matches PRs where the user is asked for review,
including through a team, and the user drops out of it once they have
reviewed. `user-review-requested:@me` matches only direct requests.
([GitHub search docs](https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests))
GitHub's advanced search accepts `AND` / `OR` and parentheses through REST
(`/search/issues?advanced_search=true`) and GraphQL
(`search(type: ISSUE_ADVANCED)`)
([changelog, 2025-03-06](https://github.blog/changelog/2025-03-06-github-issues-projects-api-support-for-issues-advanced-search-and-more/)).
The changelog describes it only "for issues". A probe showed that it unions
PR results on both APIs: `is:pr (author:a OR author:b)` returned the sum of
the two single-author counts. Without advanced search, the parenthesized query
fails with 422. Because PR support is observed and not documented, Decision 9
keeps the two-query merge as the fallback.

**GitLab: no single "involves" filter.** The documented filters are:

- REST `GET /merge_requests`: `scope` = `created_by_me` (the default),
  `assigned_to_me`, `reviews_for_me` ("the current user is assigned as a
  reviewer") or `all`, plus `author_username`, `assignee_username`,
  `reviewer_username`, `approved_by_usernames[]`, `my_reaction_emoji`,
  `search` (title and description only), `state` and `updated_after`. Filters
  combine with AND. There is no commenter, participant or mention filter.
  ([merge requests API](https://docs.gitlab.com/api/merge_requests/))
- REST `GET /issues`: `scope` = `created_by_me` (the default),
  `assigned_to_me` or `all`, with the same author, assignee and reaction
  filters. There is no reviewer scope and no commenter, participant or mention
  filter. On GitLab.com, `scope=all` on this endpoint can return
  `400 Bad Request` (`restrict_global_issues_scope_all`).
  ([issues API](https://docs.gitlab.com/api/issues/))
- GraphQL: `Query.issues` (experiment since 15.6) and `Query.mergeRequests`
  (experiment since 19.3) take an `or:` argument, "List of arguments with
  inclusive OR". For MRs, `UnionedMergeRequestFilterInput` has
  `assigneeUsernames` and experimental reviewer fields but no author field,
  so author OR assignee OR reviewer cannot be one MR query. For issues,
  `UnionedIssueFilterInput` has `assigneeUsernames` and `authorUsernames`.
  Both queries also take `subscribed: SubscriptionStatus`, whose values are
  `EXPLICITLY_SUBSCRIBED` and `EXPLICITLY_UNSUBSCRIBED`. `CurrentUser` exposes
  `authoredMergeRequests`, `assignedMergeRequests` and
  `reviewRequestedMergeRequests`, each with `state` and `updatedAfter`.
  ([queries](https://docs.gitlab.com/api/graphql/reference/queries/),
  [input types](https://docs.gitlab.com/api/graphql/reference/input_objects/),
  [enums](https://docs.gitlab.com/api/graphql/reference/enums/),
  [objects](https://docs.gitlab.com/api/graphql/reference/objects/))
- To-dos: `GET /todos?state=pending&type=MergeRequest|Issue` with actions that
  include `assigned`, `mentioned`, `directly_addressed`, `review_requested`,
  `approval_required` and `build_failed`.
  ([to-do items API](https://docs.gitlab.com/api/todos/))

**GitLab recommendation (Decision 12).** Make the default the union, merged by
key, of:

1. open MRs with `scope=created_by_me`, `scope=assigned_to_me` and
   `scope=reviews_for_me` (three REST calls, through `glab api` when present);
2. open issues with `scope=created_by_me` and `scope=assigned_to_me`;
3. the open MR or issue behind each pending to-do with action `mentioned` or
   `directly_addressed`.

The REST calls are preferred over the GraphQL `or:` and `Query.mergeRequests`
fields because those are marked experiment. What it misses compared with
`involves:@me`:

- **Items the operator only commented on.** No GitLab list filter covers a
  commenter.
- **Mentions whose to-do is done or gone.** To-dos are an inbox, not history.
  An item drops out of rule 3 when the operator marks its to-do done in GitLab,
  and a mention from before to-dos existed for it is never seen.
- **Approvals and subscriptions.** MRs the operator approved but did not
  author, get assigned or get asked to review on, and items they subscribed
  to. Both are opt-in through `query=` (`approved_by_usernames[]`, GraphQL
  `subscribed`).

**Jira Cloud.** `currentUser()` works with Assignee, Reporter, Voter, Watcher,
Creator and user-type custom fields, with `=` and `!=` only. The `comment`
field supports only text search (`~`, `!~`), so JQL has no "commented by"
field. `updatedBy(user[, dateFrom[, dateTo]])` matches items a user updated,
where an update "includes creating a work item, updating any of the work
item's fields, creating or deleting a comment, or editing a comment (only the
last edit)". It works at day resolution and only with `IN` / `NOT IN`.
`watcher` searches are limited by the "View Voters and Watchers" permission,
except when searching for items you watch yourself.
([JQL functions](https://support.atlassian.com/jira-software-cloud/docs/jql-functions/),
[JQL fields](https://support.atlassian.com/jira-software-cloud/docs/jql-fields/))
With the personal setting "Watch your work items" on, a user automatically
watches every item they create or comment on
([personal settings](https://support.atlassian.com/jira-software-cloud/docs/manage-your-jira-personal-settings/)).
A native `commentedBy` function is an open suggestion
([JRACLOUD-81842](https://jira.atlassian.com/browse/JRACLOUD-81842), status
"Gathering Interest").

**Jira recommendation (Decision 12).**
`(assignee = currentUser() OR reporter = currentUser() OR creator = currentUser() OR watcher = currentUser()) AND resolution = Unresolved`.
The `watcher` clause is the closest equivalent to GitHub's `commenter`,
through auto-watch. What it misses:

- **Comments when auto-watch is off,** or on an item the operator later
  unwatched.
- **Mentions.** No JQL field or function matches a mention.
- **Past involvement.** For example, an item that was once assigned to the
  operator. `updatedBy()` would add items the operator touched, but it also
  matches any field edit, and it is not in the default.

**Not verified, to confirm before implementation:**

- whether GitHub keeps advanced search for PRs (observed, not documented);
- whether GitLab's `UnionedIssueFilterInput` ORs *across* its author and
  assignee fields or only within each list;
- whether commenting on a GitLab item creates an `EXPLICITLY_SUBSCRIBED`
  subscription;
- whether Jira's `updatedBy()` accepts `currentUser()` as its user argument
  (the docs show only literal users);
- the community workaround for Jira mentions,
  `text ~ "\"[~accountid:<id>]\""` (from Atlassian Community, not the docs);
- the Jira Service Management "Request participants" field, which is not in
  the Jira Software JQL field reference.

## Still open

1. **Confirm the GitLab and Jira defaults** recommended in
   [Research: "related to me" on each provider](#research-related-to-me-on-each-provider)
   (Decision 12), in particular the GitLab to-do rule and Jira's reliance on
   auto-watch, along with the unverified points listed there.
2. **Provider-native idempotency and compare-and-set.** Whether GitHub, GitLab
   or Jira accept an idempotency key or a conditional write. This does not
   block Phase 3: until it is verified, the local send ledger plus a provider
   re-read before any retry is the accepted guard (Decision 14), and the
   pre-send stale check covers state verbs
   ([Recommendation: status updates](#recommendation-status-updates)).
3. **The unverified provider facts** listed under
   [Recommendation: refresh cadence](#recommendation-refresh-cadence).
