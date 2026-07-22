// Width-ladder load test for the dependency-edge index (#lzspecedgeindex).
//
// WHY THIS EXISTS. The existing bench suites (`context.bench.mjs`,
// `scale.bench.mjs`) scale *node count* and hold fan-out fixed at 2, so the
// width of a single node's dependent list was never a variable — which is
// exactly why an O(n^2) edge dedup could sit in the hot path unnoticed. This
// test makes width the independent variable.
//
// SHAPE. One source cell with `width` subscriber slots reading it. Building
// that fan-out registers `width` dependent edges on one list, so a linear-scan
// dedup costs width^2/2 comparisons to build and the per-subscriber cost grows
// linearly with width. With the index it stays flat.
//
// METHOD: climb, project, refuse. Each rung measures bytes/subscriber, then
// projects the next rung's footprint from *that measurement* and refuses to
// climb if the projection would not leave MEMORY_FLOOR_MB of headroom. This is
// the lazily-rs `examples/pubsub_load.rs` protocol: never allocate a rung you
// have not first shown you can afford.
//
// MANUAL / ON-DEMAND — not part of `make check`. Wide rungs take minutes and
// need a raised V8 heap.
//
//   node --expose-gc --max-old-space-size=8192  bench/edge-index-load.mjs
//   node --expose-gc --max-old-space-size=16384 bench/edge-index-load.mjs --max-width=10000000
//   node --expose-gc --max-old-space-size=8192  bench/edge-index-load.mjs --json
//
// `--expose-gc` is required: bytes/subscriber is meaningless without a
// deterministic collection before each sample.

import { performance } from "node:perf_hooks";
import { getHeapStatistics } from "node:v8";
import { Context } from "../src/reactive.js";

// The ladder. The cluster at 128/159/160/161/192 straddles the promote
// threshold (EDGE_INDEX_PROMOTE = 160): 159 is the last scan-only width, 160
// the first promoted one, 161 the width where a list that oscillates by one
// would thrash a naive promote/demote boundary. A thrash bug is invisible at
// every other rung, so the cluster is the point of the ladder, not padding.
const LADDER = [
  32, 64, 96, 128, 159, 160, 161, 192, 256, 1024, 4096, 65536, 262144, 1048576,
  4194304, 10000000,
];

const MEMORY_FLOOR_MB = 512; // refuse a rung projected to leave less than this

function parseArgs(argv) {
  const opts = { json: false, maxWidth: 1048576 };
  for (const a of argv.slice(2)) {
    if (a === "--json") opts.json = true;
    else if (a.startsWith("--max-width=")) opts.maxWidth = Number(a.slice(12));
  }
  return opts;
}

function gc() {
  if (typeof global.gc !== "function") {
    throw new Error(
      "edge-index-load requires --expose-gc (bytes/subscriber needs a deterministic GC)",
    );
  }
  // Two passes: the first frees the previous rung's graph, the second collects
  // anything the first promoted rather than freed.
  global.gc();
  global.gc();
}

function heapBytes() {
  return process.memoryUsage().heapUsed;
}

// heap_size_limit reflects --max-old-space-size, so the refuse decision is made
// against the heap this process actually has, not a hardcoded budget.
function heapLimitBytes() {
  return getHeapStatistics().heap_size_limit;
}

