---
name: reference-stamped-evidence-defeats-byte-emptiness-checks
description: Adding a run-id stamp to an evidence file silently voids every `[ ! -s ]` byte check over it -- nine of ten lazily bindings still refused, but every one misdiagnosed the recorder as a corpus regression
metadata:
  type: reference
---

Stamping conformance evidence with `# lazily-run-id <id>` as line 1 makes a
record-free file **non-empty**, so every `[ ! -s ]` / `getsize()==0` / `test -s`
over it stops testing what it was written to test. Audited across all ten
lazily bindings 2026-09-12 (`#lzstampsatisfiesnonempty`).

**It was NOT a false green.** Nine of nine bindings with a live byte check still
refused a stamp-only file, on floors that sit *below* the byte test. What the
stamp destroyed is the **diagnosis** -- each reported a recorder that never ran
as a coverage collapse: kt 179, cs 151, rs 150+166, go 147, py 145, dart
144/153/712, zig 138, gd 91 **wrong** findings. zig's guard even carried a
comment asserting the case was handled ("a run that produced nothing leaves an
empty file and the `-s` check above already fails it") -- false since the stamp
landed, and nothing noticed. Only lazily-cpp's workflow step was a true false
pass.

**The structural cause** (named by go): byte tests at the **call sites** and the
record check *inside* the reader, running after them. "That split, bytes at the
call site and substance inside, is what let the stamp weaken this silently."
Fix is one reader per file covering absent / zero-records / stamp-with-zero-
records / unstamped / other-run, each naming the file and the variable to set.

**Recipe-side vs producer-side stamping is NOT equivalent, and the right choice
is per binding.** This is the audited split (`#lzstampsitedoc`):

| Binding | Stamp site | Why that site is safe |
|---|---|---|
| js | Recipe-side | The `test` command truncates and stamps all four ledgers once before Node's test workers append. The reset owns freshness; no worker has to win a first-writer race. |
| cpp | Recipe-side | 38 binaries flush with `ios::app`. A producer-side “first writer stamps” check would race, so `test` / `conformance` must truncate and stamp before `ctest`. |
| py | Recipe-side | The `conformance_manifest` task stamps before pytest-xdist workers append at exit. `conftest.py` can rewrite a foreign stamp for a direct, ordered pytest run, but its own comment records that this fallback would lose workers under xdist; it is not the normal freshness protocol. |
| gd | Recipe-side | `append_line` is deliberately append-only across gdUnit4 passes and cannot clear a leftover file. The measured failure appended 250 records to the previous run's 317, so truncation and stamping must be one act; the recipe adds a separate completion marker after the suite succeeds. |
| rs | Producer-side | The first producer after truncation emits the stamp and its triggering record in one `O_APPEND` `write_all`; a producer cannot create a stamp-only ledger. |
| go | Producer-side | Each contributing test binary returns without writing when it has no records; otherwise it appends its current stamp plus sorted records as one payload. Multiple stamps are valid only when every one names the current run. |
| cs | Producer-side | A test host's flush returns when its opened set is empty, then appends the stamp and the opened records together with one `AppendAllLines`. |
| kt | Producer-side | The JVM shutdown writer returns when no fixture was loaded, then writes the stamp and sorted loaded set together. A cached `:test` starts no JVM and therefore cannot refresh stale evidence. |
| dart | Producer-side | The first writer holds the cross-isolate lock, seeks to the true end, and writes the stamp immediately before the triggering record while still holding that lock. |
| zig | Producer-side | `append` writes the stamp immediately before the evidence line that occasioned it in the same guarded append path; no record means the producer writes no stamp. |

A producer-side stamp in the six safe bindings can attest “a recorder reached
this file” because the producer cannot emit it without evidence. A recipe-side
stamp in the other four attests only “this invocation reset the file”; their
record-count rung (and gd's completion marker) is therefore load-bearing. Every
binding still counts non-stamp records: retaining that invariant keeps a future
change of stamp site from silently weakening the evidence contract.

**Two traps when writing the replacement count:** `grep -c` exits 1 on a zero
count, so under `set -o pipefail` the rung aborts with **no message at all**
(use `awk`); and interpolating a prefix variable into `sed 's///'` makes its
characters a regex rather than text (use a bash `case`).

**How to apply:** whenever you add a header, stamp, or marker to a file that
something else tests for emptiness, grep every consumer for `-s`, `-e`,
`getsize`, and byte-length tests in the same change. See
[[reference-lazily-conformance-delivery-three-failure-modes]] and
[[reference-lazily-rung0-bind-ledger-shapes-per-binding]].
