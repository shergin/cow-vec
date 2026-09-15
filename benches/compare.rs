//! Benchmarks comparing CowVec and PagedVec against the alternatives a user
//! would realistically reach for:
//!
//! - `Vec<T>`: the baseline; clone is a deep copy
//! - `Arc<Vec<T>>` + `Arc::make_mut`: idiomatic whole-buffer COW
//! - `Arc<Vec<Arc<T>>>` + `Arc::make_mut`: idiomatic element-shared COW
//! - `imbl::Vector<T>`: RRB-tree persistent vector
//!
//! The element type carries a `String` so cloning an element costs a real
//! heap allocation, which is the regime these containers are built for.

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::hint::black_box;

use cow_vec::{CowVec, PagedVec};

#[derive(Clone, PartialEq)]
struct Item {
    id: u64,
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

fn source(n: usize) -> Vec<Item> {
    (0..n).map(Item::new).collect()
}

/// Deterministic scattered indices.
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

fn bench_build(c: &mut Criterion) {
    const N: usize = 100_000;
    let mut group = c.benchmark_group(format!("build_{N}"));
    group.sample_size(20);

    group.bench_function("Vec", |b| {
        b.iter_batched(|| source(N), Vec::from, BatchSize::LargeInput)
    });
    group.bench_function("CowVec", |b| {
        b.iter_batched(|| source(N), CowVec::from, BatchSize::LargeInput)
    });
    group.bench_function("PagedVec", |b| {
        b.iter_batched(|| source(N), PagedVec::<Item>::from, BatchSize::LargeInput)
    });
    group.bench_function("imbl", |b| {
        b.iter_batched(|| source(N), imbl::Vector::from_iter, BatchSize::LargeInput)
    });
    group.finish();
}

fn bench_clone(c: &mut Criterion) {
    const N: usize = 100_000;
    let mut group = c.benchmark_group(format!("clone_{N}"));
    group.sample_size(20);

    let vec_base = source(N);
    group.bench_function("Vec", |b| b.iter(|| black_box(vec_base.clone())));

    let arc_vec_base: Arc<Vec<Item>> = Arc::new(source(N));
    group.bench_function("Arc<Vec>", |b| b.iter(|| black_box(arc_vec_base.clone())));

    let cow_base = CowVec::from(source(N));
    group.bench_function("CowVec", |b| b.iter(|| black_box(cow_base.clone())));

    let paged_base = PagedVec::<Item>::from(source(N));
    group.bench_function("PagedVec", |b| b.iter(|| black_box(paged_base.clone())));

    let imbl_base: imbl::Vector<Item> = source(N).into_iter().collect();
    group.bench_function("imbl", |b| b.iter(|| black_box(imbl_base.clone())));

    group.finish();
}

/// The core scenario: clone, then replace `EDITS` scattered elements.
/// This is the total cost of producing a diverged version.
fn bench_diverge(c: &mut Criterion, n: usize) {
    const EDITS: usize = 50;
    let mut group = c.benchmark_group(format!("clone_plus_{EDITS}_edits_{n}"));
    group.sample_size(10);
    let indices = scattered_indices(EDITS, n);

    let vec_base = source(n);
    group.bench_function("Vec", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = vec_base.clone();
                for (k, &i) in indices.iter().enumerate() {
                    v[i] = Item::new(k);
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    let arc_vec_base: Arc<Vec<Item>> = Arc::new(source(n));
    group.bench_function("Arc<Vec>", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = arc_vec_base.clone();
                let inner = Arc::make_mut(&mut v);
                for (k, &i) in indices.iter().enumerate() {
                    inner[i] = Item::new(k);
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    let arc_vec_arc_base: Arc<Vec<Arc<Item>>> =
        Arc::new(source(n).into_iter().map(Arc::new).collect());
    group.bench_function("Arc<Vec<Arc>>", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = arc_vec_arc_base.clone();
                let inner = Arc::make_mut(&mut v);
                for (k, &i) in indices.iter().enumerate() {
                    inner[i] = Arc::new(Item::new(k));
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    let cow_base = CowVec::from(source(n));
    group.bench_function("CowVec", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = cow_base.clone();
                for (k, &i) in indices.iter().enumerate() {
                    v.set(i, Item::new(k));
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    let paged_base = PagedVec::<Item>::from(source(n));
    group.bench_function("PagedVec", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = paged_base.clone();
                for (k, &i) in indices.iter().enumerate() {
                    v.set(i, Item::new(k));
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    let imbl_base: imbl::Vector<Item> = source(n).into_iter().collect();
    group.bench_function("imbl", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut v = imbl_base.clone();
                for (k, &i) in indices.iter().enumerate() {
                    v.set(i, Item::new(k));
                }
                v
            },
            BatchSize::LargeInput,
        )
    });

    group.finish();
}

fn bench_diverge_100k(c: &mut Criterion) {
    bench_diverge(c, 100_000);
}

fn bench_diverge_1m(c: &mut Criterion) {
    bench_diverge(c, 1_000_000);
}

/// The driving workload: a chain of versions, each derived from the last
/// with 50 scattered replacements, all versions kept alive.
fn bench_chained_generations(c: &mut Criterion) {
    const N: usize = 1_000_000;
    const GENS: usize = 32;
    const EDITS: usize = 50;
    let mut group = c.benchmark_group(format!("chained_{GENS}_generations_{N}"));
    group.sample_size(10);
    let indices = scattered_indices(GENS * EDITS, N);

    let cow_base = CowVec::from(source(N));
    group.bench_function("CowVec", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut versions = vec![cow_base.clone()];
                for g in 0..GENS {
                    let mut next = versions.last().unwrap().clone();
                    for (k, &i) in indices[g * EDITS..(g + 1) * EDITS].iter().enumerate() {
                        next.set(i, Item::new(k));
                    }
                    versions.push(next);
                }
                versions
            },
            BatchSize::LargeInput,
        )
    });

    let paged_base = PagedVec::<Item>::from(source(N));
    group.bench_function("PagedVec", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut versions = vec![paged_base.clone()];
                for g in 0..GENS {
                    let mut next = versions.last().unwrap().clone();
                    for (k, &i) in indices[g * EDITS..(g + 1) * EDITS].iter().enumerate() {
                        next.set(i, Item::new(k));
                    }
                    versions.push(next);
                }
                versions
            },
            BatchSize::LargeInput,
        )
    });

    let arc_vec_arc_base: Arc<Vec<Arc<Item>>> =
        Arc::new(source(N).into_iter().map(Arc::new).collect());
    group.bench_function("Arc<Vec<Arc>>", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut versions = vec![arc_vec_arc_base.clone()];
                for g in 0..GENS {
                    let mut next = versions.last().unwrap().clone();
                    let inner = Arc::make_mut(&mut next);
                    for (k, &i) in indices[g * EDITS..(g + 1) * EDITS].iter().enumerate() {
                        inner[i] = Arc::new(Item::new(k));
                    }
                    versions.push(next);
                }
                versions
            },
            BatchSize::LargeInput,
        )
    });

    let imbl_base: imbl::Vector<Item> = source(N).into_iter().collect();
    group.bench_function("imbl", |b| {
        b.iter_batched(
            || (),
            |()| {
                let mut versions = vec![imbl_base.clone()];
                for g in 0..GENS {
                    let mut next = versions.last().unwrap().clone();
                    for (k, &i) in indices[g * EDITS..(g + 1) * EDITS].iter().enumerate() {
                        next.set(i, Item::new(k));
                    }
                    versions.push(next);
                }
                versions
            },
            BatchSize::LargeInput,
        )
    });

    group.finish();
}

