// Type stubs for @lazily-hub/lazily-html (lite renderer).
// Full JSDoc-typed surface is forthcoming; v0.1 ships minimal any-typed d.ts.

export type Accessor<T> = () => T;
export type Setter<T> = (v: T | ((prev: T) => T)) => void;
export type VNode = {
  __v: true;
  type: string | ((props: any) => any) | typeof Component;
  props: any;
  key: string | number | null;
  ref: ((node: Node | null) => void) | { current: Node | null } | null;
};

export declare function createElement(type: any, props?: any, ...children: any[]): VNode;
export declare const h: typeof createElement;
export declare function Fragment(props: any): any;
export declare function isValidElement(v: any): boolean;
export declare function cloneElement(vnode: VNode, props?: any, ...children: any[]): VNode;
export declare function render(vnode: any, parent: Node): () => void;
export declare function renderToString(vnode: any): string;
export declare function renderToStaticMarkup(vnode: any): string;
export declare function renderToStreamable(vnode: any): {
  html: string;
  snapshot: { version: number; anchors: Record<string, any> };
};
export declare function hydrate(rootEl: Element, view: SyncedView): () => void;

export declare class SyncedView {
  constructor();
  applySnapshot(snapshot: { anchors: Record<string, any> } | Record<string, any>): void;
  applyDelta(delta: unknown): void;
  get(id: string): any;
  set(id: string, value: any): void;
  has(id: string): boolean;
  readonly ctx: any;
}

export declare const tags: any;
export declare function tagsNS(ns: string | null): any;
export declare function attach(dom: Node, ...children: any[]): Node;
export declare function fragment(...children: any[]): DocumentFragment;
export declare function raw(html: string | (() => string)): DocumentFragment;
export declare function setProp(dom: Element, key: string, value: any, isSVG?: boolean): void;

export declare class Component<P = any, S = any> {
  constructor(props?: P);
  props: P;
  state: S;
  context: any;
  setState(patch: Partial<S> | ((s: S) => Partial<S>)): void;
  forceUpdate(): void;
  render(): any;
}

export declare function useState<T>(initial: T | (() => T)): [Accessor<T>, Setter<T>];
export declare const useSignal: typeof useState;
export declare function useReducer<R extends (s: any, a: any) => any, I>(
  reducer: R, initialArg: I, init?: (a: I) => any,
): [Accessor<any>, (a: any) => void];
export declare function useEffect(setup: () => (() => void) | void, deps?: any[]): void;
export declare function useLayoutEffect(setup: () => (() => void) | void, deps?: any[]): void;
export declare function useRef<T>(initial: T): { current: T };
export declare function useMemo<T>(factory: () => T, deps?: any[]): Accessor<T>;
export declare function useCallback<T extends (...a: any[]) => any>(fn: T, deps?: any[]): T;
export declare function useContext<T = any>(context: Context<T>): Accessor<T>;
export declare function useId(): string;
export declare function useDebugValue(value: any, formatter?: (v: any) => any): void;

export interface Context<T> {
  Provider: (props: { value: T; children: any }) => any;
  Consumer: (props: { children: (value: T) => any }) => any;
}
export declare function createContext<T>(defaultValue: T): Context<T>;
export declare function forwardRef<P = any, R = any>(
  render: (props: P, ref: ((n: Node | null) => void) | { current: Node | null }) => any,
): (props: P) => any;
export declare function memo<P = any>(
  component: (props: P) => any,
  equal?: (a: P, b: P) => boolean,
): (props: P) => any;

export declare function root<T>(fn: (dispose: () => void) => T): T;
export declare function effect<T>(fn: (prev: T | undefined) => T, init?: T): void;
export declare function onCleanup(fn: () => void): void;
export declare function untrack<T>(fn: () => T): T;
export declare function getOwner(): any;
export declare function runWithOwner<T>(owner: any, fn: () => T): T;
export declare function mergeProps(...sources: any[]): any;
export declare function source<T>(initial: T): { get: Accessor<T>; set: Setter<T> };
