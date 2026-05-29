# FFI Relocation Pattern

How to move an FFI function from the main `agent-doc` cdylib crate down into
`agent-doc-core` (or `agent-doc-orchestration`) **without** dropping its symbol
from the shipped `libagent_doc.{so,dylib,dll}` export table that the editor
plugins (JetBrains, VS Code) link against.

This is the "pure FFI relocation" pattern proven across `#k9e1` / `#epv5` /
`#vb8h` / `#e130` (POC: `agent_doc_free_string` + `agent_doc_free_state` moved
to `agent_doc_core::ffi`, verified still exported) and tracked under `#yybc`.

## The problem

Editor plugins load a single `libagent_doc.{so,dylib,dll}` and resolve
`agent_doc_*` symbols at runtime via JNA / FFI. There is exactly **one cdylib**
— the `agent-doc` crate. When an `#[no_mangle] pub extern "C"` function is moved
into a dependency crate (`agent-doc-core`), it is no longer *defined* in the
cdylib's own compilation unit. A plain `pub use agent_doc_core::ffi::*;`
re-export is enough for Rust call sites, but the **static linker will strip the
re-exported symbol from the cdylib's dynamic export table** because nothing in
the cdylib crate references it — the editor plugin then fails to find it.

## The pattern

1. **Move the function** into `agent_doc_core::ffi` (or
   `agent_doc_orchestration::ffi`), keeping its `#[no_mangle] pub extern "C"`
   signature unchanged.
2. **Re-export** from the main crate's `src/ffi.rs`:
   ```rust
   pub use agent_doc_core::ffi::*;
   ```
3. **Force-link** the symbols so the linker keeps them in the cdylib export
   table. Add a never-called reference function in `src/ffi.rs` that names each
   relocated symbol with its exact `extern "C"` type, and reference it from
   `lib.rs` so it lands in the cdylib's compilation unit:
   ```rust
   /// Never called — the symbol references just need to exist in the main
   /// crate's compilation unit so the linker keeps them in the cdylib export
   /// table.
   #[allow(dead_code)]
   fn force_link_core_ffi_symbols() {
       use agent_doc_core::ffi::{agent_doc_free_string, /* …all relocated fns… */};
       let _: unsafe extern "C" fn(*const c_char) = agent_doc_free_string;
       // …one typed reference per relocated symbol…
   }
   ```
   Every relocated symbol must appear in this function. Adding a function to
   core's FFI without adding its reference here will silently drop it from the
   export table.

## Verification (mandatory on every FFI-touching relocation)

```sh
cargo build --release
nm -D target/release/libagent_doc.so | grep -c ' T agent_doc_'
nm -D target/release/libagent_doc.so | grep ' T agent_doc_'   # spot-check names
```

The `T` (text/exported) count must not drop after the move. Run this gate on
every wave that relocates FFI bodies — it is part of the orchestration /
core extraction acceptance criteria (see
[the extraction plan](../../../../tasks/agent-doc/plan-agent-doc-orchestration-extraction.md)).

## Why one cdylib

Editor plugins ship and link a single shared library. Splitting the FFI surface
across multiple cdylibs would force plugin changes and multi-library loading.
Keeping `ffi.rs` and the cdylib in the `agent-doc` shell crate — with relocated
bodies re-exported and force-linked from core — preserves the single-library
contract while still letting the heavy logic live in lower crates.
