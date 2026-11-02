//! Synthetic allocation cost of the real public Host::show methods.
//! Run via experiments/performance/host-payload/bench.py; stdout is real wire.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use proteus_extension::{Host, Value, json};

struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static MEASURE: AtomicBool = AtomicBool::new(false);

fn added(size: usize) {
    let live = LIVE.fetch_add(size, Relaxed) + size;
    if MEASURE.load(Relaxed) {
        BYTES.fetch_add(size, Relaxed);
        CALLS.fetch_add(1, Relaxed);
        PEAK.fetch_max(live, Relaxed);
    }
}

// The counter delegates to the system allocator and never allocates itself.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            added(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            added(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            LIVE.fetch_sub(layout.size(), Relaxed);
            added(size);
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn root(rows: usize, bytes: usize) -> Value {
    let children = (0..rows)
        .map(|index| json!({"type":"text", "id":index.to_string(), "text":"x".repeat(bytes)}))
        .collect();
    let mut root = json!({"type":"stack", "children":null});
    root["children"] = Value::Array(children);
    root
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "host_cost keys|title ROWS BYTES ITERATIONS");
    let kind = &args[1];
    assert!(matches!(kind.as_str(), "keys" | "title"));
    let rows: usize = args[2].parse().unwrap();
    let bytes: usize = args[3].parse().unwrap();
    let iterations: usize = args[4].parse().unwrap();
    assert!((1..=4096).contains(&rows) && (1..=16384).contains(&bytes));
    assert!(rows * bytes <= 8 * 1024 * 1024);
    assert!((1..=100).contains(&iterations));
    let mut host = Host::default();
    host.session = Some("/synthetic/host-cost".into());
    for iteration in 0..iterations {
        let root = root(rows, bytes);
        // Building the independent input and the retained previous tree are
        // outside the measured show. The input shape never changes per revision.
        let base = LIVE.load(Relaxed);
        PEAK.store(base, Relaxed);
        BYTES.store(0, Relaxed);
        CALLS.store(0, Relaxed);
        MEASURE.store(true, Relaxed);
        let started = Instant::now();
        match kind.as_str() {
            "keys" => host.show_with_keys("synthetic", root, Value::Object(Default::default())),
            "title" => host.show_with_title("synthetic", root, "Synthetic"),
            _ => unreachable!(),
        }
        let elapsed = started.elapsed().as_nanos();
        MEASURE.store(false, Relaxed);
        let allocated = BYTES.load(Relaxed);
        let calls = CALLS.load(Relaxed);
        let peak_extra = PEAK.load(Relaxed).saturating_sub(base);
        eprintln!(
            "{{\"iteration\":{iteration},\"allocated_bytes\":{allocated},\"allocation_calls\":{calls},\"peak_extra_live_bytes\":{peak_extra},\"elapsed_ns\":{elapsed}}}"
        );
    }
}
