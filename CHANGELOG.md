# Changelog

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
