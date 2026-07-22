// CRDT-plane benchmarks for lazily-js — TextCrdt and SeqCrdt hot paths.
//
// The reactive-core benches (`context.bench.mjs`) do not cover the CRDT plane;
// the Phase 1 plan (#lztextordcache, #lztextinsertchain, #lzopidkeytuple,
// #lzseqstringifyeq) needed a baseline + regression target. Mirrors the
// `bench/harness.mjs` criterion-style API used by `context.bench.mjs` so the
// output table is directly comparable.
//
// Run: `node bench/crdt.bench.mjs`

import { bench, blackBox, run } from "./harness.mjs";
import { TextCrdt } from "../src/text-crdt.js";
import { SeqCrdt } from "../src/seq-crdt.js";

const INSERT_STR_SIZES = [64, 256, 1024];
const REPEATED_READ_ITERS = 100;

// --- TextCrdt: bulk insertStr (#lztextinsertchain is the main lever) -------
for (const size of INSERT_STR_SIZES) {
  const payload = "a".repeat(size);
  bench.batched(
    "textcrdt_insert_str",
    `${size}`,
    () => new TextCrdt(1),
    (crdt) => {
      crdt.insertStr(0, payload);
      blackBox(crdt.len());
    },
  );
}

// --- TextCrdt: repeated text() reads between mutations (#lztextordcache) ---
// One mutation, then N reads. Pre-cache this should be O(N log N) per read;
// post-cache it is O(1) per read after the first.
for (const size of INSERT_STR_SIZES) {
  const payload = "a".repeat(size);
  bench.batched(
    "textcrdt_repeated_text",
    `${size}`,
    () => {
      const crdt = new TextCrdt(1);
      crdt.insertStr(0, payload);
      return crdt;
    },
    (crdt) => {
      let last = "";
      for (let i = 0; i < REPEATED_READ_ITERS; i++) {
        last = crdt.text();
      }
      blackBox(last);
    },
  );
}

// --- TextCrdt: state-based merge ------------------------------------------
// Two peers each insert N chars; merge b into a.
for (const size of INSERT_STR_SIZES) {
  bench.batched(
    "textcrdt_merge",
    `${size}`,
    () => {
      const a = new TextCrdt(1);
      const b = new TextCrdt(2);
      a.insertStr(0, "a".repeat(size));
      b.insertStr(0, "b".repeat(size));
      return { a, b };
    },
    ({ a, b }) => {
      blackBox(a.merge(b));
    },
  );
}

// --- TextCrdt: delta sync round-trip --------------------------------------
for (const size of INSERT_STR_SIZES) {
  bench.batched(
    "textcrdt_delta_sync",
    `${size}`,
    () => {
      const a = new TextCrdt(1);
      a.insertStr(0, "a".repeat(size));
      const b = new TextCrdt(2);
      return { a, b };
    },
    ({ a, b }) => {
      const delta = a.deltaSince({});
      blackBox(b.applyDelta(delta));
    },
  );
}

// --- SeqCrdt: bulk insertBack ---------------------------------------------
for (const size of INSERT_STR_SIZES) {
  bench.batched(
    "seqcrdt_insert_back",
    `${size}`,
    () => new SeqCrdt(1),
    (crdt) => {
      for (let i = 0; i < size; i++) {
        crdt.insertBack(`id${i}`, i, i + 1);
      }
      blackBox(crdt.entryCount());
    },
  );
}

// --- SeqCrdt: merge with primitive values (#lzseqstringifyeq) -------------
for (const size of INSERT_STR_SIZES) {
  bench.batched(
    "seqcrdt_merge",
    `${size}`,
    () => {
      const a = new SeqCrdt(1);
      const b = new SeqCrdt(2);
      for (let i = 0; i < size; i++) {
        a.insertBack(`a${i}`, i, i + 1);
        b.insertBack(`b${i}`, i * 10, i + 1);
      }
      return { a, b };
    },
    ({ a, b }) => {
      blackBox(a.merge(b, Date.now() * 1000));
    },
  );
}

await run({
  format: process.env.BENCH_FORMAT === "json" ? "json" : "markdown",
});
