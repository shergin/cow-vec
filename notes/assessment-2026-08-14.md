# CowVec Assessment — API, Performance, Positioning

*2026-08-14. Based on v1.4.0 (crates.io: 217 downloads). Test suite run under Miri as part of this review.*

## 1. Confirmed soundness bug: `IndexMut` is UB (Miri-verified)

Under Miri: 96 tests pass cleanly — the core pointer/arena design holds up — but **all 4 `index_mut` tests fail with Stacked Borrows violations**. The root cause is `src/cow_vec.rs:39`:

```rust
let reference = arena.alloc(value);  // &mut T
reference as *const T                // <- pointer tagged SharedReadOnly
```

Casting `&mut T` directly to `*const T` goes through an implicit shared reborrow, so the resulting pointer only carries read permission. `index_mut` (`src/cow_vec.rs:567`) then casts it back to `*mut T` and writes through it — that write is UB. Miri's exact words: *"trying to retag for Unique permission, but that tag only grants SharedReadOnly permission."*

**Fix (one line):** `reference as *mut T as *const T` — raw-to-raw casts preserve provenance, so write permission survives the round-trip. Ship as a patch release regardless of anything else, and add Miri to CI so this class of bug can't regress. For a crate whose value proposition rests on unsafe pointer juggling, "tested under Miri" is also a trust signal worth advertising.

Beyond the patch, **remove `IndexMut` in a 2.0** (see §3) — the fact that it needed a 35-line "HIDDEN ALLOCATION" warning is the API telling you it shouldn't exist in that shape.

## 2. Performance

### Bulk operations take the mutex once per element

`From<Vec<T>>` (`src/cow_vec.rs:495`), `extend` (loops over `push`), and `splice` each do a full lock/unlock cycle per item. Building a 1M-element CowVec is a million mutex round-trips. `typed_arena` has `alloc_extend`, which takes an iterator and allocates under one lock — and it returns a contiguous slice, which has a second benefit: bulk-loaded values end up truly adjacent in memory, making the cache-locality story real rather than aspirational. **Cheapest big win in the crate.**

### The iterator leaves a lot on the table

`CowVecIter` does an index-plus-bounds-check per element and implements nothing beyond `next`/`size_hint`. Since `as_slice()` already exists, the iterator can wrap `std::slice::Iter<'a, *const T>` and map the deref — that gets `DoubleEndedIterator`, `FusedIterator`, O(1) `nth`, and much better codegen (forward `fold` to the slice iterator and internal iteration vectorizes the pointer loads). Right now `.iter().rev()` doesn't even compile.

### Bigger idea: eliminate the mutex entirely with an arena chain

The Mutex exists because all clones share one arena. Alternative structure:

```rust
pub struct CowVec<T> {
    active: Arc<Arena<T>>,        // only allocated into while uniquely owned
    frozen: Arc<Chain<T>>,        // keep-alive list of ancestor arenas
    items: Arc<Vec<*const T>>,
}
```

On `push`: if `Arc::strong_count(&self.active) == 1`, access is provably exclusive (`&mut self` is held, and nobody else can clone the Arc without `&self`) — bump-allocate with no synchronization at all. If shared (this instance was cloned), push the old arena onto the frozen chain and start a fresh one. Frozen arenas are never allocated into again, so an `unsafe impl Sync` newtype over them is justifiable.

Payoff: **zero locks on every path**, and — key for the stated "parallel exploration" use case — clones on different threads pushing concurrently no longer contend at all, where today they serialize on one mutex. Leak profile is unchanged (the chain keeps ancestors alive exactly like the shared arena does today), and `clone_with_max_capacity` naturally becomes chain compaction. A 2.0-sized change, but the design worth having.

### Benchmarks are the missing evidence

The README makes specific quantitative claims ("42M objects, ~6ms, 50 GB/s") with no `benches/` directory. For a performance-positioned crate this is the credibility gap: add criterion benches against `Vec<T>`, `Arc<Vec<T>>` + `make_mut`, `Vec<Arc<T>>`, `im`/`imbl`, and `ecow`, and put measured numbers in the README. The results are likely genuinely favorable for the clone-heavy/large-`T` scenario — free marketing left on the table.

## 3. API

### Replace `IndexMut` with `make_mut(index) -> &mut T`

Beyond the UB, `vec[0] += 1` silently allocating — and `&mut vec[0]` allocating even when never written — violates the principle that expensive operations look expensive. `Arc::make_mut` is the established Rust idiom for "give me exclusive access, COW-copying if needed"; borrowing that name makes the cost legible at the call site and lets the warning essay be deleted.

### `pop() -> Option<&T>` is nearly unusable

