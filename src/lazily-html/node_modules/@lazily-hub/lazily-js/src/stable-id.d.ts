export const EDIT_THRESHOLD: 0.5;
export const ANCHOR_PREFIX: "a:";
export const CONTENT_PREFIX: "c:";

export class Block {
  readonly text: string;
  readonly anchor: string | null;
  constructor(text: string, anchor?: string | null);
  static text(text: string): Block;
  static anchored(anchor: string, text: string): Block;
}

export class BlockKey {
  readonly kind: "anchored" | "content";
  readonly value: string | bigint;
  readonly isAnchored: boolean;
  readonly isContent: boolean;
  constructor(kind: "anchored" | "content", value: string | bigint);
  equals(other: BlockKey): boolean;
  asString(): string;
}

export function normalize(text: string): string;
export function contentHash(text: string): bigint;
export function blockKey(block: Block): BlockKey;
export function similarity(a: string, b: string): number;

export class Match {
  readonly kind: "same" | "edited" | "inserted";
  readonly oldIndex: number;
  readonly similarity: number;
  constructor(kind: "same" | "edited" | "inserted", oldIndex?: number, similarity?: number);
  static same(oldIndex: number): Match;
  static edited(oldIndex: number, similarity: number): Match;
  static inserted(): Match;
}

export class Alignment {
  readonly newMatches: Match[];
  readonly removed: number[];
  constructor(newMatches: Match[], removed: number[]);
}

export function align(oldBlocks: Block[], newBlocks: Block[]): Alignment;
export function assignStableKeys(oldBlocks: Block[], newBlocks: Block[]): string[];
