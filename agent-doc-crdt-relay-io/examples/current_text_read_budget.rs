//! `#sharedtextrevision` measurement: full-document allocations, bytes, and
//! latency per controller current-text read on a ~6k-line (~660 KB) session
//! document, with 4 concurrent readers x 5 reads after each of 20 edits
//! (alternating insert / delete-only).
//!
//! `cargo run --release -p agent-doc-crdt-relay-io --example current_text_read_budget`
//! (`LINES=` / `EDITS=` override the fixture). A "full-text allocation" is any
//! allocation at least 90% of the document size: a copy, rebuild, or
//! serialization buffer of the whole text.
use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

struct Counting;
static THRESHOLD: AtomicUsize = AtomicUsize::new(usize::MAX);
static FULL_ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        if l.size() >= THRESHOLD.load(Ordering::Relaxed) {
            FULL_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        BYTES.fetch_add(n as u64, Ordering::Relaxed);
        if n >= THRESHOLD.load(Ordering::Relaxed) && l.size() < THRESHOLD.load(Ordering::Relaxed) {
            FULL_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn status(field: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(field))
        .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
        .unwrap_or_default()
}

/// One controller `crdt_current_text` read as the RPC handler performs it:
/// relay read, `controller_crdt_current_text` log hash, and the JSON response
/// (`text_hash` + body). Mirrors `handle_crdt_current_text_rpc`.
fn controller_read(file: &std::path::Path) -> usize {
    match agent_doc_crdt_relay_io::current_text_snapshot_for_file_with_authority(
        file,
        CrdtAuthority::MultiReplica,
        agent_doc_crdt_relay_io::CurrentTextRead::FlushBarrier,
    )
    .unwrap()
    {
        agent_doc_crdt_relay_io::CurrentTextSnapshot::Current { text, .. } => {
            let log_hash = text.content_hash().to_owned();
            let response = serde_json::json!({
                "status": "current",
                "text_len": text.len(),
                "text_hash": text.content_hash(),
                "text_revision": text.revision(),
                "text": text.as_str(),
            });
            let wire = serde_json::to_vec(&response).unwrap();
            wire.len() + log_hash.len()
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn main() {
    let lines: usize = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6_000);
    let edits: usize = std::env::var("EDITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let readers: usize = 4;
    let reads_per_reader: usize = 5;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join("sdk-dev.md");
    let mut text: String = (0..lines)
        .map(|i| format!("- [ ] queue item {i:05}: the controller must reuse one materialized revision for every single reader ok (sdk-dev)\n"))
        .collect();
    std::fs::write(&file, &text).unwrap();
    THRESHOLD.store(text.len() * 9 / 10, Ordering::Relaxed);
    // `MODE=editor` delivers each edit from a registered editor replica (the
    // operator-typing path: a remote batch the canonical folds); the default
    // applies it as a binary-authored canonical replace.
    let editor_mode = std::env::var("MODE").is_ok_and(|mode| mode == "editor");
    agent_doc_crdt_relay_io::with_hub(&file, |hub| {
        hub.apply_binary_authored_replace("", &text, "bench_seed")
            .unwrap();
        if editor_mode {
            hub.register(7).unwrap();
        }
    })
    .unwrap();
    let rss0 = status("VmRSS:");
    let (mut read_allocs, mut edit_allocs, mut read_bytes) = (0u64, 0u64, 0u64);
    let materializations = || {
        agent_doc_crdt_relay_io::with_hub(&file, |hub| hub.canonical_text_materializations())
            .unwrap()
    };
    let materialized_before = materializations();
    let started = Instant::now();
    let mut read_time = std::time::Duration::ZERO;
    for e in 0..edits {
        // The fixture is ASCII, so byte offsets are codepoint offsets.
        let (next, offset, delete_len, insert) = if e % 2 == 0 {
            let at = text.len() / 2;
            let at = text[at..].find('\n').unwrap() + at + 1;
            let insert = format!("- [ ] inserted {e}\n");
            let next = format!("{}{insert}{}", &text[..at], &text[at..]);
            (next, at, 0, insert)
        } else {
            // delete-only: drop the line inserted by the previous edit
            let line = format!("- [ ] inserted {}\n", e - 1);
            let at = text.find(&line).unwrap();
            (text.replacen(&line, "", 1), at, line.len(), String::new())
        };
        let a0 = FULL_ALLOCS.load(Ordering::Relaxed);
        agent_doc_crdt_relay_io::with_hub(&file, |hub| {
            if editor_mode {
                hub.apply_local(7, offset as u32, delete_len as u32, &insert)
                    .unwrap();
                assert_eq!(hub.canonical_text_snapshot().as_str(), next);
            } else {
                hub.apply_binary_authored_replace(&text, &next, "bench_edit")
                    .unwrap();
            }
        })
        .unwrap();
        text = next;
        let a1 = FULL_ALLOCS.load(Ordering::Relaxed);
        let b1 = BYTES.load(Ordering::Relaxed);
        let t = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..readers {
                s.spawn(|| {
                    for _ in 0..reads_per_reader {
                        controller_read(&file);
                    }
                });
            }
        });
        read_time += t.elapsed();
        read_allocs += FULL_ALLOCS.load(Ordering::Relaxed) - a1;
        read_bytes += BYTES.load(Ordering::Relaxed) - b1;
        edit_allocs += a1 - a0;
    }
    let reads = edits * readers * reads_per_reader;
    println!(
        "canonical_text_materializations_per_edit={:.2}",
        (materializations() - materialized_before) as f64 / edits as f64
    );
    println!(
        "doc_bytes={} lines={} edits={} reads={} full_text_allocs_per_read={:.2} full_text_allocs_per_edit_reads={:.1} edit_phase_full_allocs_per_edit={:.1} read_bytes_per_read={} read_ms_per_read={:.3} total_s={:.2} rss_start_kb={} rss_end_kb={} hwm_kb={}",
        text.len(),
        lines,
        edits,
        reads,
        read_allocs as f64 / reads as f64,
        read_allocs as f64 / edits as f64,
        edit_allocs as f64 / edits as f64,
        read_bytes / reads as u64,
        read_time.as_secs_f64() * 1000.0 / reads as f64,
        started.elapsed().as_secs_f64(),
        rss0,
        status("VmRSS:"),
        status("VmHWM:"),
    );
}