The returned reference keeps the *mutable* borrow of `self` alive, so you can't pop twice and hold both results, and `while let Some(x) = v.pop() { v.push(…) }` doesn't compile. Same issue for `remove` and `splice`'s `Vec<&T>`. For 2.0: make `pop`/`remove` return `Option<T>`/`T` where `T: Clone` (clone out of the arena), with `truncate` as the no-Clone escape hatch.

### Missing standard traits — biggest day-to-day ergonomics gap

- `PartialEq`/`Eq` — users can't `assert_eq!(a, b)` on two CowVecs today; the crate's own tests route through `to_vec()` for this reason. A smart impl can short-circuit: pointer-equal `items` Arcs ⇒ equal for free.
- `FromIterator` (no `.collect()` today) and the `Extend` trait (inherent method exists but generic code can't use it).
- `Hash`, `PartialOrd`/`Ord`, `From<&[T]> where T: Clone`.
- Feature-gated `serde` support — cheap to add, meaningful for adoption.

### Lean into cheap permutation — an under-advertised superpower

Elements are 8-byte pointers, so `sort_by`/`sort_unstable_by`/`sort_by_key` on a CowVec of large structs moves pointers, never elements — the classic "sort an index array" trick, built in. Same for `rotate_left/right` and `dedup_by`. None exist yet, and "sorting a vector of megabyte-sized elements without moving them" is a great README demo. Relatedly, `append(&mut self, other: &CowVec<T>)` between clones of the same family is just a pointer-extend with zero element clones — also distinctive.

### Smaller notes

- `clone_with_max_capacity` is a confusing name for what is really compaction — `clone_compacted(threshold)` or `compact(&mut self)` says what it does.
- `position()` duplicates `Iterator::position` for no gain.
- Add `rust-version` (MSRV) to Cargo.toml.
- Consider dual MIT/Apache-2.0 licensing (ecosystem norm, adds the patent grant).

## 4. Positioning

### The README argues against the wrong competitor

It compares mostly against `im`/`rpds`, but the alternatives a reviewer will raise first are:

- **`Arc<Vec<T>>` + `Arc::make_mut`** (and its polished forms, `ecow::EcoVec` / `shared_vector`): also O(1) clone, zero unsafe, no memory leak. CowVec's edge: first divergence costs a *pointer memcpy* instead of *cloning every element* — decisive when `T` is a String-and-HashMap-laden struct, irrelevant when `T` is `u64`.
- **`Vec<Arc<T>>` wrapped in `Arc`**: the fully-idiomatic version of the same idea, with automatic per-element reclamation. CowVec's edge: no per-element control-block overhead (~16–32 bytes/element), bump allocation instead of one malloc per element, and divergence is a plain memcpy instead of n atomic ref-count increments (plus n atomic decrements when the clone drops). CowVec's cost: garbage accrues until compaction, and unsafe code.

The second comparison *must* be in the README — "why not `Vec<Arc<T>>`?" is the first question any experienced Rust reviewer will ask, and there's a genuinely good answer.

### Sharpen the pitch to one sentence

> *"O(1) clone, O(1) index. When a clone diverges, you copy pointers — never elements."*

That's the entire differentiator, and it immediately communicates when to use it (expensive-to-clone elements; snapshot/branch/rollback workloads: backtracking solvers, undo history, rollback netcode, MVCC-style state) and when not to (cheap `T` — use `Arc<Vec<T>>`).

### Fix claims that are wrong or oversold

- README states `set()` requires `T: Clone` — it doesn't; it takes the value by move (plain `impl<T>` block).
- "im clone: O(1) or O(log n)" — `im::Vector` clone is O(1); the honest contrast is access (O(1) vs O(log n)) and mutation-after-clone (one O(n) pointer copy vs O(log n) forever).
- "Cache locality: excellent" needs nuance: the *pointer array* is contiguous; the *values* are contiguous only when bulk-loaded (which the `alloc_extend` change would guarantee) and scatter after churn from `set`/`push` across clones. Iteration always pays one pointer chase per element that `Vec<T>` doesn't.

## Priority order

1. Ship the one-line provenance fix + Miri in CI now.
2. `alloc_extend` for bulk ops and the slice-backed iterator — small diffs, large wins.
3. Missing traits (`PartialEq`, `FromIterator`, `Extend`, serde).
4. Criterion benches with published numbers; rewritten "why not `Arc<Vec<T>>` / `Vec<Arc<T>>`" README section.
5. Plan a 2.0: `make_mut` replacing `IndexMut`, owned-returning `pop`/`remove`, and — to be best-in-class for parallel branch exploration — the lock-free arena-chain redesign.
