#!/usr/bin/env python3
"""Reconcile GitHub pull-request status in one Slack message.

Closed or merged pull requests are struck through. Open pull requests whose
active change-request review targets an older commit than the current head are
marked with ``:white_check_mark:`` to show that a follow-up commit is ready for
review.

The Slack token must belong to the author of the message (or otherwise be able
to edit it). By default it is read from ``SLACK_USER_TOKEN``.
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
CHECK_MARK_RE = re.compile(r"(?:^|\s)(?::white_check_mark:|✅)(?=\s|$)")
SLACK_LINK_RE = re.compile(r"/archives/(?P<channel>[A-Z0-9]+)/p(?P<stamp>[0-9]{16})(?:\b|$)")


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


@dataclasses.dataclass(frozen=True)
class ReconcileResult:
    text: str
    closed: tuple[PullRequestRef, ...]
    addressed: tuple[PullRequestRef, ...]
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


def _latest_active_change_request(reviews: Iterable[dict[str, Any]]) -> str | None:
    """Return the commit for the latest still-active change-request review.

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
        return None
    latest = max(change_requests, key=lambda review: str(review.get("submitted_at") or ""))
    return str(latest["commit_id"])


def fetch_pr_status(ref: PullRequestRef) -> PullRequestStatus:
    pull = _gh_json(ref.api_base)
    reviews = _gh_json(f"{ref.api_base}/reviews?per_page=100")
    if not isinstance(pull, dict) or not isinstance(reviews, list):
        raise UpdateError(f"unexpected GitHub response shape for {ref.label}")
    head = pull.get("head") or {}
    head_sha = str(head.get("sha") or "")
    if not head_sha:
        raise UpdateError(f"GitHub response omitted the head SHA for {ref.label}")
    return PullRequestStatus(
        ref=ref,
        state=str(pull.get("state") or "").lower(),
        head_sha=head_sha,
        active_change_request_sha=_latest_active_change_request(reviews),
    )


def fetch_all_statuses(refs: Iterable[PullRequestRef]) -> dict[PullRequestRef, PullRequestStatus]:
    unique_refs = tuple(dict.fromkeys(refs))
    workers = min(8, max(1, len(unique_refs)))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        statuses = executor.map(fetch_pr_status, unique_refs)
        return {status.ref: status for status in statuses}


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


def reconcile_message(
    text: str, statuses: dict[PullRequestRef, PullRequestStatus]
) -> ReconcileResult:
    closed: list[PullRequestRef] = []
    addressed: list[PullRequestRef] = []
    unchanged: list[PullRequestRef] = []
    output_lines: list[str] = []

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
        body = _strip_outer_strike(body).rstrip()
        has_check_mark = CHECK_MARK_RE.search(body) is not None
        line_statuses = [statuses[ref] for ref in refs]

        if all(status.closed for status in line_statuses):
            body = f"~{body}~"
            closed.extend(refs)
        elif (
            any(status.review_addressed_by_commit for status in line_statuses)
            and not has_check_mark
        ):
            body = f"{body} :white_check_mark:"
            addressed.extend(
                status.ref for status in line_statuses if status.review_addressed_by_commit
            )
        else:
            unchanged.extend(refs)
        output_lines.append(f"{prefix}{body}{ending}")

    return ReconcileResult(
        text="".join(output_lines),
        closed=tuple(dict.fromkeys(closed)),
        addressed=tuple(dict.fromkeys(addressed)),
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


def _summary(result: ReconcileResult, changed: bool) -> str:
    def labels(refs: Sequence[PullRequestRef]) -> str:
        return ", ".join(ref.label for ref in refs) or "none"

    return "\n".join(
        (
            f"message changed: {'yes' if changed else 'no'}",
            f"closed/merged: {labels(result.closed)}",
            f"review addressed by newer commit: {labels(result.addressed)}",
        )
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", required=True, help="Slack channel ID, such as C012ABCDEF")
    parser.add_argument(
        "--message",
        required=True,
        help="Slack message timestamp or full permalink",
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
    channel, timestamp = parse_target(args.channel, args.message)
    token = os.environ.get(args.token_env)
    if not token:
        raise UpdateError(f"environment variable {args.token_env!r} is not set")

    original = read_message(token, channel, timestamp)
    refs = pr_refs(original)
    if not refs:
        raise UpdateError("the Slack message contains no GitHub pull-request URLs")
    result = reconcile_message(original, fetch_all_statuses(refs))
    changed = result.text != original
    if args.dry_run:
        print(result.text)
        print("\n---\n" + _summary(result, changed), file=sys.stderr)
    elif changed:
        update_message(token, channel, timestamp, result.text)
        print(_summary(result, changed))
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
