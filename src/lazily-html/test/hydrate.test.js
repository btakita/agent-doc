// Hydration round-trip: server renderToStreamable → client hydrate → reactive update.
// run: node --test test/hydrate.test.js

import assert from "node:assert/strict";
import test from "node:test";
import { DOMParser, Element, SVGElement, Node, Document, DocumentFragment, Event } from "linkedom";

function freshDocument() {
  const dom = new DOMParser().parseFromString(
    "<!DOCTYPE html><html><head></head><body></body></html>",
    "text/html",
  );
  Object.assign(globalThis, {
    document: dom,
    Element,
    SVGElement,
    Node,
    Document,
    DocumentFragment,
    Event,
    window: globalThis,
  });
  return dom;
}

const { h, renderToStreamable, hydrate, SyncedView } = await import("../src/index.js");

test("counter: server emits data-lzh anchor + snapshot", () => {
  const Counter = () => {
    let count = 0;
    return h("button", { onClick: () => {} }, () => `count: ${count}`);
  };
  // The accessor captures `count` at render time (initial 0) — server emits that.
  const { html, snapshot } = renderToStreamable(h(Counter));
  assert.match(html, /<button[^>]*data-lzh="a1"/);
  assert.match(html, />count: 0<\/button>/);
  assert.deepEqual(snapshot, { version: 1, anchors: { a1: "count: 0" } });
});

test("reactive attribute anchor: class binding", () => {
  const Box = () => h("div", { class: () => "on" }, "content");
  const { html, snapshot } = renderToStreamable(h(Box));
  assert.match(html, /<div class="on" data-lzh="a:class:a1"/);
  assert.equal(snapshot.anchors.a1, "on");
});

test("round-trip: hydrate → view.set patches text", () => {
  freshDocument();
  const Counter = () => h("span", { id: "c" }, () => "0");
  const { html, snapshot } = renderToStreamable(h(Counter));

  // Parse server HTML into the document.
  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.appendChild(root);
  const span = root.querySelector("#c");
  assert.equal(span.textContent, "0");

  // Client: build view, apply snapshot, hydrate.
  const view = new SyncedView();
  view.applySnapshot(snapshot);
  hydrate(root, view);

  // view.set drives the bound effect → textContent patches in place.
  view.set("a1", "42");
  assert.equal(span.textContent, "42");
  // Node identity preserved (resumable — no re-mount).
  assert.equal(span, root.querySelector("#c"));
});

test("round-trip: hydrate → view.set patches attribute", () => {
  freshDocument();
  const Box = () => h("div", { id: "b", class: () => "off" }, "x");
  const { html, snapshot } = renderToStreamable(h(Box));

  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.appendChild(root);
  const div = root.querySelector("#b");
  assert.equal(div.getAttribute("class"), "off");

  const view = new SyncedView();
  view.applySnapshot(snapshot);
  hydrate(root, view);

  view.set("a1", "on");
  assert.equal(div.getAttribute("class"), "on");
  // null/false removes the attribute (use hasAttribute — linkedom returns "" for
  // absent while browsers return null; hasAttribute is correct everywhere).
  view.set("a1", null);
  assert.equal(div.hasAttribute("class"), false);
});

test("round-trip: multiple anchors on one element", () => {
  freshDocument();
  const Cell = () =>
    h("td", { class: () => "active", "data-id": () => "row-7" }, () => "value");
  const { html, snapshot } = renderToStreamable(h(Cell));

  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.appendChild(root);
  const td = root.querySelector("td");
  assert.equal(td.textContent, "value");
  assert.equal(td.getAttribute("class"), "active");
  assert.equal(td.getAttribute("data-id"), "row-7");

  const view = new SyncedView();
  view.applySnapshot(snapshot);
  hydrate(root, view);

  // Each anchor updates independently — fine-grained.
  view.set(snapshot && Object.keys(snapshot.anchors).find((k) => snapshot.anchors[k] === "value"), "updated");
  assert.equal(td.textContent, "updated");
  view.set("a1", "inactive");
  assert.equal(td.getAttribute("class"), "inactive");
});

test("hydrate with empty/missing anchors is a no-op", () => {
  freshDocument();
  const root = document.createElement("div");
  root.innerHTML = "<p>static</p>";
  const view = new SyncedView();
  view.applySnapshot({});
  hydrate(root, view);
  assert.equal(root.querySelector("p").textContent, "static");
});