fn bench_random_access(c: &mut Criterion) {
    const N: usize = 1_000_000;
    const READS: usize = 10_000;
    let mut group = c.benchmark_group(format!("random_access_{READS}_of_{N}"));
    group.sample_size(20);
    let indices = scattered_indices(READS, N);

    let vec_base = source(N);
    group.bench_function("Vec", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(vec_base[i].id);
            }
            black_box(sum)
        })
    });

    let arc_vec_arc_base: Vec<Arc<Item>> = source(N).into_iter().map(Arc::new).collect();
    group.bench_function("Vec<Arc>", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(arc_vec_arc_base[i].id);
            }
            black_box(sum)
        })
    });

    let cow_base = CowVec::from(source(N));
    group.bench_function("CowVec", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(cow_base[i].id);
            }
            black_box(sum)
        })
    });

    let paged_base = PagedVec::<Item>::from(source(N));
    group.bench_function("PagedVec", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(paged_base[i].id);
            }
            black_box(sum)
        })
    });

    let imbl_base: imbl::Vector<Item> = source(N).into_iter().collect();
    group.bench_function("imbl", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(imbl_base[i].id);
            }
            black_box(sum)
        })
    });

    group.finish();
}

fn bench_iterate(c: &mut Criterion) {
    const N: usize = 1_000_000;
    let mut group = c.benchmark_group(format!("iterate_{N}"));
    group.sample_size(20);

    let vec_base = source(N);
    group.bench_function("Vec", |b| {
        b.iter(|| black_box(vec_base.iter().map(|item| item.id).sum::<u64>()))
    });

    let arc_vec_arc_base: Vec<Arc<Item>> = source(N).into_iter().map(Arc::new).collect();
    group.bench_function("Vec<Arc>", |b| {
        b.iter(|| black_box(arc_vec_arc_base.iter().map(|item| item.id).sum::<u64>()))
    });

    let cow_base = CowVec::from(source(N));
    group.bench_function("CowVec", |b| {
        b.iter(|| black_box(cow_base.iter().map(|item| item.id).sum::<u64>()))
    });

    let paged_base = PagedVec::<Item>::from(source(N));
    group.bench_function("PagedVec", |b| {
        b.iter(|| black_box(paged_base.iter().map(|item| item.id).sum::<u64>()))
    });

    let imbl_base: imbl::Vector<Item> = source(N).into_iter().collect();
    group.bench_function("imbl", |b| {
        b.iter(|| black_box(imbl_base.iter().map(|item| item.id).sum::<u64>()))
    });

    group.finish();
}

