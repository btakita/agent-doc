---
name: reference_lazily_cpp_wasm_clean_header_subset
description: "lazily-cpp's reactive core is wasm/Emscripten-clean but the umbrella header is not — verified transitive include closures"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 94dfa3d2-6288-46f3-9c9e-a497138b7902
  modified: 2026-07-30T04:12:53.747Z
---

lazily-cpp is a header-only C++17 `INTERFACE` library (only `src/ffi.cpp` +
`src/interop_peer.cpp` compile). Transitive `lazily/*` include closures scanned for
POSIX/threading deps (verified 2026-07-29):

| Entry header | lazily headers | wasm-hostile deps |
|---|---|---|
| `cell.hpp` | 8 | **none** (pure `<memory>`/`<utility>`) |
| `collections.hpp` | 11 | **none** |
| `keyed_order.hpp` | 1 | **none** |
| `statechart.hpp` | 9 | **none** |
| `work_queue.hpp` | 12 | `<thread>`, `std::thread`, `<mutex>`, `<atomic>` |
| `lazily.hpp` (umbrella) | 46 | above **plus** `shm_open`/`mmap`/`unistd.h` |

The offenders are `transport.hpp` (`ShmBackend`: `::shm_open` + `mmap`, needs the
`_GNU_SOURCE` define set in `CMakeLists.txt`), `reliable_sync.hpp` (`unistd.h`),
`thread_safe.hpp`, and `async_context.hpp`.

**Consequence:** the reactive core cross-compiles to wasm unobstructed, but ONLY if a
consumer includes narrow headers deliberately. The umbrella `lazily.hpp` is what a
naive consumer reaches for and it drags in POSIX shm. The queue family needs
`-pthread`, i.e. SharedArrayBuffer + COOP/COEP hosting headers.

**Not proven:** lazily-cpp CI is `ubuntu-latest` only, no emscripten/wasm job. The
closures are unobstructed, not tested.

Related: [[project_lazily_gdscript_binding_direction]],
[[reference_lazily_cpp_family_features_wire_level_model]]
