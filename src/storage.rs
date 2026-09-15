//! Value storage shared by the vector types in this crate.
//!
//! Values live in bump arenas. Each vector instance owns an *active* arena
//! that only it allocates into, plus a *frozen chain* of `Arc`s keeping every
//! ancestor arena alive for as long as any pointer into them may exist.
//!
//! Allocation never takes a lock. The safety story is ownership-based: an
//! instance allocates into its active arena only while it holds the sole
//! `Arc` to it (`Arc::get_mut` under `&mut self`). The moment an arena
//! becomes shared - because the instance was cloned - the next allocation
//! pushes it onto the frozen chain and starts a fresh arena. Frozen arenas
//! are never allocated into again; they are held purely to keep their memory
//! alive.

use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ptr;
use std::sync::Arc;

/// Size of the first chunk an arena opens on its own, in bytes. Chunks
/// double from there.
const INITIAL_CHUNK_BYTES: usize = 1024;

/// A fixed-capacity buffer of values.
///
/// A chunk never reallocates, so a pointer into it stays valid until the
/// chunk drops. The `Vec` is used purely as an allocation: its own length
/// stays 0 and it never touches the slots; `len` says how many slots are
/// initialized, and they are always a prefix.
struct Chunk<T> {
    buf: Vec<MaybeUninit<T>>,
    len: usize,
}

impl<T> Chunk<T> {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            len: 0,
        }
    }

    /// Takes over a `Vec`'s buffer as a chunk holding its elements.
    ///
    /// No element moves: the vector's allocation becomes the chunk.
    fn adopt(vec: Vec<T>) -> Self {
        let mut vec = ManuallyDrop::new(vec);
        let len = vec.len();
        let capacity = vec.capacity();
        let ptr = vec.as_mut_ptr() as *mut MaybeUninit<T>;
        // SAFETY: `MaybeUninit<T>` has the same size and alignment as `T`,
        // so the allocation matches; `len` is 0 because this chunk tracks
        // initialization itself. The `ManuallyDrop` wrapper hands the
        // buffer over without freeing it.
        let buf = unsafe { Vec::from_raw_parts(ptr, 0, capacity) };
        Self { buf, len }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.capacity() - self.len
    }

    /// Raw pointer to slot `index`.
    ///
    /// Derived from the allocation pointer itself, never through a
    /// reference to the slots, so it carries write provenance and creating
    /// it does not invalidate pointers handed out earlier.
    #[inline]
    fn slot(&mut self, index: usize) -> *mut T {
        debug_assert!(index <= self.capacity());
        // SAFETY: `index` stays within the allocation.
        unsafe { self.buf.as_mut_ptr().add(index) as *mut T }
    }

    /// Writes `value` into the next free slot. The caller ensures there is
    /// room.
    #[inline]
    fn push(&mut self, value: T) -> *const T {
        debug_assert!(self.len < self.capacity());
        let ptr = self.slot(self.len);
        // SAFETY: The slot is within capacity and not yet initialized.
        unsafe { ptr::write(ptr, value) };
        self.len += 1;
        ptr
    }

    /// Moves every element of `values` into the chunk, contiguously, and
    /// returns their pointers. The caller ensures there is room.
    fn push_all(&mut self, mut values: Vec<T>) -> Vec<*const T> {
        let n = values.len();
        debug_assert!(n <= self.remaining());
        let base = self.slot(self.len);
        // SAFETY: `n` free slots follow `len`; the source elements are moved
        // out and the vector's length is cleared so they are not dropped.
        unsafe {
            ptr::copy_nonoverlapping(values.as_ptr(), base, n);
            values.set_len(0);
        }
        self.len += n;
        // SAFETY: Every offset below `n` is an initialized slot.
        (0..n).map(|i| unsafe { base.add(i) } as *const T).collect()
    }
}

impl<T> Drop for Chunk<T> {
    fn drop(&mut self) {
        let len = self.len;
        let base = self.slot(0);
        // SAFETY: The first `len` slots are initialized, and the chunk is
        // dropping, so nothing references them any more.
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(base, len)) };
    }
}

/// A bump arena: chunks in allocation order, the last one being filled.
///
/// Values never move once allocated, so raw pointers to them stay valid
/// until the arena drops.
struct Arena<T> {
    chunks: Vec<Chunk<T>>,
}

