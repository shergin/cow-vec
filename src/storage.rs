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
//! are never allocated into again; they are held to keep their memory alive
//! until the last owner goes away, at which point they are merged back into
//! the sole survivor's active arena.
//!
//! Values never move once allocated, but a storage that nothing else can
//! reach may move a value *out* (`try_take`) or drop it in place
//! (`release`). Each chunk tracks which slots that has happened to, and the
//! arena hands those slots out again to later allocations, so a sole owner
//! that keeps mutating does not grow its storage.

use std::collections::HashSet;
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
/// stays 0 and it never touches the slots; `len` says how many slots have
/// been written, and they are always a prefix.
struct Chunk<T> {
    buf: Vec<MaybeUninit<T>>,
    len: usize,
    /// One bit per slot, set once the value has been moved out or dropped
    /// in place. Allocated on first use, so a chunk that only ever grows
    /// pays nothing for it.
    dead: Option<Box<[u64]>>,
}

impl<T> Chunk<T> {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            len: 0,
            dead: None,
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
        Self {
            buf,
            len,
            dead: None,
        }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.capacity() - self.len
    }

    /// Address of the buffer, for ordering and locating chunks.
    #[inline]
    fn base(&self) -> usize {
        self.buf.as_ptr() as usize
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

    /// Index of the written slot `ptr` points at, if it is in this chunk.
    ///
    /// Zero-sized values all share one address, so they are never located;
    /// moving or dropping one early is not worth telling apart.
    fn slot_of(&self, ptr: *const T) -> Option<usize> {
        let size = mem::size_of::<T>();
        if size == 0 {
            return None;
        }
        let offset = (ptr as usize).checked_sub(self.base())?;
        let index = offset / size;
        (offset % size == 0 && index < self.len).then_some(index)
    }

    #[inline]
    fn is_dead(&self, index: usize) -> bool {
        self.dead
            .as_ref()
            .is_some_and(|dead| dead[index / 64] & (1 << (index % 64)) != 0)
    }

    fn mark_dead(&mut self, index: usize) {
        let words = self.capacity().div_ceil(64);
        let dead = self
            .dead
            .get_or_insert_with(|| vec![0u64; words].into_boxed_slice());
        dead[index / 64] |= 1 << (index % 64);
    }

    /// Writes `value` into a dead slot, making it live again.
    ///
    /// # Safety
    /// `index` must be a dead slot below `len`.
    unsafe fn revive(&mut self, index: usize, value: T) -> *const T {
        debug_assert!(index < self.len && self.is_dead(index));
        let dead = self.dead.as_mut().expect("a dead slot exists");
        dead[index / 64] &= !(1 << (index % 64));
        let ptr = self.slot(index);
        ptr::write(ptr, value);
        ptr
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

    /// Moves the value out of a live slot.
    ///
    /// # Safety
    /// `index` must be a live slot that nothing else references.
    unsafe fn take(&mut self, index: usize) -> T {
        debug_assert!(index < self.len && !self.is_dead(index));
        self.mark_dead(index);
        ptr::read(self.slot(index))
    }

    /// Drops the value in a live slot.
    ///
    /// # Safety
    /// `index` must be a live slot that nothing else references.
    unsafe fn drop_slot(&mut self, index: usize) {
        debug_assert!(index < self.len && !self.is_dead(index));
        self.mark_dead(index);
        ptr::drop_in_place(self.slot(index));
    }
}

impl<T> Drop for Chunk<T> {
    fn drop(&mut self) {
        let len = self.len;
        match self.dead.take() {
            // SAFETY: The first `len` slots are initialized, and the chunk
            // is dropping, so nothing references them any more.
            None => unsafe {
                ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.slot(0), len));
            },
            Some(dead) => {
                for index in 0..len {
                    if dead[index / 64] & (1 << (index % 64)) == 0 {
                        // SAFETY: As above, for a slot still holding a value.
                        unsafe { ptr::drop_in_place(self.slot(index)) };
                    }
                }
            }
        }
    }
}

