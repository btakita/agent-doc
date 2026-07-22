// Fan-out audit for the effect paths (#lzspecedgeindex).
//
// WHY THIS EXISTS. `bench/edge-index-load.mjs` measures a source cell read by
// `width` computed *slots* — pull-based reads that never enter the effect
// scheduler. Two quadratics found in sibling bindings live on paths that
// harness cannot reach:
//
//   A. edge REMOVAL. The dependents list of a wide source is rebuilt one edge
//      at a time on every dependent recompute. If removal rescans the list,
//      destroy/recompute stays O(n^2) even with an O(1) *registration* index.
//      (zig proved the two do not compose: fixing registration alone left
//      destroy at baseline.)
//   B. pending/scheduled-effect scan. A scan of the pending effect collection
//      for an id that cannot be there, on either the notify or the teardown
//      path. "The queue is empty so the scan is free" is false in general —
//      kt's ArrayDeque.indexOf scanned a never-shrinking backing array.
//
// SHAPE (copied from lazily-rs `examples/edge_audit.rs`). TOTAL WORK IS FIXED:
// every rung creates exactly TOTAL_EFFECTS effects and fires exactly one set
// per source, so each rung performs identical work and differs only in how that
// work is distributed across fan-out width. A rung with width W uses
// TOTAL_EFFECTS/W source cells with W effects each. Per-effect cost must
// therefore be FLAT across the ladder; growth with W is the O(n^2) signature.
// The narrow rung (width 1) is the control: assert against it, not against
// absolute growth.
//
// METHOD — establishing a negative. A flat column alone is equally consistent
// with "no defect" and "blind harness", so each defect is also measured with
// the fix forced back to its naive form behind a declared flag (--arm=...).
// The naive/fixed ratio is the detection margin: it is what proves the harness
// can see the defect it reports absent. Naive arms are produced by rewriting
// `src/reactive.js` (see `naiveSource`) into a temp module, so both arms run in
// one process and can be interleaved.
//
// MEASUREMENT HYGIENE. Other work may be running on the box. Trust ratios, not
// absolute ns: arms are interleaved rung-by-rung so both see the same load, and
// each sample is the median of REPS. The JIT is warmed with WARMUP full
// ladder passes whose results are discarded.
//
//   node --expose-gc --max-old-space-size=8192 bench/effect-edge-audit.mjs
//   node --expose-gc --max-old-space-size=8192 bench/effect-edge-audit.mjs --json
//
// MANUAL / ON-DEMAND — not part of `make check`.

import { performance } from "node:perf_hooks";
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, "..", "src", "reactive.js");

// Total effects held constant across the ladder — every rung does the same work.
const TOTAL_EFFECTS = 65536;
const LADDER = [1, 4, 64, 256, 1024, 4096, 16384, 65536];
const REPS = 5;
const WARMUP = 2;

// ---------------------------------------------------------------------------
// Naive arms: declared source rewrites of the fixes under audit.
// ---------------------------------------------------------------------------

// Defect A naive form: ignore the position index on removal and fall back to
// the linear scan (the pre-#lzspecedgeindex behavior, and the form zig
// reverted to when it proved registration and removal do not compose).
const REMOVE_FIXED = `function edgeRemoveIndexed(edges, id, ownerId, index) {
  const pos = index.get(ownerId);`;
const REMOVE_NAIVE = `function edgeRemoveIndexed(edges, id, ownerId, index) {
  const pos = undefined; /* --arm=naive-remove */`;

// Defect B naive form, arm `naive-splice`: the pre-fix `disposeEffect`, which
// located the id in the pending queue and spliced it out. This is the form the
// fix removed, so this arm is the literal before/after comparison.
const SPLICE_FIXED = `    scheduledEffects.delete(id);
    if (kinds[id] !== KIND_EFFECT) {`;
const SPLICE_NAIVE = `    const idx = pendingEffects.indexOf(id, pendingHead); /* --arm=naive-splice */
    if (idx !== -1) {
      pendingEffects.splice(idx, 1);
    }
    scheduledEffects.delete(id);
    if (kinds[id] !== KIND_EFFECT) {`;

