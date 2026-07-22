// Manufactured identity for text (cell-model.md § Manufactured identity for
// text, #lzstableid). Markdown has no inherent node ids, so reconciliation keys
// are *manufactured* from text in three layers:
//
// 1. in-band **anchors** (exact, survive a body rewrite),
// 2. **content-derived hashes** of whitespace-normalized text (survive
//    reflow/reorder, change on edit),
// 3. **alignment** by word-LCS similarity (>= 0.5 => Edited/key-inherited;
//    below => Inserted).
//
// This is the bridge to LIS keyed reconciliation (#lzkeyrecon): a new block
// that is `Same`/`Edited` inherits the matched predecessor's key, so the
// reconciler emits an `update` rather than remove+insert.

export const EDIT_THRESHOLD = 0.5;
export const ANCHOR_PREFIX = "a:";
export const CONTENT_PREFIX = "c:";

export class Block {
  constructor(text, anchor = null) {
    this.text = String(text);
    this.anchor = anchor;
    Object.freeze(this);
  }
  static text(text) {
    return new Block(text, null);
  }
  static anchored(anchor, text) {
    return new Block(text, String(anchor));
  }
}

export class BlockKey {
  constructor(kind, value) {
    this.kind = kind; // "anchored" | "content"
    this.value = value; // string (anchor) | bigint (content hash)
    Object.freeze(this);
  }
  get isAnchored() {
    return this.kind === "anchored";
  }
  get isContent() {
    return this.kind === "content";
  }
  equals(other) {
    if (this.kind !== other.kind) {
      return false;
    }
    return this.value === other.value;
  }
  asString() {
    if (this.kind === "anchored") {
      return `${ANCHOR_PREFIX}${this.value}`;
    }
    return `${CONTENT_PREFIX}${this.value.toString(16).padStart(16, "0")}`;
  }
}

// Whitespace normalization: split on any Unicode whitespace run, drop empties,
// rejoin with a single ASCII space. Matches Rust `split_whitespace().join(" ")`.
export function normalize(text) {
  return String(text)
    .split(/\s+/u)
    .filter((token) => token.length > 0)
    .join(" ");
}

// FNV-1a 64-bit content hash of the normalized text. Deterministic and
// cross-language stable (unlike Rust's per-process DefaultHasher); the
// ecosystem already uses FNV-1a-64 for ShmBlobArena checksums. The spec pins
// only "content-derived hashes of normalized text", not the algorithm.
const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK64 = 0xffffffffffffffffn;

export function contentHash(text) {
  let hash = FNV_OFFSET;
  for (const byte of new TextEncoder().encode(normalize(text))) {
    hash ^= BigInt(byte);
    hash = (hash * FNV_PRIME) & MASK64;
  }
  return hash;
}

// Layer dispatch: anchor wins over content (the controlled skeleton's in-band
// marker is the highest-certainty identity and is never overridden by text).
export function blockKey(block) {
  if (block.anchor !== null && block.anchor !== undefined) {
    return new BlockKey("anchored", String(block.anchor));
  }
  return new BlockKey("content", contentHash(block.text));
}

// Longest-common-subsequence length over two token arrays (rolling two-row DP).
function lcsLen(a, b) {
  const dp = new Array(b.length + 1).fill(0);
  for (const x of a) {
    let prev = 0;
    for (let j = 0; j < b.length; j++) {
      const cur = dp[j + 1];
      dp[j + 1] = x === b[j] ? prev + 1 : Math.max(dp[j + 1], dp[j]);
      prev = cur;
    }
  }
  return dp[b.length];
}

// Word-LCS similarity ratio in [0,1]: 2·|LCS| / (|a|+|b|). 1.0 = identical token
// sequence; both-empty => 1.0.
export function similarity(a, b) {
  const aw = String(a).split(/\s+/u).filter((t) => t.length > 0);
  const bw = String(b).split(/\s+/u).filter((t) => t.length > 0);
  if (aw.length === 0 && bw.length === 0) {
    return 1.0;
  }
  const lcs = lcsLen(aw, bw);
  return (2 * lcs) / (aw.length + bw.length);
}

export class Match {
  constructor(kind, oldIndex = -1, similarity = 0) {
    this.kind = kind; // "same" | "edited" | "inserted"
    this.oldIndex = oldIndex;
    this.similarity = similarity;
    Object.freeze(this);
  }
  static same(oldIndex) {
    return new Match("same", oldIndex);
  }
  static edited(oldIndex, similarity) {
    return new Match("edited", oldIndex, similarity);
  }
  static inserted() {
    return new Match("inserted");
  }
}

export class Alignment {
  constructor(newMatches, removed) {
    this.newMatches = newMatches;
    this.removed = removed;
    Object.freeze(this);
  }
}

// Diff two block sequences by manufactured key, then by similarity.
// Pass 1: exact key match, lowest-unused-old-index first (left-to-right so
// duplicate identical blocks pair deterministically).
// Pass 2: word-LCS similarity for the remaining; >= EDIT_THRESHOLD => Edited
// (tiebreak: nearest index), else Inserted.
export function align(oldBlocks, newBlocks) {
  const oldKeys = oldBlocks.map(blockKey);
  const newKeys = newBlocks.map(blockKey);
  const oldUsed = new Array(oldBlocks.length).fill(false);
  const newMatches = new Array(newBlocks.length).fill(null);

  for (let ni = 0; ni < newBlocks.length; ni++) {
    for (let oi = 0; oi < oldBlocks.length; oi++) {
      if (!oldUsed[oi] && newKeys[ni].equals(oldKeys[oi])) {
        oldUsed[oi] = true;
        newMatches[ni] = Match.same(oi);
        break;
      }
    }
  }

  for (let ni = 0; ni < newBlocks.length; ni++) {
    if (newMatches[ni] !== null) {
      continue;
    }
    let bestOi = -1;
    let bestSim = -1;
    for (let oi = 0; oi < oldBlocks.length; oi++) {
      if (oldUsed[oi]) {
        continue;
      }
      const sim = similarity(newBlocks[ni].text, oldBlocks[oi].text);
      const better =
        sim > bestSim ||
        (sim === bestSim && Math.abs(oi - ni) < Math.abs(bestOi - ni));
      if (better) {
        bestSim = sim;
        bestOi = oi;
      }
    }
    if (bestOi !== -1 && bestSim >= EDIT_THRESHOLD) {
      oldUsed[bestOi] = true;
      newMatches[ni] = Match.edited(bestOi, bestSim);
    } else {
      newMatches[ni] = Match.inserted();
    }
  }

  const removed = [];
  for (let oi = 0; oi < oldBlocks.length; oi++) {
    if (!oldUsed[oi]) {
      removed.push(oi);
    }
  }
  return new Alignment(newMatches, removed);
}

// One stable string key per NEW block: Same/Edited inherit the matched old
// block's key; Inserted mints its own. This is what feeds keyed reconciliation.
export function assignStableKeys(oldBlocks, newBlocks) {
  const oldKeyStrings = oldBlocks.map((b) => blockKey(b).asString());
  const alignment = align(oldBlocks, newBlocks);
  return alignment.newMatches.map((m, ni) => {
    if (m.kind === "same" || m.kind === "edited") {
      return oldKeyStrings[m.oldIndex];
    }
    return blockKey(newBlocks[ni]).asString();
  });
}
