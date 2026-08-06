---
name: reference_lazily_zig_verify_all_three_pinned_toolchains
description: "lazily-zig CI pins 0.15.2 + 0.16.0 + master; verifying on local master alone hides std API-shape breaks — use the mise test_0_15_2 / test_0_16 / test_master tasks"
metadata:
  node_type: memory
  type: reference
---

`lazily-zig`'s CI matrix runs **three** Zig jobs (0.15.2, 0.16.0, master) but
`.mise.toml` pins `zig = "master"`, so a bare `zig build test` / `make check`
verifies only one of them. All three are installed locally; the tasks exist:

```
mise run test_0_15_2 / test_0_16 / test_master
# or: mise exec zig@0.16.0 -- zig build test --summary all
```

**The failure mode this hides is `std` API *shape* drift, which compiles cleanly on
one version and does not exist on another.** Confirmed 2026-07-30 (`#lzingressbindings`,
fixed in `636feba`): `@typeInfo(E).@"enum"` exposes `field_names` / `field_values`
on master and **`fields`** on 0.16.0 — and neither version exposes the other's
form. A conformance runner built with `field_names` was green locally on master
and failed to *compile* in CI on 0.16.0. Portable substitutes that work on all
three: `std.enums.values(E)` + `@tagName(e)` + `@intFromEnum(e)`.

Counter-trap on the same axis: `std.process.getEnvVarOwned` does **not** exist on
0.16/master, so it is not a fix for 0.15.2's missing
`b.graph.environ_map` in `build.zig` — that shim breaks two green jobs to not-fix
a third. See `#lzzig0152`, which also needs `src/lazily/cell.zig`'s 0.15.2
dependency loop untangled (0.15.2 has been red since before the ingress family).

Related: [[reference_zig_master_posix_file_api_moved_to_os_linux]] is the same
class of problem one layer down (std.posix/std.fs moved on master).
[[feedback_no_hallucinate_correct_errors]] — "green locally" is not "green", and
saying so is the difference.
