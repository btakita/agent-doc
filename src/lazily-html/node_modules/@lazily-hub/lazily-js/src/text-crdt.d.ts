export class OpId {
  readonly counter: number;
  readonly peer: number;
  constructor(counter: number, peer: number);
  compareTo(other: OpId): number;
}

export interface CrdtTree<VersionVector, Delta, Value, Self = unknown> {
  versionVector(): VersionVector;
  deltaSince(theirVersion: VersionVector): Delta;
  applyDelta(delta: Delta): boolean;
  value(): Value;
  mergeFrom(other: Self): boolean;
}

export interface TextOp {
  id: { counter: number; peer: number };
  ch: string;
  origin: { counter: number; peer: number } | null;
  deleted: { counter: number; peer: number } | null;
}

export class TextCrdt implements CrdtTree<Record<number, number>, TextOp[], string, TextCrdt> {
  constructor(peer: number);
  static fromStr(peer: number, str: string): TextCrdt;
  fork(peer: number): TextCrdt;
  clone(): TextCrdt;
  insert(index: number, ch: string): void;
  insertStr(index: number, str: string): void;
  delete(index: number): void;
  text(): string;
  len(): number;
  is_empty(): boolean;
  tombstoneCount(): number;
  clock(): OpId;
  merge(other: TextCrdt): boolean;
  value(): string;
  mergeFrom(other: TextCrdt): boolean;
  versionVector(): Record<number, number>;
  deltaSince(theirVersion: Record<number, number>): TextOp[];
  applyDelta(delta: TextOp[]): boolean;
  gcWith(isStable: (deleteOpId: OpId) => boolean): number;
}
