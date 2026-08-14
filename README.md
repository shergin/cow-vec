# cow_vec

[![CI](https://github.com/shergin/cow-vec/actions/workflows/ci.yml/badge.svg)](https://github.com/shergin/cow-vec/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cow_vec.svg)](https://crates.io/crates/cow_vec)
[![docs.rs](https://img.shields.io/docsrs/cow_vec)](https://docs.rs/cow_vec)

Clone a vector in constant time. When that clone starts writing, you copy
pointers — never the elements themselves.

You get two types that behave the same from the outside and disagree about
what happens the first time a clone writes:

| | `CowVec<T>` | `PagedVec<T>` |
|---|---|---|
| Layout | one flat pointer array | pointer pages behind a small root table |
| `clone()` | O(1) | O(1) |
| First mutation after clone | copies the whole pointer array | copies the root table + touched pages |
| Diverge 50 elements of 4M | ~32 MB copied | ~0.4 MB copied |
| Indexed access | 2 dependent loads | 3 dependent loads (root stays in cache) |
| `pop` / `truncate` after clone | copies pointer array once | copies nothing |
| Extras | `sort`/`dedup`/`binary_search`, `append`, `split_off`, `splice`, `as_slice` | core API |

**CowVec** is the one you want when clones rarely write — or they rewrite
everything at once — and most of the time you are just reading. **PagedVec**
is for the other life: you keep deriving new versions from old ones.
Version chains, undo history, branch-and-discard search.

## Quick start

```rust
use cow_vec::CowVec;

let base = CowVec::from(vec![
    String::from("alpha"),
    String::from("beta"),
    String::from("gamma"),
]);

// Instant. Shares storage and structure with `base`.
let mut branch = base.clone();

// Only `branch` sees this. The Strings in `base` are never cloned.
branch.set(1, String::from("BETA"));
branch.make_mut(2).push_str("!");

assert_eq!(base.to_vec(), ["alpha", "beta", "gamma"]);
assert_eq!(branch.to_vec(), ["alpha", "BETA", "gamma!"]);
```

`PagedVec` has the same API. It starts earning its keep once versions
chain:

```rust
use cow_vec::PagedVec;

let mut versions: Vec<PagedVec<u64>> = vec![(0..100_000).collect()];

// Each generation: clone the last version, poke a couple of elements.
// You pay for the root table plus one 8 KB page per page you touch —
// not the whole vector.
for gen in 1..10 {
    let mut next = versions.last().unwrap().clone();
    next.set(50_000, gen);
    next.set(99_999, gen);
    versions.push(next);
}

assert_eq!(versions[0][50_000], 50_000); // every version is still intact
assert_eq!(versions[9][50_000], 9);
```

A nice side effect of the pointer representation: sorting a `CowVec` of
megabyte-sized structs shuffles 8-byte pointers. The structs stay put.

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

Concatenation is the same story — pointers move, elements don't:

```rust
use cow_vec::CowVec;

let mut a = CowVec::from(vec![1, 2]);
let b = CowVec::from(vec![3, 4]);
a.append(&b); // b's storage stays alive; nothing is cloned
assert_eq!(a, vec![1, 2, 3, 4]);
```

## Choosing a container (including someone else's)

You clone a lot and mutate a little. Here is the honest lineup:

| | clone | diverge k of n elements | access | reclaims memory | unsafe-free |
|---|---|---|---|---|---|
| `Vec<T>` | deep copy | (clone is the cost) | contiguous, fastest | yes | yes |
| `Arc<Vec<T>>` + `make_mut` | O(1) | **clones all n elements** | contiguous, fastest | yes | yes |
| `Arc<Vec<Arc<T>>>` + `make_mut` | O(1) | n Arc bumps + k allocs | 1 deref | per element | yes |
| `imbl::Vector<T>` | O(1) | O(k log n) node copies | ~5 dependent loads at 4M | automatic | yes |
| `CowVec<T>` | O(1) | n pointer memcpy, once | 1 deref | via compaction | no |
| `PagedVec<T>` | O(1) | root + touched pages | 2 derefs | via compaction | no |

- If `T` is cheap to clone — numbers, small structs — use **`Arc<Vec<T>>`**
  and stop reading. Element clones are the cost this crate exists to avoid,
  and yours are free.
- If you want automatic reclamation and you edit heavily across many
  versions, **`imbl`** is excellent. The trade is tree-depth access.
- This crate's niche is **expensive-to-clone elements + snapshot-style
  work + flat, predictable access**. Divergence costs pointer copies.
  Access stays a fixed number of loads.

## How it works

Both types split *structure* from *storage*:

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

**Structure** is the pointer table. It is shared through `Arc` and copied
on first write (`Arc::make_mut`). `CowVec` copies the whole thing;
`PagedVec` copies the root table and whichever 8 KB pages you actually
touch.

**Storage** is append-only bump arenas. An instance only allocates into an
arena it *uniquely owns*. The first allocation after a clone freezes the
now-shared arena onto an `Arc` keep-alive chain and starts a fresh one.
That is why every `push`/`set` is lock-free: clones on different threads
never fight over the same arena. Values never move, so the raw pointers in
the structure stay valid as long as any descendant is still holding the
chain.

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

Median criterion times on an Apple M1 Pro. Elements are structs that own a
`String`, so cloning one actually allocates — the world these containers
are built for. "50 edits" replaces 50 scattered elements after a clone.
"chained" derives 32 such generations from each other and keeps every
version around.

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

What the numbers actually mean:

- **Divergence is where this crate lives.** A 50-edit version of a
  1M-element vector: `PagedVec` 23 µs, `imbl` 69 µs, `CowVec` 171 µs, and
  the usual `Arc<Vec<Arc<T>>>` at 2.6 ms. `Arc<Vec<T>>` just pays the full
  14 ms element-clone bill — the one O(1)-clone types exist to spare you.
- **Access stays flat.** Random reads through `CowVec`/`PagedVec` cost
  about the same as through a plain `Vec<Arc<T>>` (one pointer chase) and
  ~22× less than walking `imbl`'s tree. Iteration lands within ~10–30% of
  a plain `Vec`.
- **Chained generations** are why `PagedVec` exists. That workload runs
  7.5× faster than `CowVec` (which recopies the whole pointer table every
  generation) and 2.5× faster than `imbl`.

Reproduce with `cargo bench`. Hardware moves the numbers; the *ratios*
are the point.

## Memory model and compaction

Storage is append-only. `set`, `pop`, `remove`, and `clear` leave the old
values sitting there, because some other clone might still be pointing at
them, and the arenas hand out pointers that cannot move. A long-lived
vector you keep mutating therefore collects garbage. Two tools for that:

```rust
use cow_vec::CowVec;

let mut vec = CowVec::from(vec![1, 2, 3]);
for i in 0..100 {
    vec.set(0, i); // each set allocates; old values stay behind
}
assert_eq!(vec.storage_allocations(), 103); // 3 live + 100 garbage

// Rebuild into fresh, contiguous storage once you are past a threshold:
let compacted = vec.clone_compacted(50);
assert_eq!(compacted.storage_allocations(), 3);
assert_eq!(compacted, vec);
```

Drop every clone of a lineage and all of its arenas go with it. Keep-alive
chains drop iteratively, so a version chain hundreds of thousands of
generations deep unwinds without blowing the stack.

## Limitations

- **No free `&mut T`.** `make_mut` clones the value into a fresh slot
  every time. There is no in-place mutation that other clones could see —
  that is the point.
- **`pop`/`remove`/`splice` need `T: Clone`** so they can hand you an
  owned value (the original has to stay in storage for everyone else).
  `truncate` works for any `T`.
- **Garbage until you compact**, as above. A poor fit for a long-lived
  collection that mutates forever and never gets a compaction point.
- **Small cheap elements** do not belong here. The pointer hop and
  per-element allocation are pure overhead — use `Vec` or `Arc<Vec<T>>`.
- `PagedVec` currently has the core API only (no `insert`/`remove`/
  `sort`/`splice`).

## Safety and testing

This crate is raw pointers into append-only arenas, so verification is
part of the product:

- The **entire test suite runs under Miri** (Stacked Borrows) in CI —
  concurrency tests included. Pointer provenance is preserved through
  every allocation path.
- Both types share one **behavior suite** (the parity contract).
  `PagedVec` runs it at page sizes 1024 and 8, so page-boundary logic is
  hammered.
- Page-granularity tests check *which* pages diverge on a mutation, not
  just that the values come out right.

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

1.x also had a soundness bug in `IndexMut`: a write through a pointer that
had lost write provenance — undefined behavior, found by Miri. 2.0 fixes
the provenance handling everywhere and takes the footgun API out.

## License

MIT
