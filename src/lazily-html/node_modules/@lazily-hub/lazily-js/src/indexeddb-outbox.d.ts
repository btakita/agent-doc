import type { OutboxStore } from "./index.js";

export interface IndexedDbStoreOptions {
  channel: string;
  database?: string;
  version?: number;
  indexedDB?: IDBFactory;
}

export class IndexedDbStore implements OutboxStore {
  static open(options: IndexedDbStoreOptions): Promise<IndexedDbStore>;
  put(epoch: number, frame: Uint8Array): Promise<void>;
  deleteThrough(epoch: number): Promise<void>;
  scanAfter(cursor: number): Array<[number, Uint8Array]>;
  loadCursor(): number;
  saveCursor(epoch: number): Promise<void>;
  close(): void;
}
