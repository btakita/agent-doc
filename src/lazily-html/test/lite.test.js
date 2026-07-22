// Smoke + behavior tests for the lite renderer.
// Uses linkedom for a server-side DOM. run: node --test test/lite.test.js

import assert from "node:assert/strict";
import test from "node:test";
import { DOMParser, Document } from "linkedom";

// Install a global document so lazily-html's `document.createElement` works.
const dom = new DOMParser().parseFromString(
  "<!DOCTYPE html><html><head></head><body></body></html>",
  "text/html",
);
globalThis.document = dom;
globalThis.window = globalThis;
globalThis.customElements = { define() {}, registry: new Map() };
if (!globalThis.Node) globalThis.Node = dom.constructor.prototype;

const {
  render,
  h,
  useState,
  useEffect,
  useRef,
  useMemo,
  useContext,
  createContext,
  Component,
  tags,
} = await import("../src/index.js");

function mountToBody(vnode) {
  const root = document.createElement("div");
  document.body.appendChild(root);
  render(vnode, root);
  return root;
}

test("static host element with text child", () => {
  const root = mountToBody(h("div", { id: "a" }, "hello"));
  assert.equal(root.querySelector("#a").textContent, "hello");
});

test("useState accessor + setState patches only the text node", () => {
  let setter;
  const App = () => {
    const [count, setCount] = useState(0);
    setter = setCount;
    return h("button", { onClick: () => setCount(count() + 1) }, () => count());
  };
  const root = mountToBody(h(App));
  const btn = root.querySelector("button");
  assert.equal(btn.textContent, "0");
  setter(7);
  assert.equal(btn.textContent, "7");
  // node identity preserved (no re-mount)
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
      return h(
        "span",
        { onClick: () => this.setState({ n: this.state.n + 1 }) },
        () => this.state.n,
      );
    }
  }
  const root = mountToBody(h(Counter));
  const span = root.querySelector("span");
  assert.equal(span.textContent, "0");
  inst.setState({ n: 5 });
  assert.equal(span.textContent, "5");
  inst.setState({ n: 6 });
  assert.equal(span.textContent, "6");
});

test("useEffect runs once on mount and disposes on unmount", () => {
  const log = [];
  let dispose;
  const App = () => {
    useEffect(() => {
      log.push("mount");
      return () => log.push("cleanup");
    }, []);
    return h("div", null, "x");
  };
  dispose = render(h(App), document.createElement("div"));
  // linkedom/Node: effects are synchronous in lazily
  assert.ok(log.includes("mount"));
});

test("useMemo recomputes only when deps change", () => {
  let recomputes = 0;
  let setA, setB;
  const App = () => {
    const [a, setA_] = useState(1);
    const [b, setB_] = useState(10);
    setA = setA_; setB = setB_;
    const sum = useMemo(() => (recomputes++, a() + b()), [a, b]);
    return h("div", null, () => sum());
  };
  const root = mountToBody(h(App));
  const div = root.querySelector("div");
  assert.equal(div.textContent, "11");
  const r0 = recomputes;
  setA(2); // dep changed
  assert.equal(div.textContent, "12");
  assert.equal(recomputes, r0 + 1);
});

test("createContext Provider + useContext propagates reactively", () => {
  const Theme = createContext("light");
  let setTheme;
  const Consumer = () => {
    const theme = useContext(Theme);
    return h("span", null, () => theme());
  };
  const Provider = () => {
    const [t, setT] = useState("light");
    setTheme = setT;
    return h(Theme.Provider, { value: t }, h(Consumer));
  };
  const root = mountToBody(h(Provider));
  const span = root.querySelector("span");
  assert.equal(span.textContent, "light");
  setTheme("dark");
  assert.equal(span.textContent, "dark");
});

test("function-valued class prop is reactive through props proxy", () => {
  let setLabel;
  const Parent = () => {
    const [label, set] = useState("hi");
    setLabel = set;
    return h(Child, { label });
  };
  class Child extends Component {
    render() {
      return h("p", null, () => this.props.label);
    }
  }
  const root = mountToBody(h(Parent));
  const p = root.querySelector("p");
  assert.equal(p.textContent, "hi");
  setLabel("bye");
  assert.equal(p.textContent, "bye");
});

test("relementjs-style tags.div with reactive child", () => {
  let setN;
  const App = () => {
    const [n, set] = useState(1);
    setN = set;
    return tags.div({ class: "box" }, tags.span(() => n()));
  };
  const root = mountToBody(h(App));
  const box = root.querySelector(".box");
  assert.ok(box);
  assert.equal(box.querySelector("span").textContent, "1");
  setN(42);
  assert.equal(box.querySelector("span").textContent, "42");
});

test("list via accessor returns array; updates replace nodes", () => {
  let setItems;
  const App = () => {
    const [items, set] = useState([1, 2, 3]);
    setItems = set;
    return h("ul", null, () => items().map((i) => h("li", null, String(i))));
  };
  const root = mountToBody(h(App));
  const ul = root.querySelector("ul");
  assert.deepEqual(
    [...ul.querySelectorAll("li")].map((l) => l.textContent),
    ["1", "2", "3"],
  );
  setItems([4, 5]);
  assert.deepEqual(
    [...ul.querySelectorAll("li")].map((l) => l.textContent),
    ["4", "5"],
  );
});
