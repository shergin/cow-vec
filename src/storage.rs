//! Value storage shared by the vector types in this crate.
//!
//! Values live in bump arenas. Each vector instance owns an *active* arena
//! that only it allocates into, plus a *frozen chain* of `Arc`s keeping every
//! ancestor arena alive for as long as any pointer into them may exist.
//!
//! Allocation never takes a lock. The safety story is ownership-based: an
//! instance allocates into its active arena only while it holds the sole
//! `Arc` to it (checked via `Arc::strong_count` under `&mut self`). The
//! moment an arena becomes shared - because the instance was cloned - the
//! next allocation pushes it onto the frozen chain and starts a fresh arena.
//! Frozen arenas are never allocated into again; they are held purely to
//! keep their memory alive.

use std::mem;
use std::sync::Arc;

use typed_arena::Arena;

/// A single bump arena.
///
/// Arenas are append-only: values are never removed or moved once allocated,
/// so raw pointers to them stay valid until the arena drops.
struct ArenaHandle<T> {
    arena: Arena<T>,
}

impl<T> ArenaHandle<T> {
    fn new() -> Self {
        Self {
            arena: Arena::new(),
        }
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            arena: Arena::with_capacity(capacity),
        }
    }
}

// SAFETY: `Arena` is not `Sync` because allocation goes through interior
// mutability (a RefCell). We assert `Sync` for the handle because this
// module never calls any `Arena` method through a shared handle:
// - Allocation happens only in `Storage::alloc`/`alloc_extend`, which first
//   prove unique ownership of the handle's `Arc` while holding `&mut self`.
//   With a strong count of 1 no other thread holds an `Arc` to clone, so
//   the count cannot change concurrently.
// - Every other owner (clones, chain nodes) holds the handle purely to keep
//   its memory alive and never touches the `Arena` API. Element reads go
//   through raw pointers, not through the arena.
// `T: Sync` because `&T` is exposed on multiple threads; `T: Send` because
// the last owner may drop the arena (and all values) on another thread.
unsafe impl<T: Send + Sync> Sync for ArenaHandle<T> {}

/// What a chain node keeps alive. The payloads are never read; they exist
/// so their `Drop` runs when the last referencing storage goes away.
#[allow(dead_code)]
enum Kept<T> {
    /// A frozen ancestor arena.
    Arena(Arc<ArenaHandle<T>>),
    /// A whole foreign chain, retained by [`Storage::absorb`].
    Chain(Arc<ChainNode<T>>),
}

/// Keep-alive node for storage that earlier generations allocated into.
struct ChainNode<T> {
    _kept: Kept<T>,
    next: Option<Arc<ChainNode<T>>>,
}

impl<T> Drop for ChainNode<T> {
    /// Unlinks the chain iteratively.
    ///
    /// A long-lived vector accumulates one node per diverged generation;
    /// the default recursive drop would overflow the stack on chains tens
    /// of thousands of nodes deep.
    fn drop(&mut self) {
        let mut next = self.next.take();
        while let Some(node) = next {
            match Arc::try_unwrap(node) {
                Ok(mut owned) => next = owned.next.take(),
                // Someone else still references the rest of the chain;
                // unlinking it is now their job.
                Err(_) => break,
            }
        }
    }
}

/// Value storage with unlocked single-owner allocation and O(1) clone.
///
/// Cloning shares the active arena and the frozen chain. The first
/// allocation after a clone freezes the (now shared) active arena and
/// starts a fresh private one.
pub(crate) struct Storage<T> {
    active: Arc<ArenaHandle<T>>,
    frozen: Option<Arc<ChainNode<T>>>,
    /// Number of values this storage keeps alive. Exact for a single
    /// lineage; an upper bound after [`absorb`](Self::absorb), which may
    /// double-count shared ancestry.
    allocated: usize,
}

impl<T> Storage<T> {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(ArenaHandle::new()),
            frozen: None,
            allocated: 0,
        }
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            active: Arc::new(ArenaHandle::with_capacity(capacity)),
            frozen: None,
            allocated: 0,
        }
    }

    /// Returns `true` if any other instance may still reference values in
    /// this storage. Conservative: a frozen chain whose other owners have
    /// all dropped still reports `true`.
    pub(crate) fn is_shared(&self) -> bool {
        Arc::strong_count(&self.active) > 1 || self.frozen.is_some()
    }

    /// Number of values this storage keeps alive, including ones no longer
    /// reachable through any vector. Upper bound after `absorb`.
    pub(crate) fn allocated(&self) -> usize {
        self.allocated
    }

    /// Makes `active` exclusively ours, freezing it first if it is shared.
    ///
    /// The strong-count check is sound because we hold `&mut self`: a count
    /// of 1 means the only `Arc` to this arena is our own field, and no
    /// other thread can clone an `Arc` it does not have.
    fn ensure_unique_active(&mut self) {
        if Arc::strong_count(&self.active) > 1 {
            let old = mem::replace(&mut self.active, Arc::new(ArenaHandle::new()));
            self.frozen = Some(Arc::new(ChainNode {
                _kept: Kept::Arena(old),
                next: self.frozen.take(),
            }));
        }
    }

    /// Allocates a value and returns a pointer valid for this storage's
    /// lifetime (and the lifetime of every storage that later absorbs it).
    pub(crate) fn alloc(&mut self, value: T) -> *const T {
        self.ensure_unique_active();
        self.allocated += 1;
        // SAFETY: `active` is uniquely owned (ensured above) and we hold
        // `&mut self`, so no other thread can reach this arena while we
        // exercise its interior mutability.
        let reference = self.active.arena.alloc(value);
        // Cast through *mut so the pointer keeps write provenance for
        // make_mut-style in-place initialization.
        reference as *mut T as *const T
    }

    /// Allocates every value from the iterator, contiguously, and returns
    /// their pointers.
    pub(crate) fn alloc_extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> Vec<*const T> {
        self.ensure_unique_active();
        // SAFETY: As in `alloc`.
        let slice = self.active.arena.alloc_extend(iter);
        self.allocated += slice.len();
        slice.iter_mut().map(|r| r as *mut T as *const T).collect()
    }

    /// Keeps every arena of `other` alive from this storage too, so raw
    /// pointers copied out of `other` stay valid for our lifetime.
    ///
    /// Used by `append`: concatenation copies pointers, never elements.
    pub(crate) fn absorb(&mut self, other: &Storage<T>) {
        self.frozen = Some(Arc::new(ChainNode {
            _kept: Kept::Arena(Arc::clone(&other.active)),
            next: self.frozen.take(),
        }));
        if let Some(chain) = &other.frozen {
            self.frozen = Some(Arc::new(ChainNode {
                _kept: Kept::Chain(Arc::clone(chain)),
                next: self.frozen.take(),
            }));
        }
        self.allocated += other.allocated;
    }
}

impl<T> Clone for Storage<T> {
    /// O(1): shares the active arena and the frozen chain.
    fn clone(&self) -> Self {
        Self {
            active: Arc::clone(&self.active),
            frozen: self.frozen.clone(),
            allocated: self.allocated,
        }
    }
}
