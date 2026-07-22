// Context, forwardRef, memo — renderer-agnostic.
//
// Context values live as lazily cells attached to component owners; consumers
// walk the owner chain at read time and read the cell inside their tracking
// scope, so `<Provider value={x}>` updates flow straight into the consumer's
// per-expression DOM bindings (no re-render of the consumer).

import { source, getOwner } from "./owner.js";

let nextContextId = 1;

export function createContext(defaultValue) {
  const id = nextContextId++;
  const ctx = { _id: id, _defaultValue: defaultValue, _isContext: true };

  ctx.Provider = function ContextProvider(_props) {
    // Recognized specially by the mount layer (see isProvider): before mounting
    // a provider's children, mount attaches a cell with the provider's value to
    // the child owner so descendants can read it reactively.
    return null;
  };
  ctx.Provider._isProvider = true;
  ctx.Provider._context = ctx;
  ctx.Provider.displayName = "ContextProvider";

  ctx.Consumer = function ContextConsumer(props) {
    const value = readContextAccessor(ctx)();
    const child = props.children;
    return typeof child === "function" ? child(value) : child;
  };
  ctx.Consumer._context = ctx;

  return ctx;
}

export function isContextProvider(type) {
  return typeof type === "function" && type._isProvider === true;
}

export function isForwardRef(type) {
  return typeof type === "function" && type._isForwardRef === true;
}

export function isMemoComponent(type) {
  return typeof type === "function" && type._isMemo === true;
}

// Attach a context cell to an owner (called by the mount layer on Provider).
export function attachContextCell(owner, ctx, value) {
  if (!owner._contextCells) owner._contextCells = new Map();
  const cell = source(value);
  owner._contextCells.set(ctx._id, cell);
  return cell;
}

export function updateContextCell(owner, ctx, value) {
  const cells = owner._contextCells;
  if (cells && cells.has(ctx._id)) {
    cells.get(ctx._id).set(value);
  }
}

// Walk the owner chain to find the cell for ctx; return a reactive accessor.
export function readContextAccessor(ctx) {
  let owner = getOwner();
  while (owner) {
    const cells = owner._contextCells;
    if (cells && cells.has(ctx._id)) {
      const cell = cells.get(ctx._id);
      return () => cell.get();
    }
    owner = owner.parent;
  }
  return () => ctx._defaultValue;
}

// -- forwardRef ------------------------------------------------------------

export function forwardRef(render) {
  const wrapper = function ForwardRefComponent(props) {
    return render(props, props.ref);
  };
  wrapper._isForwardRef = true;
  wrapper._render = render;
  return wrapper;
}

// -- memo ------------------------------------------------------------------
//
// In Option A, component bodies run once per mount, so "skip re-render on equal
// props" is meaningful only for class components (which re-render on setState)
// and for parent-driven prop pushes. We mark the component and let the mount
// layer consult the equality function when applicable.

export function memo(component, equal) {
  const wrapper = function MemoizedComponent(props) {
    return component(props);
  };
  wrapper._isMemo = true;
  wrapper._wrapped = component;
  wrapper._equal = equal || defaultShallowEqual;
  wrapper.displayName = component.displayName || component.name || "Memo";
  return wrapper;
}

function defaultShallowEqual(a, b) {
  if (a === b) return true;
  if (!a || !b) return false;
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (const k of ak) {
    if (!Object.is(a[k], b[k])) return false;
  }
  return true;
}
