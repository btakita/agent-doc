#!/usr/bin/env python3

from __future__ import annotations

import unittest

import update_slack_pr_list as subject


def status(
    number: int,
    *,
    state: str = "open",
    head: str = "head",
    reviewed: str | None = None,
) -> subject.PullRequestStatus:
    ref = subject.PullRequestRef("haiven-dev", "widgets", number)
    return subject.PullRequestStatus(ref, state, head, reviewed)


class ParseTargetTests(unittest.TestCase):
    def test_parses_timestamp(self) -> None:
        self.assertEqual(
            subject.parse_target("C0C8LPQ7NS1", "1791874207.231429"),
            ("C0C8LPQ7NS1", "1791874207.231429"),
        )

    def test_parses_permalink(self) -> None:
        self.assertEqual(
            subject.parse_target(
                "C0C8LPQ7NS1",
                "https://haivengroup.slack.com/archives/C0C8LPQ7NS1/p1791874207231429",
            ),
            ("C0C8LPQ7NS1", "1791874207.231429"),
        )


class ReviewStateTests(unittest.TestCase):
    def test_later_approval_supersedes_change_request(self) -> None:
        reviews = [
            {
                "state": "CHANGES_REQUESTED",
                "submitted_at": "2026-10-13T10:00:00Z",
                "commit_id": "old",
                "user": {"login": "reviewer"},
            },
            {
                "state": "APPROVED",
                "submitted_at": "2026-10-13T11:00:00Z",
                "commit_id": "head",
                "user": {"login": "reviewer"},
            },
        ]
        self.assertIsNone(subject._latest_active_change_request(reviews))

    def test_comment_does_not_supersede_change_request(self) -> None:
        reviews = [
            {
                "state": "CHANGES_REQUESTED",
                "submitted_at": "2026-10-13T10:00:00Z",
                "commit_id": "old",
                "user": {"login": "reviewer"},
            },
            {
                "state": "COMMENTED",
                "submitted_at": "2026-10-13T11:00:00Z",
                "commit_id": "head",
                "user": {"login": "reviewer"},
            },
        ]
        self.assertEqual(subject._latest_active_change_request(reviews), "old")


class ReconcileTests(unittest.TestCase):
    def test_strikes_closed_and_marks_addressed(self) -> None:
        first = status(1, state="closed")
        second = status(2, head="new", reviewed="old")
        third = status(3, head="same", reviewed="same")
        message = "\n".join(
            (
                "1. <https://github.com/haiven-dev/widgets/pull/1|widgets #1> done",
                "2. ~<https://github.com/haiven-dev/widgets/pull/2|widgets #2> reopened~",
                "3. <https://github.com/haiven-dev/widgets/pull/3|widgets #3> :white_check_mark: manually checked",
            )
        )

        result = subject.reconcile_message(
            message, {item.ref: item for item in (first, second, third)}
        )

        self.assertEqual(
            result.text,
            "\n".join(
                (
                    "1. ~<https://github.com/haiven-dev/widgets/pull/1|widgets #1> done~",
                    "2. <https://github.com/haiven-dev/widgets/pull/2|widgets #2> reopened :white_check_mark:",
                    "3. <https://github.com/haiven-dev/widgets/pull/3|widgets #3> :white_check_mark: manually checked",
                )
            ),
        )

    def test_second_pass_is_idempotent(self) -> None:
        item = status(7, head="new", reviewed="old")
        original = (
            "7. <https://github.com/haiven-dev/widgets/pull/7|widgets #7> ready"
        )
        first = subject.reconcile_message(original, {item.ref: item})
        second = subject.reconcile_message(first.text, {item.ref: item})
        self.assertEqual(second.text, first.text)

    def test_commonmark_strike_from_adapter_normalizes_for_slack_api(self) -> None:
        item = status(8, state="closed")
        original = (
            "8. ~~<https://github.com/haiven-dev/widgets/pull/8|widgets #8> done~~"
        )
        result = subject.reconcile_message(original, {item.ref: item})
        self.assertEqual(
            result.text,
            "8. ~<https://github.com/haiven-dev/widgets/pull/8|widgets #8> done~",
        )


if __name__ == "__main__":
    unittest.main()