// Defect B, kt shape, arm `naive-nevershrink`: scan the whole pending array
// from 0 rather than from pendingHead, and never reset it after a drain — a
// logically-empty queue whose backing store still holds every id ever
// scheduled, scanned in full for an id that cannot be there. JS does not have
// this shape (Array.prototype.indexOf bounds on length, and the drain resets
// the array), so this arm exists to show what it would cost if it did.
const PENDING_FIXED = `    scheduledEffects.delete(id);
    if (kinds[id] !== KIND_EFFECT) {`;
const PENDING_NAIVE = `    const idx = pendingEffects.indexOf(id); /* --arm=naive-nevershrink */
    if (idx !== -1) {
      pendingEffects.splice(idx, 1);
    }
    scheduledEffects.delete(id);
    if (kinds[id] !== KIND_EFFECT) {`;
const DRAIN_FIXED = `          pendingEffects = [];
          pendingHead = 0;
          return;`;
// `pendingHead--` undoes the increment that consumed the empty slot, leaving
// head == length. Without it the next push lands *behind* the head and is never
// read — the arm would silently stop running effects and its "margin" would be
// measuring a broken graph rather than a slow one. Run-count parity across arms
// is asserted below.
const DRAIN_NAIVE = `          pendingHead--;
          return; /* --arm=naive-nevershrink: never shrink */`;

function applyOnce(src, from, to, label) {
  const i = src.indexOf(from);
  if (i === -1) throw new Error(`naive rewrite failed to match: ${label}`);
  if (src.indexOf(from, i + 1) !== -1)
    throw new Error(`naive rewrite matched more than once: ${label}`);
  return src.slice(0, i) + to + src.slice(i + from.length);
}

function naiveSource(arm) {
  let src = readFileSync(SRC, "utf8");
  if (arm === "naive-remove") {
    src = applyOnce(src, REMOVE_FIXED, REMOVE_NAIVE, "remove");
  } else if (arm === "naive-splice") {
    src = applyOnce(src, SPLICE_FIXED, SPLICE_NAIVE, "dispose-splice");
  } else if (arm === "naive-nevershrink") {
    src = applyOnce(src, PENDING_FIXED, PENDING_NAIVE, "pending-indexOf");
    src = applyOnce(src, DRAIN_FIXED, DRAIN_NAIVE, "pending-drain");
  } else {
    throw new Error(`unknown arm: ${arm}`);
  }
  return src;
}

const TMP = mkdtempSync(join(tmpdir(), "lz-effect-audit-"));

async function loadArm(arm) {
  if (arm === "fixed") return import(pathToFileURL(SRC).href);
  const path = join(TMP, `reactive-${arm}.js`);
  writeFileSync(path, naiveSource(arm));
  return import(pathToFileURL(path).href);
}

// ---------------------------------------------------------------------------
// Workload
// ---------------------------------------------------------------------------

function gc() {
  if (typeof global.gc === "function") {
    global.gc();
    global.gc();
  }
}

// One rung: `groups` sources, `width` effects each, groups*width == TOTAL_EFFECTS.
//
// build    — create the effects. Each effect's first run registers a dependent
//            edge on its source, so a group builds a width-W dependents list.
// notify   — one set per source. Every effect reruns; each rerun CLEARS and
//            RE-REGISTERS its dependency edge, so this column exercises edge
//            removal at width W (defect A) and the scheduler (defect B).
// teardown — dispose every effect. Each dispose removes its edge from the
//            width-W dependents list (defect A) and scans the pending
//            collection (defect B).
function runRung(Context, width) {
  const groups = TOTAL_EFFECTS / width;
  const ctx = new Context();
  const sources = [];
  for (let g = 0; g < groups; g++) sources.push(ctx.cell(0));

  let sink = 0;
  let runs = 0;
  const handles = new Array(TOTAL_EFFECTS);

  const t0 = performance.now();
  let k = 0;
  for (let g = 0; g < groups; g++) {
    const src = sources[g];
    for (let i = 0; i < width; i++) {
      handles[k++] = ctx.effect(() => {
        runs++;
        sink += ctx.getCell(src);
      });
    }
  }
  const t1 = performance.now();

  for (let g = 0; g < groups; g++) ctx.setCell(sources[g], g + 1);
  const t2 = performance.now();

  for (let i = 0; i < TOTAL_EFFECTS; i++) ctx.disposeEffect(handles[i]);
  const t3 = performance.now();

  if (sink === -1) throw new Error("unreachable");
  return { build: t1 - t0, notify: t2 - t1, teardown: t3 - t2, runs };
}

