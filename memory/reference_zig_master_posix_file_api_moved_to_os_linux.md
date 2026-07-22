---
name: reference-zig-master-posix-file-api-moved-to-os-linux
description: "On zig master 0.17.0-dev, std.posix.open/close/ftruncate/unlink and std.fs.* file API are gone (moved to async std.Io.Dir/File); use std.os.linux raw syscalls for fd/shm code in lazily-zig"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0b113138-a5bc-4b09-af4d-4328baff6456
---

Building lazily-zig on **zig master (observed 0.17.0-dev.892+54537285c, 2026-07-11)**: the low-level file/fd API churned hard mid-migration.

- **Gone from `std.posix`:** `open`, `close`, `ftruncate`, `unlink`, `AT.FDCWD`. Only `openat`/`openatZ`, `mmap`, `munmap` remain there.
- **`std.fs` gutted:** the whole file API moved to **`std.Io.Dir` / `std.Io.File`**, which take an async `Io` instance (`openFile(dir, io, path, opts)`, `File.close(file, io)`) — heavy to wire up for simple fd work. `std.fs.deleteFileAbsolute` is gone.
- **Stable escape hatch = `std.os.linux` raw syscalls:** `std.os.linux.{open,close,ftruncate,unlink,mmap,munmap}` all still exist and are stable. They return `usize` (raw syscall return) — check errors via `std.posix.errno(rc)` and `@intCast` the fd on `.SUCCESS`. `std.posix.mmap`/`munmap` still compile and are the portable seam (keep using them).

lazily-zig `src/lazily/transport.zig` `ShmBackend` (genuine POSIX `shm_open`+`mmap` cross-process region) uses exactly this pattern: `shmPathZ` (allocPrintSentinel `/dev/shm/<name>`) + `sysOpen`/`mapShared` helpers over `std.os.linux.open` + `std.posix.mmap`, gated Linux. `minimum_zig_version = 0.16.0` but `mise run test` builds on master, so code must compile on both — raw `std.os.linux` calls satisfy both.

Symptom if you hit the old API: `error: root source file struct 'posix' has no member named 'open'/'close'` and `struct 'fs' has no member named 'deleteFileAbsolute'`. Related: [[reference-lazily-zig-test-suite-and-coldrecalc-bottleneck]].
