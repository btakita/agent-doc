// Component base class — Preact-shape API, Option-A semantics.
//
// `this.state` is a proxy whose property reads hit lazily cells: reading
// `this.state.count` inside a tracked effect (e.g. a `() => this.state.count`
// JSX expression) registers the dependency edge, so `setState({count: v})`
// patches exactly the DOM expressions that read it. The render() body itself
// runs ONCE per mount (#lzdomnoTrackBody) — there is no full re-render on
// setState, matching function-component behavior.
//
// `this.props` is a props proxy that auto-unwraps reactive accessor props on
// read (see makePropsProxy in mount.js), so `<Child foo={count}/>` where
// `count` is a useState getter stays reactive through to the child.

import { source, onCleanup, getOwner } from "./owner.js";
import { withInstance } from "./instance.js";
import { makePropsProxy } from "./mount.js";

export class Component {
  constructor(props) {
    this.props = makePropsProxy(props || {});
    this._stateCells = Object.create(null);
    this._stateSnapshot = {};
    this._stateProxy = null;
    this.context = {};
    this._owner = null;
    this._instance = null;
    this._mounted = false;
    this.refs = {};
  }

  get state() {
    if (!this._stateProxy) {
      this._stateProxy = makeStateProxy(this._stateCells, this._stateSnapshot);
    }
    return this._stateProxy;
  }

  set state(value) {
    const plain = value || {};
    this._stateSnapshot = { ...plain };
    this._stateCells = Object.create(null);
    for (const k in plain) {
      this._stateCells[k] = source(plain[k]);
    }
    this._stateProxy = makeStateProxy(this._stateCells, this._stateSnapshot);
  }

  setState(patch) {
    const p = typeof patch === "function" ? patch(this._stateSnapshot) : patch;
    if (!p) return;
    const prev = { ...this._stateSnapshot };
    for (const k in p) {
      const v = p[k];
      this._stateSnapshot[k] = v;
      if (this._stateCells[k]) {
        this._stateCells[k].set(v);
      } else {
        this._stateCells[k] = source(v);
      }
    }
    if (this._mounted && typeof this.componentDidUpdate === "function") {
      try {
        this.componentDidUpdate(this.props, prev);
      } catch (err) {
        console.error(`lazily-html: componentDidUpdate threw:`, err);
      }
    }
  }

  forceUpdate() {
    // In Option A there is no re-render to force; cells already propagate.
    // Fire componentDidUpdate for symmetry with Preact callers.
    if (this._mounted && typeof this.componentDidUpdate === "function") {
      try {
        this.componentDidUpdate(this.props, { ...this._stateSnapshot });
      } catch (err) {
        console.error(`lazily-html: componentDidUpdate threw:`, err);
      }
    }
  }

  render() {
    return null;
  }
}
Component.prototype.isClassComponent = true;
Component.prototype.isReactComponent = true; // Preact/React parity flag

function makeStateProxy(cells, snapshot) {
  return new Proxy(snapshot, {
    get(target, key, receiver) {
      if (key in cells) return cells[key].get();
      const v = Reflect.get(target, key, receiver);
      return v;
    },
    has(target, key) {
      return key in cells || key in target;
    },
    ownKeys(target) {
      const s = new Set(Reflect.ownKeys(target));
      for (const k in cells) s.add(k);
      return [...s];
    },
    getOwnPropertyDescriptor(target, key) {
      if (key in cells) {
        return { enumerable: true, configurable: true, writable: true, value: cells[key].get() };
      }
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
}

// -- Type predicates -------------------------------------------------------

export function isClassComponent(type) {
  return (
    typeof type === "function" &&
    type.prototype &&
    type.prototype.isClassComponent === true
  );
}

export function isFunctionComponent(type) {
  return typeof type === "function" && !isClassComponent(type);
}

// -- Class component mounting ----------------------------------------------
//
// Called from mount.js once a fresh owner exists for the component. Instantiates
// the class, initializes its hook/state cells, runs render() under withInstance
// (so any hooks the class uses — rare but allowed — dispatch correctly), mounts
// the returned VNode, then fires componentDidMount and wires componentWillUnmount.

export function mountClassInstance(ClassType, props, isSVG, owner, mount) {
  let inst;
  const instance = {
    _hooks: [],
    _hookIndex: 0,
    props,
    _owner: owner,
    _isClassInstance: true,
  };
  owner.component = instance;

  withInstance(instance, () => {
    inst = new ClassType(props);
    inst._owner = owner;
    inst._instance = instance;
    if (inst.state == null) inst.state = {};
  });

  let rendered;
  withInstance(instance, () => {
    try {
      rendered = inst.render();
    } catch (err) {
      console.error(
        `lazily-html: ${(inst.constructor && inst.constructor.name) || "Component"} render threw:`,
        err,
      );
      rendered = null;
    }
  });

  const node = mount(rendered, isSVG);
  inst._mounted = true;

  if (typeof inst.componentDidMount === "function") {
    try {
      inst.componentDidMount();
    } catch (err) {
      console.error(`lazily-html: componentDidMount threw:`, err);
    }
  }

  if (typeof inst.componentWillUnmount === "function") {
    const unmount = () => {
      try {
        inst.componentWillUnmount();
      } catch (err) {
        console.error(`lazily-html: componentWillUnmount threw:`, err);
      }
    };
    const ownerCleanup = onCleanup;
    ownerCleanup(unmount);
  }

  return node;
}