impl<T> Arena<T> {
    fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    fn with_capacity(capacity: usize) -> Self {
        let mut chunks = Vec::new();
        if capacity > 0 {
            chunks.push(Chunk::with_capacity(capacity));
        }
        Self { chunks }
    }

    /// Returns the current chunk, opening a new one if it cannot take `n`
    /// more values.
    fn chunk_with_room(&mut self, n: usize) -> &mut Chunk<T> {
        let has_room = self.chunks.last().is_some_and(|c| c.remaining() >= n);
        if !has_room {
            let last = self.chunks.last().map_or(0, Chunk::capacity);
            let min = INITIAL_CHUNK_BYTES / mem::size_of::<T>().max(1);
            let capacity = n.max(last.saturating_mul(2)).max(min).max(1);
            self.chunks.push(Chunk::with_capacity(capacity));
        }
        self.chunks.last_mut().expect("a chunk was just ensured")
    }

    fn alloc(&mut self, value: T) -> *const T {
        self.chunk_with_room(1).push(value)
    }

    /// Stores every element of `values` contiguously.
    ///
    /// When the current chunk has room, the elements are moved into it;
    /// otherwise the vector's own buffer becomes the next chunk and nothing
    /// is copied at all.
    fn alloc_vec(&mut self, values: Vec<T>) -> Vec<*const T> {
        let n = values.len();
        if n == 0 {
            return Vec::new();
        }
        if self.chunks.last().is_some_and(|c| c.remaining() >= n) {
            return self.chunk_with_room(n).push_all(values);
        }
        let mut chunk = Chunk::adopt(values);
        let base = chunk.slot(0);
        self.chunks.push(chunk);
        // SAFETY: The adopted buffer holds `n` initialized elements.
        (0..n).map(|i| unsafe { base.add(i) } as *const T).collect()
    }
}

/// What a chain node keeps alive. The payloads are never read; they exist
/// so their `Drop` runs when the last referencing storage goes away.
#[allow(dead_code)]
enum Kept<T> {
    /// A frozen ancestor arena.
    Arena(Arc<Arena<T>>),
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
    active: Arc<Arena<T>>,
    frozen: Option<Arc<ChainNode<T>>>,
    /// Number of values this storage keeps alive. Exact for a single
    /// lineage; an upper bound after [`absorb`](Self::absorb), which may
    /// double-count shared ancestry.
    allocated: usize,
}

impl<T> Storage<T> {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(Arena::new()),
            frozen: None,
            allocated: 0,
        }
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            active: Arc::new(Arena::with_capacity(capacity)),
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

    /// Exclusive access to the active arena, freezing it first if it is
    /// shared.
    ///
    /// Sound because we hold `&mut self`: a strong count of 1 means the
    /// only `Arc` to this arena is our own field, and no other thread can
    /// clone an `Arc` it does not have.
    fn active_mut(&mut self) -> &mut Arena<T> {
        if Arc::strong_count(&self.active) > 1 {
            let old = mem::replace(&mut self.active, Arc::new(Arena::new()));
            self.frozen = Some(Arc::new(ChainNode {
                _kept: Kept::Arena(old),
                next: self.frozen.take(),
            }));
        }
        Arc::get_mut(&mut self.active).expect("active arena is uniquely owned")
    }

    /// Allocates a value and returns a pointer valid for this storage's
    /// lifetime (and the lifetime of every storage that later absorbs it).
    pub(crate) fn alloc(&mut self, value: T) -> *const T {
        self.allocated += 1;
        self.active_mut().alloc(value)
    }

    /// Allocates every value from the iterator, contiguously, and returns
    /// their pointers.
    pub(crate) fn alloc_extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> Vec<*const T> {
        self.alloc_vec(iter.into_iter().collect())
    }

    /// Stores every element of `values`, contiguously. When the active
    /// arena has no room, the vector's buffer is taken over as is, so no
    /// element is copied.
    pub(crate) fn alloc_vec(&mut self, values: Vec<T>) -> Vec<*const T> {
        self.allocated += values.len();
        self.active_mut().alloc_vec(values)
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
