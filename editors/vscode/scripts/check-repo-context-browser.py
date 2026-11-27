#!/usr/bin/env python3
"""Run the Explorer webview in headless Chromium with the VS Code message API stubbed."""

import argparse
import json
import re
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


EDITOR = Path(__file__).resolve().parents[1]


class Browser:
    def __init__(self, url, session):
        self.url = f"{url}/session/{session}"

    def call(self, endpoint, body=None, method=None):
        return webdriver(self.url + endpoint, body, method)

    def script(self, body, *args):
        return self.call("/execute/sync", {"script": body, "args": args})

    def load(self, state=None):
        panel = (EDITOR / "src/repoContextExplorer.ts").read_text(encoding="utf-8")
        markup = re.search(r"`(<!doctype html>.*?</html>)`", panel, re.S)[1]
        markup = markup.replace("${panel.webview.cspSource}", "'none'")
        markup = markup.replace("${nonce}", "qualification")
        script = (EDITOR / "media/repoContext.js").read_text(encoding="utf-8")
        initial = json.dumps(state).replace("<", "\\u003c")
        bootstrap = f"""
            window.outbound = [];
            window.webviewState = {initial};
            window.acquireVsCodeApi = () => ({{
                getState: () => webviewState,
                setState: state => window.webviewState = state,
                postMessage: message => outbound.push(message),
            }});
            window.deliver = data => window.dispatchEvent(new MessageEvent('message', {{data}}));
        """
        markup = re.sub(
            r'<script[^>]*src="\$\{script\}"[^>]*></script>',
            lambda _: f'<script nonce="qualification">{bootstrap}{script}</script>',
            markup,
        )
        self.call("/url", {"url": "data:text/html;charset=utf-8," + quote(markup)})
        assert self.script("return outbound[0]?.type;") == "ready"

    def deliver(self, message):
        self.script("deliver(arguments[0]);", message)

    def cdp(self, command, params):
        return self.call("/goog/cdp/execute", {"cmd": command, "params": params})


def webdriver(url, body=None, method=None):
    data = None if body is None else json.dumps(body).encode()
    request = Request(url, data=data, method=method, headers={"Content-Type": "application/json"})
    with urlopen(request, timeout=15) as response:
        return json.load(response)["value"]


def publish(browser, revision, results, related, instance="explorer-test"):
    template = json.loads((EDITOR / "media/repoContextView.json").read_text(encoding="utf-8"))
    for group, rows, action in (("results", results, "select"), ("related", related, "openLink")):
        nodes = template["definition"]["nodes"]
        nodes[group]["children"] = [row["id"] for row in rows]
        for row in rows:
            nodes[row["id"]] = {"role": "button", "text": row["name"]}
            template["actions"][row["id"]] = {"activate": action}
    browser.deliver({
        "type": "view",
        "revision": {**template, "version": 1, "view_instance": instance, "revision": revision},
    })


