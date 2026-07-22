// Shared mount logic — renderer-agnostic VNode → DOM walker.
//
// Each renderer (lite / full) calls `makeMount(api)` with a small host API:
//   {
//     isSVGTag(tag)         → bool
//     hostElement(tag,isSVG) → Node
//     assignProps(node, props, isSVG, vnode)  → void   // static + reactive props
//     insertChild(parent, child, marker, isSVG) → void // one child (text/node/array/accessor)
//     insertBetween(parent, children, isSVG)   → void  // multi-child container
//   }
//
// mount() handles: primitive→text, VNode recursion, host-element creation,
// function/class component instantiation, context Provider wiring, ref
// attachment. Component bodies run inside a fresh `root()` (disposal scope)
// and a `withInstance()` (hook dispatch), but OUTSIDE any tracking compute
// (#lzdomnoTrackBody) — only the per-expression effects the renderer creates
// subscribe to lazily cells.

import { root, getOwner, effect } from "./owner.js";
import { withInstance } from "./instance.js";
import { isContextProvider, attachContextCell, updateContextCell } from "./context.js";
import { isClassComponent, mountClassInstance, isFunctionComponent } from "./component.js";

export function makeMount(api) {
  function mount(value, isSVG) {
    if (value == null || value === false || value === true) return null;
    const t = typeof value;
    if (t === "string") return document.createTextNode(value);
    if (t === "number" || t === "bigint") return document.createTextNode(String(value));
    if (t === "function") {
      // Bare accessor at top level — wrap in a tracked fragment anchor.
      const frag = document.createDocumentFragment();
      const marker = document.createTextNode("");
      frag.appendChild(marker);
      effect((cur) => {
        let n = value();
        while (typeof n === "function") n = n();
        n = mount(n, isSVG);
        if (n == null) {
          if (cur && cur.parentNode) cur.remove();
          return null;
        }
        const nodes = Array.isArray(n) ? n : [n];
        if (cur) {
          if (nodes.length === 1 && cur.nodeType === 1) {
            cur.replaceWith(nodes[0]);
          } else {
            cur.replaceWith(...nodes);
            nodes.length = 0; // replaceWith consumed; cur now invalid
          }
        } else {
          marker.before(...nodes);
        }
        return nodes.length === 1 ? nodes[0] : (nodes.length === 0 ? null : nodes);
      }, null);
      return frag;
    }
    if (Array.isArray(value)) {
      const out = [];
      for (const v of value) {
        const n = mount(v, isSVG);
        if (n == null) continue;
        if (Array.isArray(n)) out.push(...n);
        else out.push(n);
      }
      return out;
    }
    if (value.__v) {
      return mountVNode(value, isSVG);
    }
    if (value.nodeType) return value; // already a DOM node
    // Unknown object — coerce to string.
    return document.createTextNode(String(value));
  }

  function mountVNode(vnode, isSVG) {
    const { type, props, ref } = vnode;
    if (typeof type === "function") {
      return mountComponent(type, props || {}, isSVG);
    }
    // Host element.
    const svg = isSVG || api.isSVGTag(type);
    const node = api.hostElement(type, svg);
    if (props) {
      api.assignProps(node, props, svg, vnode);
      const children = props.children;
      if (children != null && children !== false && children !== true) {
        mountHostChildren(node, children, svg);
      }
    }
    if (ref) attachRef(ref, node);
    return node;
  }

  function mountHostChildren(node, children, isSVG) {
    if (Array.isArray(children)) {
      const hasDynamic = children.some(
        (c) => typeof c === "function" || Array.isArray(c),
      );
      if (hasDynamic) {
        const marker = document.createTextNode("");
        node.appendChild(marker);
        for (const c of children) api.insertChild(node, c, marker, isSVG);
      } else {
        for (const c of children) {
          const n = mount(c, isSVG);
          if (n != null) {
            if (Array.isArray(n)) node.append(...n);
            else node.append(n);
          }
        }
      }
    } else if (typeof children === "function") {
      const marker = document.createTextNode("");
      node.appendChild(marker);
      api.insertChild(node, children, marker, isSVG);
    } else {
      const n = mount(children, isSVG);
      if (n != null) {
        if (Array.isArray(n)) node.append(...n);
        else node.append(n);
      }
    }
  }

  function attachRef(ref, node) {
    if (typeof ref === "function") ref(node);
    else if (ref && typeof ref === "object") ref.current = node;
  }

  function mountComponent(type, props, isSVG) {
    const parentOwner = getOwner();
    return root((dispose) => {
      const owner = getOwner();
      owner.dispose = dispose;
      owner.parent = parentOwner;

      if (isContextProvider(type)) {
        const ctx = type._context;
        const initial = unwrapValue(props.value);
        attachContextCell(owner, ctx, initial);
        if (typeof props.value === "function") {
          effect(() => updateContextCell(owner, ctx, props.value()));
        }
        return mountChildren(props.children, isSVG, owner);
      }

      if (isClassComponent(type)) {
        return mountClassInstance(type, props, isSVG, owner, mount);
      }

      // Plain function component (incl. forwardRef/memo wrappers).
      return mountFunctionComponent(type, props, isSVG, owner);
    });
  }

  function mountFunctionComponent(type, props, isSVG, owner) {
    const instance = {
      _hooks: [],
      _hookIndex: 0,
      props,
      _owner: owner,
      _isFunctionInstance: true,
    };
    owner.component = instance;
    let rendered;
    withInstance(instance, () => {
      try {
        rendered = type(makePropsProxy(props));
      } catch (err) {
        reportError(type, err);
        rendered = null;
      }
    });
    return mount(rendered, isSVG);
  }

  function mountChildren(children, isSVG, _owner) {
    if (children == null || children === false || children === true) return null;
    if (Array.isArray(children)) {
      const frag = document.createDocumentFragment();
      const marker = document.createTextNode("");
      frag.appendChild(marker);
      for (const c of children) {
        api.insertChild(frag, c, marker, isSVG);
      }
      // Remove the trailing marker if no reactive children need it. For mixed
      // static/dynamic children we keep it; purely-static fast path removes it.
      if (!children.some((c) => typeof c === "function" || Array.isArray(c))) {
        marker.remove();
      }
      return frag;
    }
    // Single child
    if (typeof children === "function") {
      const frag = document.createDocumentFragment();
      const marker = document.createTextNode("");
      frag.appendChild(marker);
      api.insertChild(frag, children, marker, isSVG);
      return frag;
    }
    return mount(children, isSVG);
  }

  // expose for renderer reuse (e.g. Provider children, class render output)
  mount.mountChildren = mountChildren;
  mount.mountVNode = mountVNode;
  mount.mountComponent = mountComponent;
  return mount;
}

// Unwrap an accessor to a plain value for non-reactive initialization paths.
function unwrapValue(v) {
  let cur = v;
  while (typeof cur === "function") cur = cur();
  return cur;
}

// Props proxy for component instances: function-valued props (except `on*`
// handlers) are treated as reactive accessors and auto-unwrapped on read, so
// `this.props.foo` inside a tracked effect registers the lazily edge.
export function makePropsProxy(props) {
  const known = props != null ? props : {};
  return new Proxy(known, {
    get(target, key) {
      if (key === "children") return target.children;
      const v = target[key];
      if (typeof v === "function" && !String(key).startsWith("on")) {
        return v();
      }
      return v;
    },
    has(target, key) {
      return key in target;
    },
    ownKeys(target) {
      return Reflect.ownKeys(target);
    },
    getOwnPropertyDescriptor(target, key) {
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
}

function reportError(type, err) {
  const name = type && type.displayName || (type && type.name) || "Component";
  console.error(`lazily-html: ${name} threw during render:`, err);
}

export { isFunctionComponent };
