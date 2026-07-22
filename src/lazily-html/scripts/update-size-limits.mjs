#!/usr/bin/env node
// Refresh the generated bundle-size section in README.md.
//
// Sibling of scripts/run-benchmarks.mjs: runs `size-limit --json`, captures
// the measured minified+brotlied size and the budget per entry, and rewrites
// the <!-- size-limits:start --> ... <!-- size-limits:end --> block. This keeps
// the published size table honest — it is regenerated on every `npm run build`
// so it can never drift from the actual shipped bytes.
//
// Usage:
//   node scripts/update-size-limits.mjs          # run size-limit + refresh README.md
//   node scripts/update-size-limits.mjs --check  # exit 1 if README.md is stale

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createRequire } from "node:module";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const README_MD = path.join(ROOT, "README.md");
const START = "<!-- size-limits:start -->";
const END = "<!-- size-limits:end -->";

const require = createRequire(import.meta.url);
const pkg = require(path.join(ROOT, "package.json"));

function runSizeLimit() {
  const bin = path.join(ROOT, "node_modules", ".bin", "size-limit");
  const out = execFileSync(bin, ["--json"], {
    cwd: ROOT,
    maxBuffer: 1 << 30,
    encoding: "utf8",
  });
  return JSON.parse(out);
}

// Format bytes as a compact human-readable size (B / KB). Uses decimal kilobytes
// (1 KB = 1000 B) to match the numbers `size-limit` itself prints, so the README
// table and `npm run test:size` report the same value for every entry.
function formatSize(bytes) {
  if (bytes < 1000) return `${bytes} B`;
  return `${(bytes / 1000).toFixed(2)} KB`;
}

function render(results) {
  const lines = [];
  lines.push(
    `Generated for package \`${pkg.name}\` version \`${pkg.version}\`. Every entry is **minified + brotlied, tree-shaken to the named import** (\`size-limit\` + esbuild, the same pipeline Webpack/Rollup/Vite apply via \`"sideEffects": false\`).`,
  );
  lines.push("");
  lines.push("Refresh command:");
  lines.push("");
  lines.push("```bash");
  lines.push("npm run build            # regenerates this table as part of every build");
  lines.push("npm run test:size        # gate: fails CI if any entry exceeds its budget");
  lines.push("```");
  lines.push("");
  lines.push("| Import | Size | Budget |");
  lines.push("|---|---:|---:|");
  for (const r of results) {
    const status = r.passed ? "✓" : "✗";
    lines.push(
      `| ${r.name} | ${formatSize(r.size)} ${status} | ${formatSize(r.sizeLimit)} |`,
    );
  }
  return lines.join("\n");
}

function refresh(results) {
  const body = render(results);
  let md = readFileSync(README_MD, "utf8");
  const startIdx = md.indexOf(START);
  const endIdx = md.indexOf(END);
  if (startIdx === -1 || endIdx === -1) {
    throw new Error(`missing ${START} / ${END} markers in README.md`);
  }
  const next =
    md.slice(0, startIdx + START.length) +
    "\n\n" +
    body +
    "\n\n" +
    md.slice(endIdx);
  writeFileSync(README_MD, next);
}

function currentSection() {
  const md = readFileSync(README_MD, "utf8");
  const startIdx = md.indexOf(START);
  const endIdx = md.indexOf(END);
  if (startIdx === -1 || endIdx === -1) return null;
  return md.slice(startIdx + START.length, endIdx);
}

function main() {
  const args = process.argv.slice(2);
  const results = runSizeLimit();

  if (args.includes("--check")) {
    // Deterministic gate (sizes are stable across runs, unlike timings): the
    // README table must byte-match what size-limit just produced. The table
    // header line carries the package version, so a bumped version without a
    // refresh also trips this gate.
    const fresh = "\n\n" + render(results) + "\n\n";
    const current = currentSection();
    if (current === null) {
      console.error("README.md missing size-limits markers.");
      process.exit(1);
    }
    if (current === fresh) {
      console.error("README.md size-limits section is up to date.");
      return;
    }
    console.error("README.md size-limits section is stale.");
    console.error("Run: npm run build  (or  node scripts/update-size-limits.mjs)");
    process.exit(1);
  }

  refresh(results);
  const failed = results.filter((r) => !r.passed);
  console.error(
    "Refreshed README.md size-limits section with %d entries (%d over budget).",
    results.length,
    failed.length,
  );
  if (failed.length) {
    for (const r of failed) {
      console.error(`  OVER: ${r.name} ${formatSize(r.size)} > ${formatSize(r.sizeLimit)}`);
    }
    process.exit(1);
  }
}

main();