def qualify(browser):
    def row(identity, name):
        return {"id": identity, "file": f"{name}.rs", "line": 1, "endLine": 1, "name": name}

    first, second, third = row("results:a", "alpha"), row("results:b", "beta"), row("results:c", "gamma")
    link, other_link = row("related:a", "reference"), row("related:b", "caller")
    results = [first, second]
    browser.load()
    publish(browser, 1, results, [link])
    browser.deliver({"type": "results", "items": results})
    browser.deliver({"type": "selection", "item": first, "links": [link]})
    browser.script("""
        window.firstRow = document.querySelector('#results li');
        window.firstLink = document.querySelector('#related li');
        window.draft = document.getElementById('question');
        window.composition = [];
        for (const type of ['compositionstart', 'compositionupdate', 'compositionend']) {
            draft.addEventListener(type, event => composition.push(event.type));
        }
        draft.value = 'draft';
        draft.focus();
        draft.setSelectionRange(2, 2);
    """)
    browser.cdp("Input.imeSetComposition", {"text": "入力", "selectionStart": 1, "selectionEnd": 1})
    draft_state = """
        return {focused: document.activeElement === draft, text: draft.value,
                caret: [draft.selectionStart, draft.selectionEnd], composition: composition.slice()};
    """
    before = browser.script(draft_state)
    assert before == {"focused": True, "text": "dr入力aft", "caret": [3, 3],
                      "composition": ["compositionstart", "compositionupdate"]}, before

    browser.deliver({"type": "results", "items": [], "loading": True})
    publish(browser, 2, [second, first, third], [other_link, link])
    browser.deliver({"type": "results", "items": [first, second, third]})
    browser.deliver({"type": "selection", "item": first, "links": [link, other_link]})
    browser.deliver({"type": "answer", "body": "A result arrived.", "source": first})
    publish(browser, 1, results, [link])
    assert browser.script(draft_state) == before
    assert browser.script("""
        return document.querySelectorAll('#results li')[1] === firstRow &&
               document.querySelectorAll('#related li')[1] === firstLink &&
               document.querySelector('#results li button').innerText.startsWith('beta');
    """)
    browser.script("document.getElementById('ask-form').requestSubmit();")
    assert browser.script("return outbound.length;") == 1

    browser.cdp("Input.insertText", {"text": "入力"})
    assert browser.script("return composition.at(-1);") == "compositionend"
    browser.script("document.getElementById('ask-form').requestSubmit();")
    frame = browser.script("return outbound.at(-1);")
    assert frame["type"] == "view-event", frame
    pending = frame["frame"]
    assert pending["displayed_revision"] == 2 and pending["node_id"] == "submit-question", pending
    assert pending["event"] == "submit" and pending["payload"] == "dr入力aft", pending
    assert browser.script("return outbound.length;") == 2
    state = browser.script("return webviewState;")
    browser.script("draft.value = 'a later draft';")
    browser.deliver({"type": "asked", "event_id": pending["event_id"], "file": "sample.md"})
    assert browser.script("return draft.value;") == "a later draft"
    browser.deliver({"type": "settled", "event_id": pending["event_id"]})
    browser.script("firstRow.querySelector('button').click();")
    activation = browser.script("return outbound.at(-1).frame;")
    assert activation["node_id"] == first["id"] and activation["displayed_revision"] == 2, activation
    assert activation["event"] == "activate" and activation["payload"] is None, activation

    browser.load(state)
    publish(browser, 3, [second, first], [link])
    assert browser.script("return outbound.slice(1);") == [frame]
    browser.deliver({"type": "settled", "event_id": "a-different-event"})
    assert browser.script("return webviewState.pendingViewEvent;") == pending
    browser.deliver({"type": "settled", "event_id": pending["event_id"]})
    publish(browser, 3, [second, first], [link])
    assert browser.script("return webviewState.pendingViewEvent === null && outbound.length === 2;")

    browser.load(state)
    publish(browser, 1, results, [], instance="different-explorer")
    assert browser.script("return webviewState.pendingViewEvent === null && outbound.length === 1;")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser", default="chromium")
    parser.add_argument("--driver", default="chromedriver")
    args = parser.parse_args()
    binary = shutil.which(args.browser)
    if not binary or not shutil.which(args.driver):
        parser.error("Chromium and its matching ChromeDriver are required")
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryDirectory(prefix="agent-doc-explorer-browser-") as profile:
        with subprocess.Popen([args.driver, f"--port={port}"], stdout=subprocess.DEVNULL) as driver:
            try:
                for _ in range(100):
                    try:
                        webdriver(base + "/status")
                        break
                    except URLError:
                        time.sleep(0.05)
                else:
                    raise TimeoutError("ChromeDriver did not become ready")
                options = {
                    "binary": binary,
                    "args": ["--headless=new", "--disable-dev-shm-usage", f"--user-data-dir={profile}"],
                }
                session = webdriver(base + "/session", {
                    "capabilities": {"alwaysMatch": {"browserName": "chrome", "goog:chromeOptions": options}},
                })
                browser = Browser(base, session["sessionId"])
                try:
                    qualify(browser)
                finally:
                    browser.call("", method="DELETE")
            finally:
                driver.terminate()
    print("Explorer webview: keyed rows, focus, caret, IME, draft preservation and replay passed.")


if __name__ == "__main__":
    main()