/// A deterministic permutation of `0..len`.
fn permutation(len: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let mut state = 0x9E3779B97F4A7C15u64;
    for i in (1..len).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    order
}

/// Reads after heavy editing. The clean read benchmarks above use freshly
/// built vectors whose values sit in one contiguous chunk in index order;
/// here every element has been replaced in scattered order, so live values
/// are spread across arena chunks with no relation to their index. This is
/// the honest number for a long-lived, much-edited vector, and `compact`
/// shows what it takes to get the clean number back.
fn bench_dirty_reads(c: &mut Criterion) {
    const N: usize = 1_000_000;
    const READS: usize = 10_000;
    let order = permutation(N);
    let indices = scattered_indices(READS, N);

    let mut cow_dirty = CowVec::from(source(N));
    let mut paged_dirty = PagedVec::<Item>::from(source(N));
    for &i in &order {
        cow_dirty.set(i, Item::new(i));
        paged_dirty.set(i, Item::new(i));
    }
    let mut cow_compacted = cow_dirty.clone();
    cow_compacted.compact(0);
    // The same scattering applied to a plain Vec<Arc<T>>: each Arc is
    // allocated in permuted order.
    let mut arcs: Vec<Option<Arc<Item>>> = vec![None; N];
    for &i in &order {
        arcs[i] = Some(Arc::new(Item::new(i)));
    }
    let vec_arc_dirty: Vec<Arc<Item>> = arcs.into_iter().map(Option::unwrap).collect();

    let mut group = c.benchmark_group(format!("random_access_dirty_{READS}_of_{N}"));
    group.sample_size(20);
    group.bench_function("Vec<Arc>", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(vec_arc_dirty[i].id);
            }
            black_box(sum)
        })
    });
    group.bench_function("CowVec", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(cow_dirty[i].id);
            }
            black_box(sum)
        })
    });
    group.bench_function("PagedVec", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(paged_dirty[i].id);
            }
            black_box(sum)
        })
    });
    group.bench_function("CowVec compacted", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &i in &indices {
                sum = sum.wrapping_add(cow_compacted[i].id);
            }
            black_box(sum)
        })
    });
    group.finish();

    let mut group = c.benchmark_group(format!("iterate_dirty_{N}"));
    group.sample_size(20);
    group.bench_function("Vec<Arc>", |b| {
        b.iter(|| black_box(vec_arc_dirty.iter().map(|item| item.id).sum::<u64>()))
    });
    group.bench_function("CowVec", |b| {
        b.iter(|| black_box(cow_dirty.iter().map(|item| item.id).sum::<u64>()))
    });
    group.bench_function("PagedVec", |b| {
        b.iter(|| black_box(paged_dirty.iter().map(|item| item.id).sum::<u64>()))
    });
    group.bench_function("CowVec compacted", |b| {
        b.iter(|| black_box(cow_compacted.iter().map(|item| item.id).sum::<u64>()))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_clone,
    bench_diverge_100k,
    bench_diverge_1m,
    bench_chained_generations,
    bench_random_access,
    bench_iterate,
    bench_dirty_reads,
);
criterion_main!(benches);
