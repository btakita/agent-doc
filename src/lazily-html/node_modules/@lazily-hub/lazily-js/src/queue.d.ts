// Reactive queue: QueueCell + pluggable QueueStorage backend (#lzqueue).
// Pure logic — no reactive graph. See queue.js for the reader-kind invalidation
// contract and the shell / storage split.

export type QueuePushErrorLabel = "Full" | "Closed";
export type QueuePopErrorLabel = "Empty" | "Closed";

export const QueuePushError: Readonly<{ Full: "Full"; Closed: "Closed" }>;
export const QueuePopError: Readonly<{ Empty: "Empty"; Closed: "Closed" }>;

/** The reader-kind invalidation matrix returned by every mutating op. */
export type QueueInvalidates = {
  head: boolean;
  len: boolean;
  is_empty: boolean;
  is_full: boolean;
  closed: boolean;
};

/** Result of a push op (`returns` is the error label, or `null` on success). */
export type QueuePushResult = {
  returns: null | QueuePushErrorLabel;
  invalidates: QueueInvalidates;
};

/** Result of a pop op (`returns` is the element, or the error label). */
export type QueuePopResult = {
  returns: unknown | QueuePopErrorLabel;
  invalidates: QueueInvalidates;
};

/** Result of a close op. */
export type QueueCloseResult = {
  returns: null;
  invalidates: QueueInvalidates;
};

/** A duck-typed `QueueStorage` backend. */
export type QueueStorage = {
  tryPush(value: unknown): null | QueuePushErrorLabel;
  tryPop(): unknown | QueuePopErrorLabel;
  peek(): unknown;
  len(): number;
  capacity(): number | null;
  isClosed(): boolean;
  close(): void;
  snapshot(): QueueStorageSnapshot;
};

export type QueueStorageSnapshot = {
  elements: unknown[];
  capacity: number | null;
  closed: boolean;
};

export type QueueInitial = {
  elements?: unknown[];
  capacity?: number | null;
  closed?: boolean;
};

/** The reference `QueueStorage` backend (unbounded or bounded array FIFO). */
export class VecDequeStorage {
  constructor(initial?: QueueInitial);
  elements: unknown[];
  static from(initial?: QueueInitial): VecDequeStorage;
  tryPush(value: unknown): null | QueuePushErrorLabel;
  tryPop(): unknown | QueuePopErrorLabel;
  peek(): unknown;
  len(): number;
  capacity(): number | null;
  isClosed(): boolean;
  close(): void;
  snapshot(): QueueStorageSnapshot;
}

/** A reactive FIFO queue — SPSC primitive with an MPSC usage rule. */
export class QueueCell {
  constructor(initial?: QueueInitial, storage?: QueueStorage);
  static from(initial?: QueueInitial, storage?: QueueStorage): QueueCell;
  tryPush(value: unknown): QueuePushResult;
  tryPop(): QueuePopResult;
  close(): QueueCloseResult;
  head(): unknown;
  len(): number;
  isEmpty(): boolean;
  isFull(): boolean;
  isClosed(): boolean;
  capacity(): number | null;
elements(): unknown[];
}

export type TopicDurabilityLabel = "durable" | "ephemeral";
export const TopicDurability: Readonly<{ Durable: "durable"; Ephemeral: "ephemeral" }>;

export type TopicSubscribeOutcomeLabel = "Created" | "Reconnected" | "AlreadyConnected";
export const TopicSubscribeOutcome: Readonly<{
Created: "Created";
Reconnected: "Reconnected";
AlreadyConnected: "AlreadyConnected";
}>;

export type TopicSubscriptionSnapshot = {
cursor: number;
durability: TopicDurabilityLabel;
connected: boolean;
};

export type TopicInitial = {
base_offset?: number;
elements?: unknown[];
subscriptions?: Record<string, TopicSubscriptionSnapshot>;
};

export type TopicMutationResult = {
returns: unknown;
invalidates: Record<string, boolean>;
offset?: number;
};

/** Broadcast log with independent, non-destructive subscriber cursors. */
export class TopicCell {
constructor(initial?: TopicInitial);
static from(initial?: TopicInitial): TopicCell;
subscribe(id: string, durability: TopicDurabilityLabel): TopicMutationResult;
reconnect(id: string): TopicMutationResult;
disconnect(id: string): TopicMutationResult;
publish(value: unknown): TopicMutationResult;
readStream(id: string): unknown[];
read(id: string): unknown;
advance(id: string): TopicMutationResult;
restart(id: string): TopicMutationResult;
gc(): TopicMutationResult;
baseOffset(): number;
endOffset(): number;
elements(): unknown[];
subscription(id: string): TopicSubscriptionSnapshot | null;
subscriptions(): Record<string, TopicSubscriptionSnapshot>;
  snapshot(): Required<TopicInitial>;
}

export type WorkQueueInvalidates = {
  pending_len: boolean;
  is_empty: boolean;
  in_flight_len: boolean;
  dead_letter_len: boolean;
};

export type WorkQueueItem<T = unknown> = {item_id: number; value: T; attempts: number};

export type WorkQueueDelivery<T = unknown> = {
  delivery_id: number;
  item_id: number;
  value: T;
  worker: string;
  attempt: number;
  deadline: number;
};

export type WorkQueueDeadLetter<T = unknown> = {
  item_id: number;
  value: T;
  attempts: number;
  reason: "nack" | "expired";
};

export const WorkQueueDeadLetterReason: Readonly<{Nack: "nack"; Expired: "expired"}>;

/** Pull-based competing-consumer work queue with exclusive delivery leases. */
export class WorkQueueCell<T = unknown> {
  constructor(config: {visibility_timeout: number; max_deliveries: number});
  push(value: T): {returns: number; invalidates: WorkQueueInvalidates};
  claim(worker: string, now: number): {
    returns: WorkQueueDelivery<T> | null;
    invalidates: WorkQueueInvalidates;
  };
  ack(worker: string, deliveryId: number): {returns: boolean; invalidates: WorkQueueInvalidates};
  nack(worker: string, deliveryId: number): {returns: boolean; invalidates: WorkQueueInvalidates};
  reapExpired(now: number): {returns: number; invalidates: WorkQueueInvalidates};
  pendingLen(): number;
  isEmpty(): boolean;
  inFlightLen(): number;
  deadLetterLen(): number;
  pendingItems(): WorkQueueItem<T>[];
  inFlightDeliveries(): WorkQueueDelivery<T>[];
  deadLetterItems(): WorkQueueDeadLetter<T>[];
}
