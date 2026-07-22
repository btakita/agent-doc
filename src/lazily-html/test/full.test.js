// Behavior tests for the full (dom-expressions-backed) renderer.
// Mirrors test/lite.test.js to prove both renderers share the same Preact
// surface. run: node --test test/full.test.js

import assert from "node:assert/strict";
import test from "node:test";
import {
  DOMParser,
  Element,
  SVGElement,
  HTMLElement,
  Node,
  Text,
  Comment,
  Event,
  CustomEvent,
  Document,
  DocumentFragment,
  ShadowRoot,
} from "linkedom";

const dom = new DOMParser().parseFromString(
  "<!DOCTYPE html><html><head></head><body></body></html>",
  "text/html",
);
// Expose the full set of DOM globals that dom-expressions' client.js references.
// In a browser these all exist natively; under Node+linkedom we mirror them.
Object.assign(globalThis, {
  document: dom,
  Element,
  SVGElement,
  HTMLElement,
  Node,
  Text,
  Comment,
  Event,
  CustomEvent,
  Document,
  DocumentFragment,
  ShadowRoot,
  window: globalThis,
  customElements: { define() {}, registry: new Map() },
});
globalThis.window.document = dom;

const {
  render,
  h,
  createElement,
  useState,
  useEffect,
  useMemo,
  useContext,
  createContext,
  Component,
} = await import("../src/full/index.js");

function mountToBody(vnode) {
  const root = document.createElement("div");
  document.body.appendChild(root);
  render(vnode, root);
  return root;
}

test("static host element with text child", () => {
  const root = mountToBody(createElement("div", { id: "a" }, "hello"));
  assert.equal(root.querySelector("#a").textContent, "hello");
});

test("useState accessor + setState patches only the text node", () => {
  let setter;
  const App = () => {
    const [count, setCount] = useState(0);
    setter = setCount;
    return createElement("button", { onClick: () => setCount(count() + 1) }, () => count());
  };
  const root = mountToBody(createElement(App));
  const btn = root.querySelector("button");
  assert.equal(btn.textContent, "0");
  setter(7);
  assert.equal(btn.textContent, "7");
  assert.equal(btn, root.querySelector("button"));
});

test("class component: state proxy is reactive via setState", () => {
  let inst;
  class Counter extends Component {
    constructor(props) {
      super(props);
      this.state = { n: 0 };
    }
    render() {
      inst = this;
      return createElement(
        "span",
        { onClick: () => this.setState({ n: this.state.n + 1 }) },
        () => this.state.n,
      );
    }
  }
  const root = mountToBody(createElement(Counter));
  const span = root.querySelector("span");
  assert.equal(span.textContent, "0");
  inst.setState({ n: 5 });
  assert.equal(span.textContent, "5");
});

test("useEffect runs once on mount", () => {
  const log = [];
  const App = () => {
    useEffect(() => {
      log.push("mount");
      return () => log.push("cleanup");
    }, []);
    return createElement("div", null, "x");
  };
  render(createElement(App), document.createElement("div"));
  assert.ok(log.includes("mount"));
});

test("useMemo auto-tracks lazily deps", () => {
  let recomputes = 0;
  let setA;
  const App = () => {
    const [a, setA_] = useState(1);
    setA = setA_;
    const doubled = useMemo(() => (recomputes++, a() * 2), [a]);
    return createElement("div", null, () => doubled());
  };
  const root = mountToBody(createElement(App));
  const div = root.querySelector("div");
  assert.equal(div.textContent, "2");
  const r0 = recomputes;
  setA(5);
  assert.equal(div.textContent, "10");
  assert.equal(recomputes, r0 + 1);
});

test("createContext Provider + useContext propagates reactively", () => {
  const Theme = createContext("light");
  let setTheme;
  const Consumer = () => {
    const theme = useContext(Theme);
    return createElement("span", null, () => theme());
  };
  const Provider = () => {
    const [t, setT] = useState("light");
    setTheme = setT;
    return createElement(Theme.Provider, { value: t }, createElement(Consumer));
  };
  const root = mountToBody(createElement(Provider));
  const span = root.querySelector("span");
  assert.equal(span.textContent, "light");
  setTheme("dark");
  assert.equal(span.textContent, "dark");
});

test("native hyperscript h() with reactive child", () => {
  let setN;
  const App = () => {
    const [n, set] = useState(1);
    setN = set;
    return h("div", { class: "box" }, () => n());
  };
  const root = mountToBody(createElement(App));
  const box = root.querySelector(".box");
  assert.equal(box.textContent, "1");
  setN(42);
  assert.equal(box.textContent, "42");
});
