//! Bound the long-lived project controller's heap.
//!
//! The controller holds every attached session document as a character CRDT
//! and serves each editor request on its own connection thread. A replica RPC
//! on a large document allocates and frees tens of megabytes at a time (op
//! vectors, columnar encodes, decoded payloads). With glibc's defaults that
//! churn never goes back to the kernel:
//!
//! - every connection thread is bound to its own malloc arena, up to
//!   `8 × cores` of them (256 on the 32-core dogfood host), and
//! - freeing one large mmapped block raises glibc's *dynamic* mmap threshold
//!   to that size and the trim threshold to twice it, so later 1–32 MB
//!   buffers are carved from the arenas and each arena keeps up to ~64 MB of
//!   freed memory at its top instead of trimming it.
//!
//! Measured on 2026-11-22: the haiven-dev controller reached 7.3 GB RSS after
//! 58 minutes (`VmHWM` 9.2 GB) and agent-loop 5.5 GB, with 124 and 82
//! fully-resident 64 MB arena heaps respectively, while the documents they
//! held total 1.9 M and 1.1 M characters. RSS then oscillated around that
//! plateau instead of growing: the memory was freed, never returned.
//!
//! [`configure_process_allocator`] pins the thresholds (which also disables the
//! dynamic adjustment) and caps the arena count, and [`spawn_periodic_trim`]
//! hands freed pages inside the remaining arenas back with `malloc_trim(0)`.
//! Other allocators (musl, macOS, Windows) need neither and are left alone.

use std::time::Duration;

/// Arenas the controller may create. Connection threads mostly wait on
/// sockets; four arenas keep allocator contention low while bounding the
/// number of heaps that can each retain freed memory.
pub const CONTROLLER_MALLOC_ARENA_MAX: i32 = 4;

/// Allocations at or above this size are served by `mmap` and unmapped on
/// free. Fixing the value disables glibc's dynamic threshold, which is what let
/// large transient CRDT buffers settle inside the arenas.
pub const CONTROLLER_MMAP_THRESHOLD_BYTES: i32 = 1 << 20;

/// Free memory at the top of a heap beyond this is returned on `free`.
pub const CONTROLLER_TRIM_THRESHOLD_BYTES: i32 = 8 << 20;

/// How often the controller returns freed pages inside its arenas.
pub const CONTROLLER_HEAP_TRIM_INTERVAL: Duration = Duration::from_secs(30);

/// What [`configure_process_allocator`] applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocatorPolicy {
    /// `false` on platforms whose allocator is not glibc malloc.
    pub supported: bool,
    /// Every `mallopt` call was accepted.
    pub applied: bool,
}

impl AllocatorPolicy {
    pub fn ops_log_line(self) -> String {
        format!(
            "controller_heap_policy supported={} applied={} arena_max={} mmap_threshold_bytes={} trim_threshold_bytes={} trim_interval_secs={}",
            self.supported,
            self.applied,
            CONTROLLER_MALLOC_ARENA_MAX,
            CONTROLLER_MMAP_THRESHOLD_BYTES,
            CONTROLLER_TRIM_THRESHOLD_BYTES,
            CONTROLLER_HEAP_TRIM_INTERVAL.as_secs(),
        )
    }
}

/// Apply the controller's allocator policy to this process. Call it once, as
/// early as possible in `controller serve`, before the worker threads start.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn configure_process_allocator() -> AllocatorPolicy {
    // SAFETY: `mallopt` only adjusts glibc's allocator tunables; every value
    // here is in range for its parameter.
    let applied = unsafe {
        libc::mallopt(libc::M_ARENA_MAX, CONTROLLER_MALLOC_ARENA_MAX) == 1
            && libc::mallopt(libc::M_MMAP_THRESHOLD, CONTROLLER_MMAP_THRESHOLD_BYTES) == 1
            && libc::mallopt(libc::M_TRIM_THRESHOLD, CONTROLLER_TRIM_THRESHOLD_BYTES) == 1
    };
    AllocatorPolicy {
        supported: true,
        applied,
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn configure_process_allocator() -> AllocatorPolicy {
    AllocatorPolicy {
        supported: false,
        applied: false,
    }
}

/// Return freed pages in every arena to the kernel. Returns whether glibc
/// released any memory; always `false` where the allocator is not glibc.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn trim_heap() -> bool {
    // SAFETY: `malloc_trim` takes the arena locks itself and only releases
    // memory that is already free.
    unsafe { libc::malloc_trim(0) == 1 }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn trim_heap() -> bool {
    false
}

/// Start the detached thread that calls [`trim_heap`] every
/// [`CONTROLLER_HEAP_TRIM_INTERVAL`]. A no-op where the allocator is not glibc.
pub fn spawn_periodic_trim() {
    if !configure_process_allocator_supported() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("agent-doc-heap-trim".to_string())
        .spawn(|| {
            loop {
                std::thread::sleep(CONTROLLER_HEAP_TRIM_INTERVAL);
                trim_heap();
            }
        });
    if let Err(error) = spawned {
        eprintln!("[controller] failed to start heap-trim worker: {error}");
    }
}

const fn configure_process_allocator_supported() -> bool {
    cfg!(all(target_os = "linux", target_env = "gnu"))
}

/// Resident set size of this process in bytes, from `/proc/self/statm`.
#[cfg(target_os = "linux")]
pub fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: `sysconf` has no preconditions.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(pages.saturating_mul(u64::try_from(page_size).ok()?))
}

#[cfg(not(target_os = "linux"))]
pub fn resident_bytes() -> Option<u64> {
    None
}
