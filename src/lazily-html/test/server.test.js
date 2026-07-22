// SSR tests — renderToString produces HTML matching the browser's first paint.
// No DOM global is installed for this suite; renderToString must work in pure JS.
// run: node --test test/server.test.js

import assert from "node:assert/strict";
import test from "node:test";

const { h, renderToString, useState, useMemo, useContext, createContext, Component } =
  await import("../src/index.js");

test("static host element with text child", () => {
  assert.equal(renderToString(h("div", { id: "a" }, "hello")), `<div id="a">hello</div>`);
});

test("void element has no closing tag", () => {
  assert.equal(renderToString(h("img", { src: "/x.png", alt: "x" })), `<img src="/x.png" alt="x">`);
  assert.equal(renderToString(h("br")), `<br>`);
});

test("HTML escaping in text and attributes", () => {
  assert.equal(renderToString(h("p", null, "<script>")), `<p>&lt;script&gt;</p>`);
  assert.equal(
    renderToString(h("div", { "data-q": `he said "hi"` })),
    `<div data-q="he said &quot;hi&quot;"></div>`,
  );
});

test("useState accessor evaluates to initial value", () => {
  const App = () => {
    const [count] = useState(42);
    return h("span", null, () => count());
  };
  assert.equal(renderToString(h(App)), `<span>42</span>`);
});

test("class component renders with state proxy", () => {
  class C extends Component {
    constructor(p) { super(p); this.state = { n: 7 }; }
    render() { return h("b", null, () => this.state.n); }
  }
  assert.equal(renderToString(h(C)), `<b>7</b>`);
});

test("useMemo computes once on server", () => {
  let calls = 0;
  const App = () => {
    const v = useMemo(() => (calls++, "computed"), []);
    return h("i", null, () => v());
  };
  assert.equal(renderToString(h(App)), `<i>computed</i>`);
  assert.equal(calls, 1);
});

test("createContext Provider flows default and provided values", () => {
  const Theme = createContext("light");
  const Consumer = () => {
    const t = useContext(Theme);
    return h("span", null, () => t());
  };
  // Default
  assert.equal(renderToString(h(Consumer)), `<span>light</span>`);
  // Provided
  assert.equal(
    renderToString(h(Theme.Provider, { value: "dark" }, h(Consumer))),
    `<span>dark</span>`,
  );
});

test("nested children + arrays", () => {
  const tree = h("ul", null, [
    h("li", null, "a"),
    h("li", null, "b"),
    h("li", null, "c"),
  ]);
  assert.equal(
    renderToString(tree),
    `<ul><li>a</li><li>b</li><li>c</li></ul>`,
  );
});

test("style object → kebab-case cssText", () => {
  assert.equal(
    renderToString(h("div", { style: { color: "red", fontSize: "12px" } })),
    `<div style="color:red;font-size:12px"></div>`,
  );
});

test("className + classList merge to class attribute", () => {
  assert.equal(
    renderToString(h("div", { className: "x" }, "y")),
    `<div class="x">y</div>`,
  );
  assert.equal(
    renderToString(h("div", { classList: { on: true, off: false } })),
    `<div class="on"></div>`,
  );
});

test("event handlers are omitted on the server", () => {
  assert.equal(
    renderToString(h("button", { onClick: () => {} }, "go")),
    `<button>go</button>`,
  );
});

test("boolean attribute rendering", () => {
  assert.equal(
    renderToString(h("input", { disabled: true, required: false })),
    `<input disabled>`,
  );
});
