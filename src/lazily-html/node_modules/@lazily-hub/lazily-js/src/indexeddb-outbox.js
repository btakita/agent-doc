/**
 * Browser IndexedDB storage adapter for the root `Outbox` protocol.
 *
 * `open()` hydrates a synchronous replay mirror. Mutations return promises;
 * callers must await `Outbox.append` before sending and `Outbox.ackThrough`
 * before treating an ack as committed.
 */
export class IndexedDbStore {
  #db;
  #channel;
  #entries;
  #cursor;
  #cursorWrites = Promise.resolve();

  constructor(db, channel, entries, cursor) {
    this.#db = db;
    this.#channel = channel;
    this.#entries = entries;
    this.#cursor = cursor;
  }

  static async open({
    channel,
    database = "lazily-reliable-sync",
    version = 1,
    indexedDB = globalThis.indexedDB,
  }) {
    if (!indexedDB) throw new Error("IndexedDB is unavailable in this environment");
    const db = await new Promise((resolve, reject) => {
      const request = indexedDB.open(database, version);
      request.onupgradeneeded = () => {
        const next = request.result;
        if (!next.objectStoreNames.contains("frames")) {
          next.createObjectStore("frames", { keyPath: ["channel", "epoch"] });
        }
        if (!next.objectStoreNames.contains("cursors")) {
          next.createObjectStore("cursors", { keyPath: "channel" });
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });

    const entries = new Map();
    const cursor = await new Promise((resolve, reject) => {
      const tx = db.transaction(["frames", "cursors"], "readonly");
      const frameRequest = tx.objectStore("frames").openCursor();
      frameRequest.onsuccess = () => {
        const row = frameRequest.result;
        if (!row) return;
        if (row.value.channel === channel) {
          entries.set(row.value.epoch, Uint8Array.from(row.value.frame));
        }
        row.continue();
      };
      const cursorRequest = tx.objectStore("cursors").get(channel);
      tx.oncomplete = () => resolve(cursorRequest.result?.epoch ?? 0);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
    return new IndexedDbStore(db, channel, entries, cursor);
  }

  #transaction(storeName, mode, operation) {
    return new Promise((resolve, reject) => {
      const tx = this.#db.transaction(storeName, mode);
      operation(tx.objectStore(storeName));
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  }

  async put(epoch, frame) {
    const bytes = Uint8Array.from(frame);
    await this.#transaction("frames", "readwrite", (store) => {
      store.put({ channel: this.#channel, epoch, frame: bytes });
    });
    this.#entries.set(epoch, bytes);
  }

  async deleteThrough(epoch) {
    const keys = [...this.#entries.keys()].filter((key) => key <= epoch);
    await this.#transaction("frames", "readwrite", (store) => {
      for (const key of keys) store.delete([this.#channel, key]);
    });
    for (const key of keys) this.#entries.delete(key);
  }

  scanAfter(epoch) {
    return [...this.#entries]
      .filter(([key]) => key > epoch)
      .sort((a, b) => a[0] - b[0])
      .map(([key, frame]) => [key, Uint8Array.from(frame)]);
  }

  loadCursor() {
    return this.#cursor;
  }

  saveCursor(epoch) {
    const previous = this.#cursorWrites.catch(() => undefined);
    const write = previous.then(async () => {
      let next = Math.max(this.#cursor, epoch);
      await this.#transaction("cursors", "readwrite", (store) => {
        // IndexedDB serializes readwrite transactions that overlap this store.
        // Reading and writing inside one transaction makes this a database-wide
        // max, not merely a per-JavaScript-handle max.
        const request = store.get(this.#channel);
        request.onsuccess = () => {
          next = Math.max(request.result?.epoch ?? 0, this.#cursor, epoch);
          store.put({ channel: this.#channel, epoch: next });
        };
      });
      this.#cursor = next;
    });
    this.#cursorWrites = write;
    return write;
  }

  close() {
    this.#db.close();
  }
}
