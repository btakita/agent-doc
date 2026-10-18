import importlib.machinery
import importlib.util
import json
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

SCRIPT = Path(__file__).parents[1] / "bin" / "agent-doc-dogfood-watch"
loader = importlib.machinery.SourceFileLoader("agent_doc_dogfood_watch", str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
watch = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = watch
loader.exec_module(watch)


def completed(stdout="", stderr="", returncode=0):
    return SimpleNamespace(stdout=stdout, stderr=stderr, returncode=returncode)


def cluster(example="typed invariant", timestamp=90, files=None):
    return {
        "family": "queue lifecycle invariant",
        "severity": "high",
        "count": 1,
        "latest_timestamp": timestamp,
        "files": files or [],
        "examples": [example],
        "recommendation": "inspect durable claim evidence",
    }


class WatchHarness:
    def __init__(
        self, root: Path, docs: list[Path], policies: dict[Path, dict], clusters=None
    ):
        self.root = root
        self.docs = docs
        self.policies = policies
        self.clusters = clusters or [cluster(files=[str(docs[0])])]
        self.calls = []

    def run(self, cmd, cwd=None):
        self.calls.append(cmd)
        if cmd[:3] == ["agent-doc", "dogfood", "policy"]:
            doc = Path(cmd[3]).resolve()
            return completed(json.dumps(self.policies[doc]))
        if cmd[:3] == ["agent-doc", "ops", "summary"]:
            return completed(json.dumps({"bug_clusters": self.clusters}))
        if cmd[:2] == ["agent-doc", "read"]:
            return completed("")
        if cmd[:2] == ["agent-doc", "backlog"]:
            return completed("ok")
        if cmd[:3] == ["agent-doc", "queue", "sync"]:
            return completed("ok")
        raise AssertionError(f"unexpected command: {cmd}")

    def args(self, target=None):
        return Namespace(
            target=target,
            doc=[str(doc) for doc in self.docs],
            all_sessions=False,
            limit=4000,
            min_severity="high",
            exclude_family=[],
            dry_run=False,
        )


class DogfoodWatchTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / ".agent-doc").mkdir()
        self.state = self.root / ".agent-doc" / "dogfood-watch-state.json"
        self.state_patch = mock.patch.object(watch, "STATE_PATH", self.state)
        self.state_patch.start()

    def tearDown(self):
        self.state_patch.stop()
        self.tmp.cleanup()

    def doc(self, name="session.md"):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("# session\n")
        return path

    @staticmethod
    def policy(doc, enabled, target=None, auto=False):
        return {
            "enabled": enabled,
            "source_document": str(doc),
            "target": str(target) if target else None,
            "self_target": target == doc if target else False,
            "auto": auto,
        }

    def scan(self, harness, args=None):
        with (
            mock.patch.object(watch, "run", harness.run),
            mock.patch.object(watch, "find_project_root", return_value=self.root),
        ):
            return watch.scan_once(args or harness.args())

    def test_absent_or_false_policy_does_not_scan_or_file(self):
        doc = self.doc()
        harness = WatchHarness(
            self.root,
            [doc],
            {doc: self.policy(doc, False)},
        )

        self.assertEqual(self.scan(harness), 0)
        self.assertFalse(
            any(call[:2] == ["agent-doc", "backlog"] for call in harness.calls)
        )
        self.assertFalse(
            any(call[:3] == ["agent-doc", "ops", "summary"] for call in harness.calls)
        )

    def test_auto_false_files_backlog_only_to_resolved_path(self):
        doc = self.doc()
        target = self.doc("tasks/bugs.md")
        harness = WatchHarness(
            self.root,
            [doc],
            {doc: self.policy(doc, True, target, auto=False)},
        )

        self.assertEqual(self.scan(harness), 1)
        backlog = next(
            call for call in harness.calls if call[:2] == ["agent-doc", "backlog"]
        )
        self.assertEqual(Path(backlog[2]), target)
        self.assertNotIn(":inbox_tray:", backlog[-1])
        self.assertFalse(
            any(call[:3] == ["agent-doc", "queue", "sync"] for call in harness.calls)
        )

    def test_auto_true_marks_only_generated_item_and_syncs_target_queue(self):
        doc = self.doc()
        target = self.doc("tasks/bugs.md")
        harness = WatchHarness(
            self.root,
            [doc],
            {doc: self.policy(doc, True, target, auto=True)},
        )

        self.assertEqual(self.scan(harness), 1)
        backlog = next(
            call for call in harness.calls if call[:2] == ["agent-doc", "backlog"]
        )
        self.assertIn(":inbox_tray:", backlog[-1])
        self.assertIn(
            ["agent-doc", "queue", "sync", str(target)],
            harness.calls,
        )

    def test_cluster_file_attribution_does_not_route_through_unrelated_session(self):
        enabled = self.doc("enabled.md")
        disabled = self.doc("disabled.md")
        target = self.doc("tasks/bugs.md")
        harness = WatchHarness(
            self.root,
            [enabled, disabled],
            {
                enabled: self.policy(enabled, True, target),
                disabled: self.policy(disabled, False),
            },
            clusters=[cluster(files=[str(disabled)])],
        )

        self.assertEqual(self.scan(harness), 0)
        self.assertFalse(
            any(call[:2] == ["agent-doc", "backlog"] for call in harness.calls)
        )

    def test_self_target_write_window_does_not_refire_on_its_own_event(self):
        doc = self.doc()
        harness = WatchHarness(
            self.root,
            [doc],
            {doc: self.policy(doc, True, doc, auto=True)},
            clusters=[cluster("original invariant", 99, [str(doc)])],
        )
        with (
            mock.patch.object(watch.time, "time", return_value=100),
            mock.patch.object(watch, "run", harness.run),
            mock.patch.object(watch, "find_project_root", return_value=self.root),
        ):
            self.assertEqual(watch.scan_once(harness.args()), 1)
            harness.clusters = [cluster("watcher mutation artifact", 100, [str(doc)])]
            self.assertEqual(watch.scan_once(harness.args()), 0)

        backlog_calls = [
            call for call in harness.calls if call[:2] == ["agent-doc", "backlog"]
        ]
        self.assertEqual(len(backlog_calls), 1)

    def test_target_flag_overrides_destination_but_never_enables_disabled_source(self):
        doc = self.doc()
        configured = self.doc("configured.md")
        override = self.doc("override.md")
        harness = WatchHarness(
            self.root,
            [doc],
            {doc: self.policy(doc, True, configured)},
        )

        self.assertEqual(self.scan(harness, harness.args(str(override))), 1)
        backlog = next(
            call for call in harness.calls if call[:2] == ["agent-doc", "backlog"]
        )
        self.assertEqual(Path(backlog[2]), override)

        disabled = WatchHarness(self.root, [doc], {doc: self.policy(doc, False)})
        self.assertEqual(self.scan(disabled, disabled.args(str(override))), 0)
        self.assertFalse(
            any(call[:2] == ["agent-doc", "backlog"] for call in disabled.calls)
        )


if __name__ == "__main__":
    unittest.main()