// Build a width-N fan-out and measure it.
//
// `build` covers creating the subscribers AND forcing their first read, because
// the first read is what runs the tracked compute that registers the dependency
// edge — creating a lazy slot alone registers nothing, so timing creation only
// would measure nothing relevant.
function measureRung(width) {
  gc();
  const before = heapBytes();

  const ctx = Context();
  const source = ctx.cell(0);
  const subs = new Array(width);

  const tBuild0 = performance.now();
  for (let i = 0; i < width; i++) {
    subs[i] = ctx.memo(() => ctx.getCell(source) + 1);
  }
  // Force the first read: this is the edge-registration pass.
  for (let i = 0; i < width; i++) {
    ctx.get(subs[i]);
  }
  const tBuild1 = performance.now();

  gc();
  const after = heapBytes();

  // Notify: one publish, then read every subscriber. Exercises the removal +
  // re-registration path (recomputeSlotNow clears and re-tracks each slot's
  // dependency), which is where an O(n) *removal* would reintroduce O(n^2).
  const tNotify0 = performance.now();
  ctx.setCell(source, 1);
  let observed = 0;
  for (let i = 0; i < width; i++) {
    if (ctx.get(subs[i]) === 2) observed++;
  }
  const tNotify1 = performance.now();

  // Correctness: every survivor must observe the final publish.
  if (observed !== width) {
    throw new Error(
      `width ${width}: only ${observed}/${width} subscribers observed the final publish`,
    );
  }

  const buildNsPerSub = ((tBuild1 - tBuild0) * 1e6) / width;
  const notifyNsPerSub = ((tNotify1 - tNotify0) * 1e6) / width;
  const bytesPerSub = (after - before) / width;

  return { width, buildNsPerSub, notifyNsPerSub, bytesPerSub, observed };
}

// Repeat a rung and take the median. Narrow rungs complete in microseconds, so
// a single run is dominated by timer resolution and by whichever V8 tier the
// code happens to be in; medians over repeated runs are the only stable figure.
// Wide rungs are measured once — they take seconds and cost gigabytes, and
// their per-subscriber cost is large enough to swamp timer noise anyway.
function measureRungMedian(width) {
  const reps = width <= 65536 ? 7 : 1;
  const samples = [];
  for (let i = 0; i < reps; i++) samples.push(measureRung(width));
  const pick = (key) => {
    const vals = samples.map((s) => s[key]).sort((a, b) => a - b);
    return vals[vals.length >> 1];
  };
  return {
    width,
    reps,
    buildNsPerSub: pick("buildNsPerSub"),
    notifyNsPerSub: pick("notifyNsPerSub"),
    bytesPerSub: pick("bytesPerSub"),
    observed: width,
  };
}

// Tier the reactive core up to TurboFan before the first measured rung, so the
// narrow rungs are not measuring the interpreter. Uses a width above the
// promote threshold so BOTH the scan path and the indexed path are warm.
function warmup() {
  for (let i = 0; i < 200; i++) {
    measureRung(256);
  }
}

function main() {
  const opts = parseArgs(process.argv);
  const results = [];
  const refusals = [];

  const limitMB = heapLimitBytes() / (1024 * 1024);
  if (!opts.json) {
    console.log(`node ${process.version}`);
    console.log(`heap limit: ${limitMB.toFixed(0)} MB`);
    console.log(`memory floor: ${MEMORY_FLOOR_MB} MB`);
    console.log("warming up (200 x width-256 build+notify, discarded)...");
  }
  warmup();
  if (!opts.json) console.log("");

  let prev = null;
  for (const width of LADDER) {
    if (width > opts.maxWidth) {
      refusals.push({ width, reason: `above --max-width=${opts.maxWidth}` });
      break;
    }
    // Climb, project, refuse: project this rung from the previous rung's
    // MEASURED bytes/subscriber, not from a constant.
    if (prev !== null) {
      const projectedMB = (prev.bytesPerSub * width) / (1024 * 1024);
      const freeMB = limitMB - projectedMB;
      if (freeMB < MEMORY_FLOOR_MB) {
        refusals.push({
          width,
          reason:
            `projected ${projectedMB.toFixed(0)} MB from measured ` +
            `${prev.bytesPerSub.toFixed(1)} B/sub at width ${prev.width}; ` +
            `would leave ${freeMB.toFixed(0)} MB < ${MEMORY_FLOOR_MB} MB floor`,
        });
        break;
      }
    }

    let r;
    try {
      r = measureRungMedian(width);
    } catch (err) {
      refusals.push({ width, reason: `aborted: ${err.message}` });
      break;
    }
    results.push(r);
    prev = r;
    if (!opts.json) {
      console.log(
        `width ${String(width).padStart(9)}  ` +
          `build ${r.buildNsPerSub.toFixed(1).padStart(9)} ns/sub  ` +
          `notify ${r.notifyNsPerSub.toFixed(1).padStart(8)} ns/sub  ` +
          `${r.bytesPerSub.toFixed(1).padStart(7)} B/sub`,
      );
    }
  }

  const report = assertLadder(results, refusals);

  if (opts.json) {
    console.log(JSON.stringify({ results, refusals, report }, null, 2));
  } else {
    console.log("");
    for (const ref of refusals) {
      console.log(`REFUSED width ${ref.width}: ${ref.reason}`);
    }
    console.log("");
    for (const line of report.lines) console.log(line);
  }

  process.exitCode = report.ok ? 0 : 1;
}

