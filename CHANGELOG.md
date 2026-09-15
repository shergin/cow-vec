# Changelog

## Unreleased

### Added

- **`compact(max_allocations)`** on both types: rebuilds the storage in
  place, moving the elements when nothing else references it and cloning
  them otherwise. `clone_compacted` still always clones.
- **Owned `IntoIterator`** for both types (`T: Clone`): `for x in vec`
  moves each element out when nothing else references the storage and
  clones it otherwise, the same rule as `pop`. Note for existing code:
  `vec.into_iter()` on an owned vector now yields `T` instead of
  auto-referencing to the `&T` iterator; use `vec.iter()` for references.
- **`CowVector<T>` trait** naming the API both types share (`len`, `get`,
  `iter`, `push`, `pop`, `set`, `make_mut`, `truncate`, `compact`, and
  the sharing introspection), so code can be generic over which vector
  it snapshots.
- `PagedVec::append`, which shares the other vector's pages outright when
  this vector's length is page-aligned and copies pointers otherwise;
  `PagedVec::insert`, `remove`, and `splice`, which rewrite the pages
  after the index; `PagedVec::swap`; and the `sort`/`sort_by`/
  `sort_by_key`/`sort_unstable`/`sort_unstable_by` family on `PagedVec`,
  all reordering pointers only.

### Changed

- **A sole owner moves and drops values.** When no clone or ancestor
  shares the storage, `pop`, `remove`, and `splice` move values out
  instead of cloning them; `set`, `truncate`, `clear`, `retain`, and
  `dedup` drop the replaced values immediately; and `make_mut` returns
  the slot in place without allocating. Shared storage behaves as before.
- **Dead ancestors are reclaimed.** A vector that outlives its clones
  takes their arenas back on its next mutation, and values from those
  arenas can be moved like any other. `is_storage_shared` is now exact:
  it turns `false` as soon as the last other owner is gone, instead of
  staying `true` forever.
- **A sole owner reuses freed slots.** Slots vacated by a move-out or an
  in-place drop are handed to later allocations, so mutating a vector
  nothing else shares no longer grows its storage. `storage_allocations`
  counts slots, reused ones once; garbage now only accumulates for values
  a clone could still see.
- **Storage no longer depends on `typed-arena`.** Values live in a
  private chunk list that is only ever mutated through `Arc::get_mut`,
  so the hand-written `Sync` impl on the arena is gone; the remaining
  `unsafe impl`s cover only the raw-pointer tables.
- **`From<Vec<T>>` takes over the vector's buffer** as storage for both
  types. No element is moved or copied.
- `CowVec::clear`, `truncate`, and `retain` on a shared pointer table copy
  only the surviving prefix; `clear` copies nothing.
- `PagedVec` releases trailing pages left behind by a shared `truncate`
  on the next write that owns the root table.
- `PagedVecIter` walks pages directly instead of calling `get` per
  element; full iteration is about 12% faster and `fold`/`rfold` run as
  slice walks.

### Fixed

- Doc comments still described 1.x's arena lock; allocation is unlocked
  single-owner, not "lock-free" (Arc atomics remain). Access cost is now
  described consistently as pointer hops.

## 2.0.0 — 2026-08-14

### Added

- **`PagedVec<T, PAGE_SIZE = 1024>`** - a second vector type with
  page-granular copy-on-write. Diverging a clone copies only the touched
  pages instead of the whole pointer table; `pop` and `truncate` copy
  nothing. Costs one extra dependent load on access.
- **Lock-free storage engine** shared by both types. Values live in
  per-instance bump arenas with an `Arc` keep-alive chain; `push`/`set`
  never take a lock and clones on different threads allocate with zero
  contention. (1.x guarded one shared arena with a `Mutex`.)
- `CowVec::append(&other)` - concatenation by pointer copy; no element is
  moved or cloned.
- `CowVec::make_mut(index)` - explicit-cost mutable access, replacing
  `IndexMut` (see Changed).
- Pointer-permutation operations on `CowVec`: `sort`, `sort_by`,
  `sort_by_key`, `sort_unstable`, `sort_unstable_by`, `rotate_left`,
  `rotate_right`, `dedup`, `dedup_by`, `binary_search`,
  `binary_search_by` - all reorder 8-byte pointers, never elements.
- Standard traits on both types: `PartialEq`/`Eq` (with O(1) shared-
  structure fast path, plus comparisons against `Vec` and slices),
  `PartialOrd`/`Ord`, `Hash`, `FromIterator`, `Extend`, `From<&[T]>`.
- `serde` support behind the `serde` feature; wire format matches `Vec`.
- `storage_allocations()` for deciding when to compact.
- Iterators are now `DoubleEndedIterator` + `FusedIterator` + `Clone`
  with O(1) `nth`; `CowVecIter` is backed by a slice iterator.
- CI running tests, rustfmt, clippy, and Miri.

### Changed

- **`IndexMut` removed.** `vec[i] = x` silently allocated a fresh arena
  value on every access, even without a write. Use `set(i, x)` to replace
  a value or `make_mut(i)` for in-place mutation with an explicit
  copy-on-write cost, mirroring `Arc::make_mut`.
- **`pop`/`remove` return owned `T`** (requires `T: Clone`) instead of
  references tied to the `&mut self` borrow, which made call sites like
  popping in a loop impossible. Use `truncate` to drop elements of
  non-`Clone` types. `splice` returns `Vec<T>` for the same reason.
- **`clone_with_max_capacity` renamed to `clone_compacted`.** The
  operation is compaction, not capacity control. The threshold now counts
  this lineage's allocations rather than the globally shared arena's.
- `is_storage_shared` is now conservative: storage retaining frozen
  ancestor arenas reports `true` even after other owners dropped.
- Bulk operations (`From<Vec>`, `extend`, `collect`, `splice`) allocate
  in one batch and store values contiguously (1.4 locked per element).

### Removed

- `position()` - use `iter().position()`.

### Fixed

- Undefined behavior in the 1.x `IndexMut` path: the arena returned
  pointers that had lost write provenance (caught by Miri's Stacked
  Borrows checker); writing through them was UB. All allocation now
  preserves provenance, and the full test suite runs under Miri in CI.

## 1.4.0 and earlier

See the git history.
