// createElement / h / Fragment — Preact-shape VNode factory.
//
// Produces a plain descriptor `{ type, props, key, ref }` (the Preact VNode
// shape). No DOM is touched here; the renderer's `mount(vnode)` walks it. Both
// the lite renderer and the dom-expressions-backed full renderer consume the
// same descriptor, so user code is identical across the two entry points.

let vnodeId = 0;

const RESERVED = new Set(["key", "ref", "children"]);

export function createElement(type, propsConfig, ...children) {
  let props = {};
  let key = null;
  let ref = null;
  if (propsConfig != null) {
    for (const k in propsConfig) {
      const v = propsConfig[k];
      if (k === "key") key = v;
      else if (k === "ref") ref = v;
      else props[k] = v;
    }
  }
  if (children.length === 1) {
    props.children = children[0];
  } else if (children.length > 1) {
    props.children = children;
  }
  return createVNode(type, props, key, ref);
}

export const h = createElement;

export function createVNode(type, props, key, ref) {
  const vnode = {
    __v: true,
    type,
    props,
    key,
    ref,
    _id: ++vnodeId,
  };
  return vnode;
}

export function Fragment(props) {
  return props.children;
}

export function isValidElement(v) {
  return v != null && v.__v === true;
}

export function cloneElement(vnode, props, ...children) {
  const merged = { ...vnode.props, ...props };
  if (children.length === 1) merged.children = children[0];
  else if (children.length > 1) merged.children = children;
  return createVNode(
    vnode.type,
    merged,
    props && props.key != null ? props.key : vnode.key,
    props && props.ref != null ? props.ref : vnode.ref,
  );
}