// Assertions, not just prints. Each returns a pass/fail line so a run is
// self-judging.
function assertLadder(results, refusals) {
  const lines = [];
  let ok = true;
  const at = (w) => results.find((r) => r.width === w);

  const check = (label, pass, detail) => {
    if (!pass) ok = false;
    lines.push(`${pass ? "PASS" : "FAIL"}  ${label}${detail ? ` — ${detail}` : ""}`);
  };

  // 1. build ns/sub grows < 2x from 1k -> 1M. This is THE assertion: under the
  //    linear scan this ratio is ~1000x, since per-subscriber cost is O(width).
  const lo = at(1024);
  const hi = at(1048576);
  if (lo && hi) {
    const ratio = hi.buildNsPerSub / lo.buildNsPerSub;
    check(
      "build ns/sub grows <2x from width 1k to 1M",
      ratio < 2,
      `${lo.buildNsPerSub.toFixed(1)} -> ${hi.buildNsPerSub.toFixed(1)} ns/sub = ${ratio.toFixed(2)}x`,
    );
  } else {
    lines.push("SKIP  build 1k->1M growth — ladder did not reach width 1M");
  }

  // 2. bytes/sub flat within ~20% across the ladder (ignore the smallest rungs,
  //    where fixed per-context overhead dominates the per-subscriber figure).
  const wide = results.filter((r) => r.width >= 1024);
  if (wide.length >= 2) {
    const vals = wide.map((r) => r.bytesPerSub);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const spread = (max - min) / min;
    check(
      "bytes/sub flat within 20% across rungs >=1k",
      spread <= 0.2,
      `${min.toFixed(1)}..${max.toFixed(1)} B/sub = ${(spread * 100).toFixed(1)}% spread`,
    );
  } else {
    lines.push("SKIP  bytes/sub flatness — fewer than 2 rungs at width >=1k");
  }

  // 3. every survivor observed the final publish (measureRung throws otherwise,
  //    so reaching here with a full result set is the pass).
  check(
    "every survivor observed the final publish",
    results.every((r) => r.observed === r.width),
    `${results.length} rungs verified`,
  );

  // 4. notify unchanged by the edge-index change: notify ns/sub must not grow
  //    with width either (the removal path is indexed too).
  if (lo && hi) {
    const ratio = hi.notifyNsPerSub / lo.notifyNsPerSub;
    check(
      "notify ns/sub grows <2x from width 1k to 1M",
      ratio < 2,
      `${lo.notifyNsPerSub.toFixed(1)} -> ${hi.notifyNsPerSub.toFixed(1)} ns/sub = ${ratio.toFixed(2)}x`,
    );
  }

  // 5. no regression at the promote boundary: width 161 (just promoted, and the
  //    width a naive promote/demote boundary would thrash) must not cost
  //    materially more per subscriber than width 159 (last scan-only width).
  const below = at(159);
  const above = at(161);
  if (below && above) {
    const ratio = above.notifyNsPerSub / below.notifyNsPerSub;
    check(
      "no demotion thrash at the promote boundary (notify 161 vs 159)",
      ratio < 2,
      `${below.notifyNsPerSub.toFixed(1)} -> ${above.notifyNsPerSub.toFixed(1)} ns/sub = ${ratio.toFixed(2)}x`,
    );
  }

  if (refusals.length > 0) {
    lines.push(
      `NOTE  ladder stopped at width ${results[results.length - 1]?.width}; ` +
        `${refusals.length} rung(s) refused`,
    );
  }
  return { ok, lines };
}

main();
