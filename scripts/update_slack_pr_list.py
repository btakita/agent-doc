#!/usr/bin/env python3
"""Create or reconcile a GitHub pull-request status message in Slack.

Closed or merged pull requests are struck through. Open pull requests are marked
with ``:white_check_mark:`` when approved, ``:eyes:`` when requested changes have
been followed by a newer commit, and ``:warning:`` when requested changes still
target the current head.

Repository owners and repositories may be supplied repeatedly to generate the
list. With ``--message``, the script updates that message; without it, the
script creates a message in ``--channel``. The Slack token must be able to post
or edit the target message and defaults to ``SLACK_USER_TOKEN``.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import dataclasses
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Iterable, Sequence
from typing import Any

GITHUB_PR_RE = re.compile(
    r"https://github\.com/(?P<owner>[A-Za-z0-9_.-]+)/"
    r"(?P<repo>[A-Za-z0-9_.-]+)/pull/(?P<number>[0-9]+)"
)
LINE_PREFIX_RE = re.compile(r"^(?P<prefix>\s*(?:\d+[.)]|[-*])\s+)(?P<body>.*)$")
MANAGED_MARK_RE = re.compile(
    r"(?:^|\s)(?::white_check_mark:|✅|:eyes:|👀|:warning:|⚠️?|:crown:|👑|:link:|🔗)(?=\s|$)"
)
SLACK_LINK_RE = re.compile(r"/archives/(?P<channel>[A-Z0-9]+)/p(?P<stamp>[0-9]{16})(?:\b|$)")

APPROVED_MARK = ":white_check_mark:"
ADDRESSED_MARK = ":eyes:"
UNADDRESSED_MARK = ":warning:"
STACK_HEAD_MARK = ":crown:"
STACK_MEMBER_MARK = ":link:"


class UpdateError(RuntimeError):
    """A recoverable command-line failure with a concise user-facing message."""


@dataclasses.dataclass(frozen=True, order=True)
class PullRequestRef:
    owner: str
    repo: str
    number: int

    @property
    def api_base(self) -> str:
        return f"repos/{self.owner}/{self.repo}/pulls/{self.number}"

    @property
    def label(self) -> str:
        return f"{self.owner}/{self.repo}#{self.number}"


@dataclasses.dataclass(frozen=True)
class PullRequestStatus:
    ref: PullRequestRef
    state: str
    head_sha: str
    active_change_request_sha: str | None
    has_active_approval: bool = False
    base_ref: str = ""
    head_ref: str = ""
    head_repository: str = ""

    @property
    def closed(self) -> bool:
        return self.state != "open"

    @property
    def review_addressed_by_commit(self) -> bool:
        return (
            not self.closed
            and self.active_change_request_sha is not None
            and self.active_change_request_sha != self.head_sha
        )

    @property
    def review_unaddressed(self) -> bool:
        return (
            not self.closed
            and self.active_change_request_sha is not None
            and self.active_change_request_sha == self.head_sha
        )

    @property
    def approved(self) -> bool:
        return (
            not self.closed
            and self.active_change_request_sha is None
            and self.has_active_approval
        )


@dataclasses.dataclass(frozen=True)
class PullRequestListing:
    ref: PullRequestRef
    title: str


@dataclasses.dataclass(frozen=True)
class ReconcileResult:
    text: str
    closed: tuple[PullRequestRef, ...]
    approved: tuple[PullRequestRef, ...]
    addressed: tuple[PullRequestRef, ...]
    unaddressed: tuple[PullRequestRef, ...]
    stack_heads: tuple[PullRequestRef, ...]
    stack_members: tuple[PullRequestRef, ...]
    unchanged: tuple[PullRequestRef, ...]


def pr_refs(text: str) -> tuple[PullRequestRef, ...]:
    return tuple(
        PullRequestRef(
            owner=match.group("owner"),
            repo=match.group("repo"),
            number=int(match.group("number")),
        )
        for match in GITHUB_PR_RE.finditer(text)
    )


def parse_target(channel: str, message: str) -> tuple[str, str]:
    """Normalize a channel ID and a Slack timestamp or permalink."""
    channel_id = channel.strip()
    message_value = message.strip()
    link_match = SLACK_LINK_RE.search(message_value)
    if link_match:
        link_channel = link_match.group("channel")
        if channel_id and channel_id != link_channel:
            raise UpdateError(
                f"channel {channel_id!r} does not match permalink channel {link_channel!r}"
            )
        stamp = link_match.group("stamp")
        return link_channel, f"{stamp[:10]}.{stamp[10:]}"

    if not re.fullmatch(r"[A-Z0-9]+", channel_id):
        raise UpdateError(f"invalid Slack channel ID: {channel_id!r}")
    if re.fullmatch(r"[0-9]{16}", message_value):
        message_value = f"{message_value[:10]}.{message_value[10:]}"
    if not re.fullmatch(r"[0-9]+\.[0-9]+", message_value):
        raise UpdateError(
            "message must be a Slack timestamp (for example 1791874207.231429) "
            "or message permalink"
        )
    return channel_id, message_value


def repository_targets(
    github_users: Iterable[str], repositories: Iterable[str]
) -> tuple[tuple[str, str], ...]:
    """Expand repeatable/comma-separated owners and repository names."""
    users = tuple(
        dict.fromkeys(
            item.strip()
            for value in github_users
            for item in value.split(",")
            if item.strip()
        )
    )
    repository_values = tuple(
        dict.fromkeys(
            item.strip()
            for value in repositories
            for item in value.split(",")
            if item.strip()
        )
    )
    if users and not repository_values:
        raise UpdateError("--github-user requires at least one --repository")

    targets: list[tuple[str, str]] = []
    for value in repository_values:
        if "/" in value:
            owner, repo = value.split("/", 1)
            if not owner or not repo:
                raise UpdateError(f"invalid repository name: {value!r}")
            targets.append((owner, repo))
        else:
            if not users:
                raise UpdateError(
                    f"bare repository {value!r} requires at least one --github-user"
                )
            targets.extend((owner, value) for owner in users)
    return tuple(dict.fromkeys(targets))


def _gh_json(path: str) -> Any:
    completed = subprocess.run(
        ["gh", "api", path],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise UpdateError(f"gh api {path!r} failed: {detail}")
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise UpdateError(f"gh api {path!r} returned invalid JSON: {error}") from error


def _active_review_state(
    reviews: Iterable[dict[str, Any]],
) -> tuple[bool, str | None]:
    """Return active approval and change-request state.

    GitHub retains historical reviews. The latest formal review by each user is
    authoritative, so a later approval or dismissal supersedes that user's
    earlier change request. COMMENTED reviews do not change formal review state.
    """
    latest_by_user: dict[str, dict[str, Any]] = {}
    for review in reviews:
        state = str(review.get("state", "")).upper()
        if state not in {"APPROVED", "CHANGES_REQUESTED", "DISMISSED"}:
            continue
        user = review.get("user") or {}
        login = str(user.get("login") or review.get("user_id") or "")
        if not login:
            continue
        current = latest_by_user.get(login)
        submitted = str(review.get("submitted_at") or "")
        if current is None or submitted >= str(current.get("submitted_at") or ""):
            latest_by_user[login] = review

    change_requests = [
        review
        for review in latest_by_user.values()
        if str(review.get("state", "")).upper() == "CHANGES_REQUESTED"
        and review.get("commit_id")
    ]
    if not change_requests:
        return (
            any(
                str(review.get("state", "")).upper() == "APPROVED"
                for review in latest_by_user.values()
            ),
            None,
        )
    latest = max(change_requests, key=lambda review: str(review.get("submitted_at") or ""))
    return False, str(latest["commit_id"])


def _latest_active_change_request(reviews: Iterable[dict[str, Any]]) -> str | None:
    """Compatibility helper returning the active change-request commit."""
    return _active_review_state(reviews)[1]


def fetch_pr_status(ref: PullRequestRef) -> PullRequestStatus:
    pull = _gh_json(ref.api_base)
    reviews = _gh_json(f"{ref.api_base}/reviews?per_page=100")
    if not isinstance(pull, dict) or not isinstance(reviews, list):
        raise UpdateError(f"unexpected GitHub response shape for {ref.label}")
    head = pull.get("head") or {}
    base = pull.get("base") or {}
    head_sha = str(head.get("sha") or "")
    if not head_sha:
        raise UpdateError(f"GitHub response omitted the head SHA for {ref.label}")
    has_active_approval, active_change_request_sha = _active_review_state(reviews)
    return PullRequestStatus(
        ref=ref,
        state=str(pull.get("state") or "").lower(),
        head_sha=head_sha,
        active_change_request_sha=active_change_request_sha,
        has_active_approval=has_active_approval,
        base_ref=str(base.get("ref") or ""),
        head_ref=str(head.get("ref") or ""),
        head_repository=str((head.get("repo") or {}).get("full_name") or ""),
    )


def fetch_all_statuses(refs: Iterable[PullRequestRef]) -> dict[PullRequestRef, PullRequestStatus]:
    unique_refs = tuple(dict.fromkeys(refs))
    workers = min(8, max(1, len(unique_refs)))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        statuses = executor.map(fetch_pr_status, unique_refs)
        return {status.ref: status for status in statuses}


def fetch_open_pull_requests(
    targets: Iterable[tuple[str, str]],
) -> tuple[PullRequestListing, ...]:
    """Fetch open pull requests, preserving repository argument order."""

    def fetch(target: tuple[str, str]) -> tuple[PullRequestListing, ...]:
        owner, repo = target
        pulls = _gh_json(f"repos/{owner}/{repo}/pulls?state=open&per_page=100")
        if not isinstance(pulls, list):
            raise UpdateError(f"unexpected GitHub response shape for {owner}/{repo}")
        listings: list[PullRequestListing] = []
        for pull in pulls:
            if not isinstance(pull, dict) or not pull.get("number"):
                raise UpdateError(f"unexpected pull-request entry for {owner}/{repo}")
            listings.append(
                PullRequestListing(
                    PullRequestRef(owner, repo, int(pull["number"])),
                    str(pull.get("title") or "(untitled pull request)"),
                )
            )
        return tuple(listings)

    unique_targets = tuple(dict.fromkeys(targets))
    workers = min(8, max(1, len(unique_targets)))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        batches = executor.map(fetch, unique_targets)
        return tuple(item for batch in batches for item in batch)


def _escape_slack_text(value: str) -> str:
    return value.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def augment_message_with_open_pull_requests(
    text: str, listings: Iterable[PullRequestListing]
) -> str:
    """Append newly discovered open pull requests to an existing status list."""
    existing = set(pr_refs(text))
    additions = [listing for listing in listings if listing.ref not in existing]
    if not additions:
        return text

    numbers = []
    for line in text.splitlines():
        match = LINE_PREFIX_RE.match(line)
        if match:
            number_match = re.match(r"\s*(\d+)", match.group("prefix"))
            if number_match:
                numbers.append(int(number_match.group(1)))
    next_number = max(numbers, default=0) + 1
    new_lines = []
    for offset, listing in enumerate(additions):
        ref = listing.ref
        link = f"<https://github.com/{ref.owner}/{ref.repo}/pull/{ref.number}|{ref.repo} #{ref.number}>"
        new_lines.append(
            f"{next_number + offset}. {link} {_escape_slack_text(listing.title)}"
        )
    separator = "\n" if text and not text.endswith("\n") else ""
    joined_lines = "\n".join(new_lines)
    return f"{text}{separator}{joined_lines}"


def _strip_outer_strike(body: str) -> str:
    stripped = body.strip()
    # Slack's Web API stores native mrkdwn with one tilde, while adapters that
    # accept CommonMark may expose the same formatting with two. Accept both so
    # the direct API updater remains idempotent after either kind of editor.
    for marker in ("~~", "~"):
        if (
            len(stripped) >= len(marker) * 2
            and stripped.startswith(marker)
            and stripped.endswith(marker)
        ):
            return stripped[len(marker) : -len(marker)]
    return body


def _strip_review_marks(body: str) -> str:
    """Remove updater-owned status markers while preserving the line text."""
    return re.sub(r"[ \t]+", " ", MANAGED_MARK_RE.sub(" ", body)).strip()


def stack_positions(
    statuses: Iterable[PullRequestStatus],
) -> tuple[set[PullRequestRef], set[PullRequestRef]]:
    """Return stack tips and other members for open same-repository PR chains."""
    open_statuses = tuple(status for status in statuses if not status.closed)
    branch_prs = {
        (status.ref.owner, status.ref.repo, status.head_ref): status.ref
        for status in open_statuses
        if status.head_ref
        and status.head_repository == f"{status.ref.owner}/{status.ref.repo}"
    }
    parent_by_child: dict[PullRequestRef, PullRequestRef] = {}
    for status in open_statuses:
        parent = branch_prs.get((status.ref.owner, status.ref.repo, status.base_ref))
        if parent is not None and parent != status.ref:
            parent_by_child[status.ref] = parent

    members = set(parent_by_child) | set(parent_by_child.values())
    non_heads = set(parent_by_child.values())
    return members - non_heads, non_heads


def reconcile_message(
    text: str, statuses: dict[PullRequestRef, PullRequestStatus]
) -> ReconcileResult:
    closed: list[PullRequestRef] = []
    approved: list[PullRequestRef] = []
    addressed: list[PullRequestRef] = []
    unaddressed: list[PullRequestRef] = []
    stack_heads: list[PullRequestRef] = []
    stack_members: list[PullRequestRef] = []
    unchanged: list[PullRequestRef] = []
    output_lines: list[str] = []
    head_refs, member_refs = stack_positions(statuses.values())

    for line in text.splitlines(keepends=True):
        ending = "\n" if line.endswith("\n") else ""
        content = line[:-1] if ending else line
        refs = pr_refs(content)
        if not refs:
            output_lines.append(line)
            continue
        missing = [ref.label for ref in refs if ref not in statuses]
        if missing:
            raise UpdateError(f"missing GitHub status for {', '.join(missing)}")

        match = LINE_PREFIX_RE.match(content)
        prefix = match.group("prefix") if match else ""
        body = match.group("body") if match else content
        body = _strip_review_marks(_strip_outer_strike(body).rstrip())
        line_statuses = [statuses[ref] for ref in refs]

        status_mark = ""
        if any(status.review_unaddressed for status in line_statuses):
            status_mark = UNADDRESSED_MARK
            unaddressed.extend(
                status.ref for status in line_statuses if status.review_unaddressed
            )
        elif any(status.review_addressed_by_commit for status in line_statuses):
            status_mark = ADDRESSED_MARK
            addressed.extend(
                status.ref for status in line_statuses if status.review_addressed_by_commit
            )
        elif any(status.approved for status in line_statuses):
            status_mark = APPROVED_MARK
            approved.extend(status.ref for status in line_statuses if status.approved)

        stack_mark = ""
        if any(status.ref in head_refs for status in line_statuses):
            stack_mark = STACK_HEAD_MARK
            stack_heads.extend(status.ref for status in line_statuses if status.ref in head_refs)
        elif any(status.ref in member_refs for status in line_statuses):
            stack_mark = STACK_MEMBER_MARK
            stack_members.extend(
                status.ref for status in line_statuses if status.ref in member_refs
            )

        if all(status.closed for status in line_statuses):
            body = f"~{body}~"
            closed.extend(refs)
        else:
            marks = " ".join(mark for mark in (status_mark, stack_mark) if mark)
            if marks:
                body = f"{body} {marks}"
            else:
                unchanged.extend(refs)
        output_lines.append(f"{prefix}{body}{ending}")

    return ReconcileResult(
        text="".join(output_lines),
        closed=tuple(dict.fromkeys(closed)),
        approved=tuple(dict.fromkeys(approved)),
        addressed=tuple(dict.fromkeys(addressed)),
        unaddressed=tuple(dict.fromkeys(unaddressed)),
        stack_heads=tuple(dict.fromkeys(stack_heads)),
        stack_members=tuple(dict.fromkeys(stack_members)),
        unchanged=tuple(dict.fromkeys(unchanged)),
    )


def _slack_request(token: str, method: str, payload: dict[str, Any]) -> dict[str, Any]:
    encoded = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        f"https://slack.com/api/{method}",
        data=encoded,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json; charset=utf-8",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            result = json.load(response)
    except (urllib.error.URLError, TimeoutError) as error:
        raise UpdateError(f"Slack API {method} failed: {error}") from error
    if not isinstance(result, dict) or not result.get("ok"):
        detail = result.get("error", "invalid_response") if isinstance(result, dict) else result
        raise UpdateError(f"Slack API {method} failed: {detail}")
    return result


def read_message(token: str, channel: str, timestamp: str) -> str:
    result = _slack_request(
        token,
        "conversations.replies",
        {"channel": channel, "ts": timestamp, "limit": 1, "inclusive": True},
    )
    messages = result.get("messages") or []
    if not messages or str(messages[0].get("ts")) != timestamp:
        raise UpdateError(f"Slack message {channel}/{timestamp} was not found")
    return str(messages[0].get("text") or "")


def update_message(token: str, channel: str, timestamp: str, text: str) -> None:
    _slack_request(token, "chat.update", {"channel": channel, "ts": timestamp, "text": text})


def create_message(token: str, channel: str, text: str) -> str:
    result = _slack_request(token, "chat.postMessage", {"channel": channel, "text": text})
    timestamp = str(result.get("ts") or "")
    if not timestamp:
        raise UpdateError("Slack chat.postMessage response omitted the message timestamp")
    return timestamp


def _summary(result: ReconcileResult, changed: bool) -> str:
    def labels(refs: Sequence[PullRequestRef]) -> str:
        return ", ".join(ref.label for ref in refs) or "none"

    return "\n".join(
        (
            f"message changed: {'yes' if changed else 'no'}",
            f"closed/merged: {labels(result.closed)}",
            f"approved: {labels(result.approved)}",
            f"review addressed by newer commit: {labels(result.addressed)}",
            f"review changes still unaddressed: {labels(result.unaddressed)}",
            f"PR stack heads: {labels(result.stack_heads)}",
            f"other PR stack members: {labels(result.stack_members)}",
        )
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", default="", help="Slack channel ID, such as C012ABCDEF")
    parser.add_argument(
        "--message",
        default="",
        help="Slack message timestamp or full permalink",
    )
    parser.add_argument(
        "--github-user",
        action="append",
        default=[],
        help="GitHub repository owner; repeat or comma-separate for multiple owners",
    )
    parser.add_argument(
        "--repository",
        action="append",
        default=[],
        help="repository name (with --github-user) or OWNER/REPO; repeat as needed",
    )
    parser.add_argument(
        "--token-env",
        default="SLACK_USER_TOKEN",
        help="environment variable holding an edit-capable Slack token (default: %(default)s)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the reconciled message without updating Slack",
    )
    return parser


def run(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    targets = repository_targets(args.github_user, args.repository)
    token = os.environ.get(args.token_env, "")

    timestamp = ""
    if args.message:
        channel, timestamp = parse_target(args.channel, args.message)
        if not token:
            raise UpdateError(f"environment variable {args.token_env!r} is not set")
        original = read_message(token, channel, timestamp)
    else:
        channel = args.channel.strip()
        if not re.fullmatch(r"[A-Z0-9]+", channel):
            raise UpdateError("--channel is required when --message is omitted")
        if not targets:
            raise UpdateError(
                "creating a message requires --github-user/--repository targets"
            )
        original = ""

    if targets:
        original = augment_message_with_open_pull_requests(
            original, fetch_open_pull_requests(targets)
        )
    refs = pr_refs(original)
    if not refs and targets:
        original = "_No open pull requests._"
    if not refs and not targets:
        raise UpdateError("the Slack message contains no GitHub pull-request URLs")
    result = reconcile_message(original, fetch_all_statuses(refs))
    changed = result.text != original
    if args.dry_run:
        print(result.text)
        print("\n---\n" + _summary(result, changed), file=sys.stderr)
    elif timestamp and changed:
        update_message(token, channel, timestamp, result.text)
        print(_summary(result, changed))
    elif not timestamp:
        if not token:
            raise UpdateError(f"environment variable {args.token_env!r} is not set")
        timestamp = create_message(token, channel, result.text)
        print(f"created message: {channel}/{timestamp}")
        print(_summary(result, True))
    else:
        print(_summary(result, changed))
    return 0


def main() -> int:
    try:
        return run()
    except UpdateError as error:
        print(f"update-slack-pr-list: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
