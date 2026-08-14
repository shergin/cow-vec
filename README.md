# cow_vec

Vectors optimized for O(1) clone. When a clone diverges, you copy pointers —
never elements.

The crate provides two containers with identical semantics and different
divergence/access trade-offs:

| | `CowVec<T>` | `PagedVec<T>` |
|---|---|---|
| Layout | one flat pointer array | pointer pages behind a small root table |
| `clone()` | O(1) | O(1) |
| First mutation after clone | copies the whole pointer array | copies the root table + touched pages |
| Diverge 50 elements of 4M | ~32 MB copied | ~0.4 MB copied |
| Indexed access | 2 dependent loads | 3 dependent loads (root stays in cache) |
| `pop` / `truncate` after clone | copies pointer array once | copies nothing |
| Extras | `sort`/`dedup`/`binary_search`, `append`, `split_off`, `splice`, `as_slice` | core API |

Rule of thumb: **`CowVec`** when clones diverge rarely or all at once and reads
dominate; **`PagedVec`** when you keep deriving new versions from old ones —
version chains, undo history, branch-and-discard search.

## Quick start

```rust
use cow_vec::CowVec;

let base = CowVec::from(vec![
    String::from("alpha"),
    String::from("beta"),
    String::from("gamma"),
]);

// O(1): shares storage and structure with `base`.
let mut branch = base.clone();

// Copy-on-write: only `branch` sees the change. The Strings in `base`
// are never cloned.
branch.set(1, String::from("BETA"));
branch.make_mut(2).push_str("!");

assert_eq!(base.to_vec(), ["alpha", "beta", "gamma"]);
assert_eq!(branch.to_vec(), ["alpha", "BETA", "gamma!"]);
```

`PagedVec` has the same API; it earns its keep when versions chain:

```rust
use cow_vec::PagedVec;

let mut versions: Vec<PagedVec<u64>> = vec![(0..100_000).collect()];

// Each generation: clone the last version, replace a few elements.
// Cost per generation: the root table plus one 8 KB page per touched
// page - not the whole vector.
for gen in 1..10 {
    let mut next = versions.last().unwrap().clone();
    next.set(50_000, gen);
    next.set(99_999, gen);
    versions.push(next);
}

assert_eq!(versions[0][50_000], 50_000); // every version intact
assert_eq!(versions[9][50_000], 9);
```

Cheap permutation is a side effect of the pointer representation: sorting a
`CowVec` of megabyte-sized structs moves 8-byte pointers, never the structs.

```rust
use cow_vec::CowVec;

let mut vec = CowVec::from(vec![
    String::from("pear"),
    String::from("apple"),
    String::from("plum"),
]);
vec.sort_by(|a, b| a.cmp(b)); // permutes pointers only
assert_eq!(vec.to_vec(), ["apple", "pear", "plum"]);
```

Concatenation copies pointers, not elements:

```rust
use cow_vec::CowVec;

let mut a = CowVec::from(vec![1, 2]);
let b = CowVec::from(vec![3, 4]);
a.append(&b); // b's storage is kept alive; nothing is cloned
assert_eq!(a, vec![1, 2, 3, 4]);
```

## Choosing a container (including someone else's)

The honest comparison set, for a vector you clone often and mutate a little:

| | clone | diverge k of n elements | access | reclaims memory | unsafe-free |
|---|---|---|---|---|---|
| `Vec<T>` | deep copy | (clone is the cost) | contiguous, fastest | yes | yes |
| `Arc<Vec<T>>` + `make_mut` | O(1) | **clones all n elements** | contiguous, fastest | yes | yes |
| `Arc<Vec<Arc<T>>>` + `make_mut` | O(1) | n Arc bumps + k allocs | 1 deref | per element | yes |
| `imbl::Vector<T>` | O(1) | O(k log n) node copies | ~5 dependent loads at 4M | automatic | yes |
| `CowVec<T>` | O(1) | n pointer memcpy, once | 1 deref | via compaction | no |
| `PagedVec<T>` | O(1) | root + touched pages | 2 derefs | via compaction | no |