// SATURATED teardown: dispose a cohort from *inside an effect body*, so the
// disposes run while the rest of the cohort is still queued behind them.
//
// This column is the whole point of the file. The `teardown` column above
// disposes after the flush has drained, and `flushEffects` resets the pending
// array on drain — so that column can never see a pending scan, and a forced
// naive arm measured against it comes back flat and reads as a clean negative.
// It isn't; it is measuring nothing. (The same trap caught the dart audit,
// which saturated via `ctx.batch()` — batching defers the cascade to batch
// exit, leaving the pending list empty for the entire arm.)
//
// Ordering matters: the flush drains dependents in REVERSE registration order,
// so the disposer is registered LAST in order to run FIRST. Register it first
// and the victims have already drained by the time it runs, indexOf finds
// nothing, and the defect is invisible again.
// SATURATION IS ASSERTED, NOT ASSUMED. Building this arm carefully is not
// enough: three sibling audits (dart, zig, and the first draft of this one) all
// had a saturated arm that was silently inert, and an inert arm reports a flat
// column that reads exactly like a clean negative. `verifyCascadeSaturated`
// therefore checks, through the instrumentation counters, that the pending
// queue actually reached full width before any timing is trusted, and throws if
// it did not.
function verifyCascadeSaturated(Context, width) {
  if (width < 2) return; // no cohort to queue behind the disposer
  const ctx = new Context({ instrument: true });
  const src = ctx.cell(0);
  const victims = [];
  for (let i = 0; i < width - 1; i++) victims.push(ctx.effect(() => ctx.getCell(src)));
  let armed = false;
  let ranWhilePending = 0;
  ctx.effect(() => {
    ctx.getCell(src);
    if (!armed) return;
    for (let i = 0; i < victims.length; i++) ctx.disposeEffect(victims[i]);
  });
  armed = true;
  for (const v of victims) {
    const before = ctx.isEffectActive(v);
    if (before) ranWhilePending++;
  }
  ctx.resetInstrumentation();
  ctx.setCell(src, 1);
  const snap = ctx.instrumentationSnapshot();
  if (!snap) throw new Error("cascade saturation check needs instrumentation");
  if (snap.maxEffectQueueDepth < width) {
    throw new Error(
      `cascade arm is NOT saturated at width ${width}: peak queue depth was ` +
        `${snap.maxEffectQueueDepth}, expected >= ${width}. The disposes are not ` +
        `running while the cohort is queued, so this arm measures nothing and a ` +
        `flat column from it is meaningless.`,
    );
  }
  // The disposer must actually have suppressed the cohort — if the victims all
  // ran anyway, the disposes landed after they drained (the benign order).
  for (const v of victims) {
    if (ctx.isEffectActive(v)) {
      throw new Error(`cascade arm at width ${width}: victims survived the cascade`);
    }
  }
  if (ranWhilePending !== width - 1) {
    throw new Error(`cascade arm at width ${width}: cohort was not live before the publish`);
  }
}

function runCascade(Context, width) {
  const groups = TOTAL_EFFECTS / width;
  const ctx = new Context();
  const srcs = [];
  let runs = 0;
  for (let g = 0; g < groups; g++) {
    const src = ctx.cell(0);
    srcs.push(src);
    const victims = [];
    for (let i = 0; i < width - 1; i++) {
      victims.push(ctx.effect(() => { runs++; ctx.getCell(src); }));
    }
    let armed = false;
    ctx.effect(() => {
      runs++;
      ctx.getCell(src);
      if (!armed) return;
      for (let i = 0; i < victims.length; i++) ctx.disposeEffect(victims[i]);
    });
    armed = true;
  }
  const t0 = performance.now();
  for (let g = 0; g < groups; g++) ctx.setCell(srcs[g], g + 1);
  const t1 = performance.now();
  return { cascade: t1 - t0, runs };
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  return s[s.length >> 1];
}

const NS_PER_EFFECT = (ms) => (ms * 1e6) / TOTAL_EFFECTS;