/// A bump arena: a set of chunks, one of which is being filled.
///
/// Values never move once allocated, so raw pointers to them stay valid
/// until the arena drops.
struct Arena<T> {
    /// Sorted by buffer address, so a pointer can be located by bisection.
    chunks: Vec<Chunk<T>>,
    /// Index of the chunk taking new values; `usize::MAX` before the first.
    current: usize,
    /// Addresses of dead slots, reused by [`alloc`](Self::alloc) before it
    /// touches fresh capacity. Only ever non-empty in an arena its owner
    /// has moved values out of.
    free: Vec<usize>,
}

impl<T> Arena<T> {
    fn new() -> Self {
        Self {
            chunks: Vec::new(),
            current: usize::MAX,
            free: Vec::new(),
        }
    }

    fn with_capacity(capacity: usize) -> Self {
        let mut arena = Self::new();
        if capacity > 0 {
            arena.insert(Chunk::with_capacity(capacity));
        }
        arena
    }

    /// Adds a chunk in address order and makes it current.
    fn insert(&mut self, chunk: Chunk<T>) -> &mut Chunk<T> {
        let pos = self.chunks.partition_point(|c| c.base() < chunk.base());
        self.chunks.insert(pos, chunk);
        self.current = pos;
        &mut self.chunks[pos]
    }

    /// Returns the current chunk, opening a new one if it cannot take `n`
    /// more values.
    fn chunk_with_room(&mut self, n: usize) -> &mut Chunk<T> {
        let current = self.chunks.get(self.current);
        if current.is_some_and(|c| c.remaining() >= n) {
            return &mut self.chunks[self.current];
        }
        let last = current.map_or(0, Chunk::capacity);
        let min = INITIAL_CHUNK_BYTES / mem::size_of::<T>().max(1);
        let capacity = n.max(last.saturating_mul(2)).max(min).max(1);
        self.insert(Chunk::with_capacity(capacity))
    }

    /// Stores `value`, reusing a dead slot when one is available. Returns
    /// the pointer and whether a fresh slot was taken.
    fn alloc(&mut self, value: T) -> (*const T, bool) {
        if let Some(addr) = self.free.pop() {
            let (chunk, slot) = self
                .locate(addr as *const T)
                .expect("free list entries are slots of this arena");
            // SAFETY: Every free-list entry is a dead slot below `len`.
            return (unsafe { self.chunks[chunk].revive(slot, value) }, false);
        }
        (self.chunk_with_room(1).push(value), true)
    }

    /// Moves the value out of the slot at `ptr` and queues the slot for
    /// reuse.
    ///
    /// # Safety
    /// `ptr` must be a live slot of this arena that nothing else references.
    unsafe fn take(&mut self, chunk: usize, slot: usize) -> T {
        let value = self.chunks[chunk].take(slot);
        self.free.push(self.chunks[chunk].slot(slot) as usize);
        value
    }

    /// Drops the value in the slot in place and queues the slot for reuse.
    ///
    /// # Safety
    /// As for [`take`](Self::take).
    unsafe fn drop_slot(&mut self, chunk: usize, slot: usize) {
        self.chunks[chunk].drop_slot(slot);
        self.free.push(self.chunks[chunk].slot(slot) as usize);
    }

    /// Stores every element of `values` contiguously.
    ///
    /// When the current chunk has room, the elements are moved into it;
    /// otherwise the vector's own buffer becomes the next chunk and nothing
    /// is copied at all. Zero-sized types always share one chunk.
    fn alloc_vec(&mut self, values: Vec<T>) -> Vec<*const T> {
        let n = values.len();
        if n == 0 {
            return Vec::new();
        }
        let has_room = self
            .chunks
            .get(self.current)
            .is_some_and(|c| c.remaining() >= n);
        if has_room || mem::size_of::<T>() == 0 {
            return self.chunk_with_room(n).push_all(values);
        }
        let chunk = self.insert(Chunk::adopt(values));
        let base = chunk.slot(0);
        // SAFETY: The adopted buffer holds `n` initialized elements.
        (0..n).map(|i| unsafe { base.add(i) } as *const T).collect()
    }