- If `T` is cheap to clone (numbers, small structs), use **`Arc<Vec<T>>`** and
  stop reading — element clones are the whole cost this crate avoids, and
  yours are free.
- If you need automatic reclamation and heavy editing across many versions,
  **`imbl`** is excellent; its trade is tree-depth access.
- This crate's niche is **expensive-to-clone elements + snapshot-style
  workloads + flat, predictable access latency**. Divergence costs pointer
  copies; access stays fixed-depth.

## How it works

Both containers separate *structure* from *storage*:

```text
CowVec                                PagedVec
------                                --------
items: Arc<Vec<*const T>>             pages: Arc<Vec<Arc<Page>>>
            |                                    |         Page = [*const T; 1024]
            v                                    v
       +---------------------- storage ----------------------+
       |  active bump arena  <-  frozen keep-alive chain     |
       +-----------------------------------------------------+
```

**Structure** is the pointer table, shared via `Arc` and copied on first
write (`Arc::make_mut`). `CowVec` copies it wholesale; `PagedVec` copies the
root table and individual 8 KB pages.

**Storage** is append-only bump arenas. Each instance allocates only into an
arena it *uniquely owns* — the first allocation after a clone freezes the
now-shared arena onto an `Arc` keep-alive chain and starts a fresh one. That
makes every `push`/`set` lock-free: clones on different threads never
contend. Values never move, so the raw pointers in the structure stay valid
for as long as any descendant holds the chain.

## Performance characteristics

| Operation | `CowVec` | `PagedVec` |
|---|---|---|
| `clone()` | O(1) | O(1) |
| first mutation after clone | O(n) pointer memcpy | O(PAGE_SIZE) per touched page |
| `get()` / `[i]` | O(1), 2 loads | O(1), 3 loads |
| `push()` | O(1) amortized, no lock | O(1) amortized, no lock |
| `set()` | O(1) + COW | O(1) + page COW |
| `pop()` / `truncate()` | O(1) + COW | O(1), copies nothing |
| `sort_*()` | O(n log n) pointer swaps | — |
| `append()` | O(m) pointer copies | — |
| bulk build (`from`/`collect`) | one allocation batch, values contiguous | same |

### Benchmarks

Median criterion times on an Apple M1 Pro. Elements are structs carrying a
`String`, so element clones cost real heap allocations — the regime these
containers target. "50 edits" replaces 50 scattered elements after a clone;
"chained" derives 32 such generations from each other, keeping every
version.

| Scenario | `Vec` | `Arc<Vec>` | `Arc<Vec<Arc>>` | `imbl` | `CowVec` | `PagedVec` |
|---|---|---|---|---|---|---|
| clone, 100k | 2.6 ms | 9.8 ns | — | 24 ns | 11 ns | 12 ns |
| clone + 50 edits, 100k | 1.4 ms | 1.4 ms | 262 µs | 56 µs | 17 µs | **14 µs** |
| clone + 50 edits, 1M | 13.8 ms | 14.2 ms | 2.6 ms | 69 µs | 171 µs | **23 µs** |
| 32 chained generations, 1M | — | — | 82 ms | 2.6 ms | 7.9 ms | **1.0 ms** |
| 10k random reads, 1M | **7.9 µs** | — | 15.9 µs¹ | 377 µs | 16.9 µs | 17.2 µs |
| full iteration, 1M | **0.62 ms** | — | 1.07 ms¹ | 2.9 ms | 0.69 ms | 0.81 ms |
| build from `Vec`, 100k | — | — | — | 923 µs | 265 µs | **131 µs** |

¹ measured as `Vec<Arc<T>>` (the read path is identical).

What the numbers say:

