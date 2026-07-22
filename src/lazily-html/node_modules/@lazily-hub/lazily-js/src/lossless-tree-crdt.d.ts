// Lossless full-document tree CRDT — M1 syntax-agnostic core (#lzlosstree).
// Native JS port of lazily-rs::lossless_tree_crdt. See lossless-tree-crdt.js.

export type TreeOpId = { counter: number; peer: number };
export type TreeNodeId = TreeOpId;
export type SortKey = { frac: number[]; peer: number };

export type LeafKindName = "Token" | "Trivia" | "Raw" | "Error";
export const LeafKind: Readonly<Record<LeafKindName, LeafKindName>>;
export const ROOT: Readonly<TreeNodeId>;

export type NodeSeed =
  | { type: "element"; kind: string }
  | { type: "leaf"; leafKind: LeafKindName; text: string };

/** A batch of ops — the output of `diff`, the input to `applyUpdate`. */
export type TreeUpdate = { ops: TreeOp[] };
export type TreeOp = { id: TreeOpId; kind: unknown };

/** A dotted, non-contiguous version frontier. */
export class TreeVersionFrontier {
  contains(id: TreeOpId): boolean;
  observe(id: TreeOpId): void;
  copy(): TreeVersionFrontier;
}

/** A lossless concrete-syntax tree CRDT (M1 core). */
export class LosslessTreeCrdt {
  constructor(peer: number);
  fork(peer: number): LosslessTreeCrdt;
  render(): string;
  liveNodeCount(): number;
  frontier(): TreeVersionFrontier;
  elementKind(node: TreeNodeId): string | null;
  leafKind(node: TreeNodeId): LeafKindName | null;
  children(parent: TreeNodeId): TreeNodeId[];
  leafText(node: TreeNodeId): string;
  createNode(parent: TreeNodeId, after: TreeNodeId | null, seed: NodeSeed): TreeNodeId;
  tombstoneNode(node: TreeNodeId): void;
  reorderChild(node: TreeNodeId, after: TreeNodeId | null): void;
  editLeaf(node: TreeNodeId, atByte: number, deleteBytes: number, insert: string): void;
  splitLeaf(node: TreeNodeId, atByte: number): TreeNodeId;
  mergeAdjacentLeaves(left: TreeNodeId, right: TreeNodeId): void;
  diff(their: TreeVersionFrontier): TreeUpdate;
  applyUpdate(update: TreeUpdate): void;
}

/** A fractional key strictly between `lo` and `hi` (each `null` = open end). */
export function keyBetween(lo: number[] | null, hi: number[] | null): number[];

/** Serialize a TreeUpdate to the externally-tagged lossless-tree-delta.json wire form. */
export function treeUpdateToWire(update: TreeUpdate): unknown;