async function main() {
  const json = process.argv.includes("--json");
  const arms = ["fixed", "naive-remove", "naive-splice", "naive-nevershrink"];
  const mods = {};
  for (const a of arms) mods[a] = await loadArm(a);

  // Prove every arm's cascade column is live before measuring anything.
  for (const a of arms) for (const width of LADDER) verifyCascadeSaturated(mods[a].Context, width);
  console.log(`cascade saturation verified for ${arms.length} arms across ${LADDER.length} rungs`);

  // Warm the JIT: full ladder passes, results discarded. Every arm is a
  // separate module instance, so each needs its own warm-up.
  for (let w = 0; w < WARMUP; w++) {
    for (const a of arms)
      for (const width of LADDER) {
        runRung(mods[a].Context, width);
        runCascade(mods[a].Context, width);
      }
  }
  gc();

  const results = {};
  for (const a of arms) results[a] = {};

  // Interleave: all arms for a rung, then the next rung, so arms see the same
  // ambient load. Repeat REPS times and take medians.
  for (const width of LADDER) {
    const samples = {};
    for (const a of arms) samples[a] = { build: [], notify: [], teardown: [], cascade: [] };
    // Equal-work guard. A naive arm that silently stops running effects looks
    // fast (or makes the fixed arm look slow) and would produce a completely
    // fictitious margin. Every arm must perform the identical number of effect
    // runs at every rung, or the rung's numbers are not comparable and the run
    // is aborted rather than reported.
    let expectedRuns = null;
    let expectedCascadeRuns = null;
    const parity = (got, want, a, what) => {
      if (want === null) return got;
      if (got !== want) {
        throw new Error(
          `arm ${a} ran ${got} effects (${what}) at width ${width}, expected ${want} ` +
            `— arms are not doing equal work, measurements are not comparable`,
        );
      }
      return want;
    };
    for (let r = 0; r < REPS; r++) {
      for (const a of arms) {
        gc();
        const s = runRung(mods[a].Context, width);
        expectedRuns = parity(s.runs, expectedRuns, a, "ladder");
        samples[a].build.push(s.build);
        samples[a].notify.push(s.notify);
        samples[a].teardown.push(s.teardown);
        const c = runCascade(mods[a].Context, width);
        expectedCascadeRuns = parity(c.runs, expectedCascadeRuns, a, "cascade");
        samples[a].cascade.push(c.cascade);
      }
    }
    for (const a of arms) {
      results[a][width] = {
        build: NS_PER_EFFECT(median(samples[a].build)),
        notify: NS_PER_EFFECT(median(samples[a].notify)),
        teardown: NS_PER_EFFECT(median(samples[a].teardown)),
        cascade: NS_PER_EFFECT(median(samples[a].cascade)),
      };
    }
  }

  if (json) {
    console.log(JSON.stringify({ total: TOTAL_EFFECTS, ladder: LADDER, results }, null, 2));
    return;
  }

  const f = (x) => x.toFixed(1).padStart(9);
  for (const a of arms) {
    console.log(`\n=== arm: ${a} === (ns per effect, total work fixed at ${TOTAL_EFFECTS})`);
    console.log("    width |    build |   notify | teardown |  cascade");
    for (const width of LADDER) {
      const r = results[a][width];
      console.log(
        `${String(width).padStart(9)} |${f(r.build)} |${f(r.notify)} |${f(r.teardown)} |${f(r.cascade)}`,
      );
    }
  }

  const ctl = LADDER[0];
  const top = LADDER[LADDER.length - 1];
  console.log(`\n=== growth vs width-${ctl} control, at width ${top} ===`);
  console.log("             arm |    build |   notify | teardown |  cascade");
  for (const a of arms) {
    const g = (col) => (results[a][top][col] / results[a][ctl][col]).toFixed(1).padStart(9);
    console.log(
      `${a.padStart(16)} |${g("build")} |${g("notify")} |${g("teardown")} |${g("cascade")}`,
    );
  }
  console.log(`\n=== detection margin: naive / fixed at width ${top} ===`);
  console.log("             arm |    build |   notify | teardown |  cascade");
  for (const a of arms.slice(1)) {
    const m = (col) => (results[a][top][col] / results.fixed[top][col]).toFixed(1).padStart(9);
    console.log(
      `${a.padStart(16)} |${m("build")} |${m("notify")} |${m("teardown")} |${m("cascade")}`,
    );
  }
}

main();