- **Divergence is where this crate lives.** Producing a 50-edit version of a
  1M-element vector: `PagedVec` 23 µs vs `imbl` 69 µs vs `CowVec` 171 µs vs
  the idiomatic `Arc<Vec<Arc<T>>>` at 2.6 ms — and `Arc<Vec<T>>` pays the
  full 14 ms element-clone bill that O(1)-clone types exist to avoid.
- **Access stays flat.** Random reads through `CowVec`/`PagedVec` cost the
  same as through a plain `Vec<Arc<T>>` (one pointer chase) and ~22× less
  than `imbl`'s tree walk. Iteration lands within ~10–30% of plain `Vec`.
- **The chained-generations workload** — the reason `PagedVec` exists —
  runs 7.5× faster than `CowVec` (which re-copies the full pointer table
  every generation) and 2.5× faster than `imbl`.

Reproduce with `cargo bench`. Numbers move with hardware; the *ratios* are
the point.

## Memory model and compaction

Storage is append-only: `set`, `pop`, `remove`, and `clear` leave old values
allocated, because other clones may still reference them and the arenas give
out stable pointers. A long-lived, frequently mutated vector therefore
accumulates garbage. Two tools manage it:

```rust
use cow_vec::CowVec;

let mut vec = CowVec::from(vec![1, 2, 3]);
for i in 0..100 {
    vec.set(0, i); // each set allocates; old values stay behind
}
assert_eq!(vec.storage_allocations(), 103); // 3 live + 100 garbage

// Rebuild into fresh, contiguous storage once past a threshold:
let compacted = vec.clone_compacted(50);
assert_eq!(compacted.storage_allocations(), 3);
assert_eq!(compacted, vec);
```

Dropping every clone of a lineage frees all of its arenas; keep-alive chains
drop iteratively, so version chains hundreds of thousands of generations
deep unwind without recursion.

## Limitations

- **No `&mut T` without cost**: `make_mut` clones the value to a fresh slot
  every call. There is no in-place mutation that other clones could observe.
- **`pop`/`remove`/`splice` need `T: Clone`** to return owned values (the
  original must stay in storage for other clones). `truncate` works for any
  `T`.
- **Garbage until compaction**, as described above. Poor fit for long-lived
  collections with unbounded mutation and no compaction points.
- **Small cheap elements**: the pointer indirection and per-element
  allocation are pure overhead — use `Vec` or `Arc<Vec<T>>`.
- `PagedVec` currently offers the core API only (no `insert`/`remove`/
  `sort`/`splice`).

## Safety and testing

The crate is built on raw pointers into append-only arenas, so it treats
verification as a feature:

- The **entire test suite runs under Miri** (Stacked Borrows) in CI — the
  concurrency tests included. Pointer provenance is preserved through every
  allocation path.
- Both containers pass one **shared behavior suite** (the parity contract),
  `PagedVec` at page sizes 1024 and 8 so page-boundary logic is hammered.
- Page-granularity tests assert *which* pages diverge on mutation, not just
  that values end up correct.

## Migration from 1.x

| 1.x | 2.0 |
|---|---|
| `vec[i] = x` (via `IndexMut`) | `vec.set(i, x)` |
| `vec[i] += 1` | `*vec.make_mut(i) += 1` |
| `vec.pop()` → `Option<&T>` | `vec.pop()` → `Option<T>` (`T: Clone`) |
| `vec.remove(i)` → `&T` | `vec.remove(i)` → `T` (`T: Clone`) |
| `vec.splice(..)` → `Vec<&T>` | `vec.splice(..)` → `Vec<T>` |
| `vec.position(p)` | `vec.iter().position(p)` |
| `vec.clone_with_max_capacity(n)` | `vec.clone_compacted(n)` |

1.x also had a soundness bug in `IndexMut` (a write through a pointer that
had lost write provenance — undefined behavior, found by Miri). 2.0 fixes
the provenance handling everywhere and removes the footgun API.

## License

MIT