    /// Finds the chunk and slot holding `ptr`.
    fn locate(&self, ptr: *const T) -> Option<(usize, usize)> {
        let addr = ptr as usize;
        let i = self
            .chunks
            .partition_point(|c| c.base() <= addr)
            .checked_sub(1)?;
        self.chunks[i].slot_of(ptr).map(|slot| (i, slot))
    }

    /// Adds every chunk (and free slot) of `other`, keeping address order
    /// and the current chunk.
    fn merge(&mut self, other: Arena<T>) {
        let current_base = self.chunks.get(self.current).map(Chunk::base);
        self.chunks.extend(other.chunks);
        self.free.extend(other.free);
        self.chunks.sort_unstable_by_key(Chunk::base);
        self.current = current_base.map_or(usize::MAX, |base| {
            self.chunks.partition_point(|c| c.base() < base)
        });
    }
}

/// What a chain node keeps alive.
enum Kept<T> {
    /// A frozen ancestor arena.
    Arena(Arc<Arena<T>>),
    /// A whole foreign chain, retained by [`Storage::absorb`].
    Chain(Arc<ChainNode<T>>),
}

/// Keep-alive node for storage that earlier generations allocated into.
struct ChainNode<T> {
    /// `None` only transiently, while [`Storage::reclaim`] dismantles the
    /// node.
    kept: Option<Kept<T>>,
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
    /// Number of value slots this storage holds, live or not. A slot
    /// reused after a move-out or in-place drop is not counted again.
    /// Exact for a single lineage; an upper bound after
    /// [`absorb`](Self::absorb), which may double-count shared ancestry.
    allocated: usize,
    /// Set by [`absorb`](Self::absorb) when the other storage shared an
    /// arena with this one: a single pointer table may then hold the same
    /// value twice, so nothing may be moved out or dropped in place until
    /// the storage is rebuilt.
    aliased: bool,
}

