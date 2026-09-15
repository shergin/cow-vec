//! Measures the memory each container needs to keep a chain of versions
//! alive, the workload the README's chained-generation benchmark times.
//!
//! Time is the weaker half of that story: a `CowVec` version keeps its own
//! full pointer table, a `PagedVec` version shares every page it did not
//! touch. Run with `cargo run --release --example memory`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cow_vec::{CowVec, PagedVec};

/// The system allocator, wrapped to count live bytes.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: Every call is forwarded to `System` unchanged; only a counter is
// maintained on the side.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = System.realloc(ptr, layout, new_size);
        if !new.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            LIVE.fetch_add(new_size, Ordering::Relaxed);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

#[derive(Clone)]
struct Item {
    #[allow(dead_code)]
    id: u64,
    #[allow(dead_code)]
    name: String,
}

impl Item {
    fn new(i: usize) -> Self {
        Self {
            id: i as u64,
            name: format!("item-{i:07}-payload"),
        }
    }
}

const N: usize = 1_000_000;
const GENS: usize = 32;
const EDITS: usize = 50;

/// Deterministic scattered indices, as in the benchmarks.
fn scattered_indices(count: usize, len: usize) -> Vec<usize> {
    let mut state = 0x2545F4914F6CDD1Du64;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % len
        })
        .collect()
}

/// Builds `GENS` versions derived from `base`, each replacing `EDITS`
/// scattered elements, and returns how many extra bytes keeping all of
/// them alive costs.
fn measure<V: Clone>(base: V, indices: &[usize], set: impl Fn(&mut V, usize, Item)) -> usize {
    let before = live_bytes();
    let mut versions = vec![base];
    for g in 0..GENS {
        let mut next = versions.last().unwrap().clone();
        for (k, &i) in indices[g * EDITS..(g + 1) * EDITS].iter().enumerate() {
            set(&mut next, i, Item::new(k));
        }
        versions.push(next);
    }
    let extra = live_bytes() - before;
    drop(versions);
    extra
}

fn megabytes(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() {
    let indices = scattered_indices(GENS * EDITS, N);
    let source = || (0..N).map(Item::new).collect::<Vec<_>>();

    println!(
        "{GENS} chained generations of {N} elements, {EDITS} scattered edits each, all versions alive\n"
    );
    println!(
        "{:<18} {:>12} {:>16}",
        "container", "extra memory", "per generation"
    );

    let report = |name: &str, extra: usize| {
        println!(
            "{name:<18} {:>9.1} MB {:>13.2} MB",
            megabytes(extra),
            megabytes(extra) / GENS as f64
        );
    };

    let base = CowVec::from(source());
    report("CowVec", measure(base, &indices, |v, i, x| v.set(i, x)));

    let base = PagedVec::<Item>::from(source());
    report("PagedVec", measure(base, &indices, |v, i, x| v.set(i, x)));

    let base: Arc<Vec<Arc<Item>>> = Arc::new(source().into_iter().map(Arc::new).collect());
    report(
        "Arc<Vec<Arc>>",
        measure(base, &indices, |v, i, x| {
            Arc::make_mut(v)[i] = Arc::new(x);
        }),
    );

    let base: imbl::Vector<Item> = source().into_iter().collect();
    report(
        "imbl",
        measure(base, &indices, |v, i, x| {
            v.set(i, x);
        }),
    );
}
