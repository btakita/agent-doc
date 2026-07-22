export class HlcStamp {
  readonly wallTime: number;
  readonly logical: number;
  readonly peer: number;
  constructor(wallTime: number, logical: number, peer: number);
  compareTo(other: HlcStamp): number;
  static max(a: HlcStamp, b: HlcStamp): HlcStamp;
}

export class Hlc {
  constructor(peer: number);
  send(nowMicros: number): HlcStamp;
  recv(remote: HlcStamp, nowMicros: number): HlcStamp;
}

export class Position {
  readonly frac: number[];
  readonly peer: number;
  constructor(frac: number[], peer: number);
  compareTo(other: Position): number;
}

export class SeqCrdt<Id, V> {
  constructor(peer: number);
  insertBetween(id: Id, value: V, left: Id | null, right: Id | null, nowMicros: number): void;
  insertBack(id: Id, value: V, nowMicros: number): void;
  insertFront(id: Id, value: V, nowMicros: number): void;
  setValue(id: Id, value: V, nowMicros: number): boolean;
  moveBetween(id: Id, left: Id | null, right: Id | null, nowMicros: number): boolean;
  moveAfter(id: Id, anchor: Id, nowMicros: number): boolean;
  moveBefore(id: Id, anchor: Id, nowMicros: number): boolean;
  remove(id: Id, nowMicros: number): boolean;
  contains(id: Id): boolean;
  get(id: Id): V | undefined;
  order(): Id[];
  values(): Array<[Id, V]>;
  tombstoneCount(): number;
  fork(peer: number): SeqCrdt<Id, V>;
  clone(): SeqCrdt<Id, V>;
  merge(other: SeqCrdt<Id, V>, nowMicros: number): boolean;
  gcWith(isStable: (stamp: HlcStamp) => boolean): number;
  gc(watermark: HlcStamp): number;
  entryCount(): number;
}