impl<T> Storage<T> {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(Arena::new()),
            frozen: None,
            allocated: 0,
            aliased: false,
        }
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            active: Arc::new(Arena::with_capacity(capacity)),
            ..Self::new()
        }
    }

    /// Returns `true` if any other instance may still reference values in
    /// this storage.
    ///
    /// Conservative between mutations: a frozen chain whose other owners
    /// have all dropped is only merged back on the next mutation. A storage
    /// that has absorbed a related one reports `true` until rebuilt.
    pub(crate) fn is_shared(&self) -> bool {
        self.aliased || Arc::strong_count(&self.active) > 1 || self.frozen.is_some()
    }

    /// Number of value slots this storage holds, including ones whose value
    /// is unreachable or already gone. Upper bound after `absorb`.
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
                kept: Some(Kept::Arena(old)),
                next: self.frozen.take(),
            }));
        } else {
            self.reclaim();
        }
        Arc::get_mut(&mut self.active).expect("active arena is uniquely owned")
    }

    /// Merges frozen ancestors that nothing else references back into the
    /// active arena, walking the chain from the head until it meets a node
    /// or arena some other storage still holds.
    ///
    /// Amortized O(1): every node is dismantled at most once, and a shared
    /// head is rejected after two atomic loads.
    fn reclaim(&mut self) {
        if self.frozen.is_none() || Arc::strong_count(&self.active) > 1 {
            return;
        }
        // Foreign chains spliced in by `absorb` nest inside a node. While
        // one is being walked, the rest of the enclosing chain waits here.
        let mut pending: Vec<Option<Arc<ChainNode<T>>>> = Vec::new();
        loop {
            let head = match &self.frozen {
                Some(head) => head,
                None => match pending.pop() {
                    Some(rest) => {
                        self.frozen = rest;
                        continue;
                    }
                    None => break,
                },
            };
            let claimable = Arc::strong_count(head) == 1
                && match &head.kept {
                    Some(Kept::Arena(arena)) => Arc::strong_count(arena) == 1,
                    Some(Kept::Chain(chain)) => Arc::strong_count(chain) == 1,
                    None => true,
                };
            if !claimable {
                break;
            }
            let head = self.frozen.take().expect("checked above");
            let mut head = Arc::try_unwrap(head)
                .ok()
                .expect("strong count was 1 under &mut self");
            self.frozen = head.next.take();
            match head.kept.take() {
                Some(Kept::Arena(arena)) => {
                    let arena = Arc::try_unwrap(arena)
                        .ok()
                        .expect("strong count was 1 under &mut self");
                    Arc::get_mut(&mut self.active)
                        .expect("strong count is 1")
                        .merge(arena);
                }
                Some(Kept::Chain(chain)) => {
                    pending.push(self.frozen.take());
                    self.frozen = Some(chain);
                }
                None => {}
            }
        }
        // Stopped inside a foreign chain: put what remains of the enclosing
        // chains back behind it.
        let mut chain = self.frozen.take();
        for rest in pending.into_iter().rev() {
            chain = match (chain, rest) {
                (chain, None) => chain,
                (None, rest) => rest,
                (Some(inner), rest) => Some(Arc::new(ChainNode {
                    kept: Some(Kept::Chain(inner)),
                    next: rest,
                })),
            };
        }
        self.frozen = chain;
    }

    /// Returns `true` if values in the active arena may be moved out or
    /// dropped in place: no other storage can reach them.
    pub(crate) fn owns_active(&mut self) -> bool {
        if self.aliased || Arc::strong_count(&self.active) > 1 {
            return false;
        }
        self.reclaim();
        true
    }

    /// Returns `true` if every value this storage holds is reachable from
    /// nothing else.
    pub(crate) fn owns_all(&mut self) -> bool {
        self.owns_active() && self.frozen.is_none()
    }

    /// Allocates a value and returns a pointer valid for this storage's
    /// lifetime (and the lifetime of every storage that later absorbs it).
    ///
    /// A slot freed earlier by [`try_take`](Self::try_take) or
    /// [`release`](Self::release) is reused before fresh capacity is.
    pub(crate) fn alloc(&mut self, value: T) -> *const T {
        let (ptr, fresh) = self.active_mut().alloc(value);
        if fresh {
            self.allocated += 1;
        }
        ptr
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

    /// Moves the value at `ptr` out if this storage exclusively owns it.
    /// Otherwise the value stays where it is and `None` is returned.
    pub(crate) fn try_take(&mut self, ptr: *const T) -> Option<T> {
        if !self.owns_active() {
            return None;
        }
        let active = Arc::get_mut(&mut self.active)?;
        let (chunk, slot) = active.locate(ptr)?;
        // SAFETY: The arena is ours alone and nothing else holds a pointer
        // into it (see owns_active), and the caller has removed `ptr` from
        // its own table.
        Some(unsafe { active.take(chunk, slot) })
    }

    /// Returns the value at `ptr` mutably if this storage exclusively owns
    /// it.
    pub(crate) fn get_mut(&mut self, ptr: *const T) -> Option<&mut T> {
        if !self.owns_active() {
            return None;
        }
        let active = Arc::get_mut(&mut self.active)?;
        let (chunk, slot) = active.locate(ptr)?;
        let chunk = &mut active.chunks[chunk];
        debug_assert!(!chunk.is_dead(slot));
        // SAFETY: A live slot in an arena nothing else can reach; the slot
        // pointer carries write provenance, and the borrow is tied to
        // `&mut self`.
        Some(unsafe { &mut *chunk.slot(slot) })
    }

    /// Drops the values at `ptrs` in place if this storage exclusively owns
    /// them. Otherwise they stay until the arena drops.
    pub(crate) fn release<I: IntoIterator<Item = *const T>>(&mut self, ptrs: I) {
        if !self.owns_active() {
            return;
        }
        let Some(active) = Arc::get_mut(&mut self.active) else {
            return;
        };
        for ptr in ptrs {
            if let Some((chunk, slot)) = active.locate(ptr) {
                // SAFETY: As in try_take.
                unsafe { active.drop_slot(chunk, slot) };
            }
        }
    }

    /// Moves every value at `ptrs` out, in order, if this storage
    /// exclusively owns all of its values. Otherwise `None`.
    pub(crate) fn take_all(&mut self, ptrs: &[*const T]) -> Option<Vec<T>> {
        if !self.owns_all() {
            return None;
        }
        let active = Arc::get_mut(&mut self.active)?;
        // Every live pointer of a fully owned storage sits in its active
        // arena; check before moving anything so a broken invariant cannot
        // leave the table half dangling.
        let located: Vec<(usize, usize)> = ptrs
            .iter()
            .map(|&ptr| active.locate(ptr))
            .collect::<Option<_>>()?;
        Some(
            located
                .into_iter()
                // SAFETY: As in try_take, for each pointer once.
                .map(|(chunk, slot)| unsafe { active.chunks[chunk].take(slot) })
                .collect(),
        )
    }

    /// Keeps every arena of `other` alive from this storage too, so raw
    /// pointers copied out of `other` stay valid for our lifetime.
    ///
    /// Used by `append`: concatenation copies pointers, never elements.
    pub(crate) fn absorb(&mut self, other: &Storage<T>) {
        if self.shares_arena_with(other) {
            self.aliased = true;
        }
        if !Arc::ptr_eq(&self.active, &other.active) {
            self.frozen = Some(Arc::new(ChainNode {
                kept: Some(Kept::Arena(Arc::clone(&other.active))),
                next: self.frozen.take(),
            }));
        }
        if let Some(chain) = &other.frozen {
            let same = self.frozen.as_ref().is_some_and(|f| Arc::ptr_eq(f, chain));
            if !same {
                self.frozen = Some(Arc::new(ChainNode {
                    kept: Some(Kept::Chain(Arc::clone(chain))),
                    next: self.frozen.take(),
                }));
            }
        }
        self.allocated += other.allocated;
    }

    /// Returns `true` if `self` and `other` keep any arena in common, in
    /// which case pointers copied from `other` may duplicate ours.
    fn shares_arena_with(&self, other: &Storage<T>) -> bool {
        let mut ours = HashSet::new();
        self.for_each_arena(|arena| {
            ours.insert(arena as usize);
        });
        let mut shared = false;
        other.for_each_arena(|arena| shared |= ours.contains(&(arena as usize)));
        shared
    }

    /// Visits the address of every arena this storage keeps alive.
    fn for_each_arena(&self, mut visit: impl FnMut(*const Arena<T>)) {
        visit(Arc::as_ptr(&self.active));
        let mut stack: Vec<&ChainNode<T>> = self.frozen.as_deref().into_iter().collect();
        while let Some(node) = stack.pop() {
            match &node.kept {
                Some(Kept::Arena(arena)) => visit(Arc::as_ptr(arena)),
                Some(Kept::Chain(chain)) => stack.push(chain),
                None => {}
            }
            if let Some(next) = &node.next {
                stack.push(next);
            }
        }
    }
}

impl<T> Clone for Storage<T> {
    /// O(1): shares the active arena and the frozen chain.
    fn clone(&self) -> Self {
        Self {
            active: Arc::clone(&self.active),
            frozen: self.frozen.clone(),
            allocated: self.allocated,
            aliased: self.aliased,
        }
    }
}
