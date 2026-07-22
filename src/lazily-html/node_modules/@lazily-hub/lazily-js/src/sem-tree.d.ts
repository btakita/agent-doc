import type { Context, SlotHandle } from "./reactive.js";

export type TreeNodeSpec = {
  id: string;
  value: unknown;
  children?: { order?: string[]; values?: Record<string, TreeNodeSpec> };
};

export type FoldFn<V, D> = (value: V, childDerived: D[]) => D;

export class SemTree<V = unknown, D = unknown> {
  constructor(ctx: Context, rootSpec: TreeNodeSpec, fold: FoldFn<V, D>);
  static build<V, D>(ctx: Context, rootSpec: TreeNodeSpec, fold: FoldFn<V, D>): SemTree<V, D>;
  setValue(id: string, value: V): void;
  removeChild(parentId: string, childId: string): void;
  value(): D;
  nodeValue(id: string): D | undefined;
  isCached(id: string): boolean;
  rootHandle(): SlotHandle<D>;
  nodeHandle(id: string): SlotHandle<D> | null;
}
