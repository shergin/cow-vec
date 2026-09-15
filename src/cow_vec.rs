use std::fmt;
use std::ops::{Bound, Index, RangeBounds};
use std::sync::Arc;

use super::CowVecIter;
use crate::storage::Storage;

/// A vector-like container optimized for efficient cloning.
///
/// `CowVec` stores values in append-only bump arenas shared between clones
/// (kept alive via `Arc`). Each instance maintains a vector of pointers to
/// those values; cloning shares both the storage and the pointer vector, so
/// `clone()` is O(1).
///
/// # Copy-on-Write Semantics
/// The `set` method implements copy-on-write: it allocates a new value in
/// this instance's storage and updates only this instance's pointer. Other
/// clones continue to see the original value.
///
/// # Thread Safety
/// `CowVec<T>` is `Send` and `Sync` when `T: Send + Sync`. Allocation
/// takes no lock: each instance only ever allocates into an arena it
/// uniquely owns, so clones on different threads never contend.
///
/// # Example
/// ```
/// use cow_vec::CowVec;
///
/// let vec1 = CowVec::from(vec![1, 2, 3]);
/// let mut vec2 = vec1.clone(); // Cheap clone - shares the arena
/// vec2.set(0, 10); // Only vec2 sees the change
/// assert_eq!(vec1[0], 1);
/// assert_eq!(vec2[0], 10);
/// ```
pub struct CowVec<T> {
    storage: Storage<T>,
    items: Arc<Vec<*const T>>,
}

// SAFETY: CowVec is Send+Sync when T: Send+Sync because:
// - Storage<T> allocates without a lock but only ever through the instance
//   that uniquely owns the active arena, under &mut self (see storage.rs).
// - The *const T items point into arenas that the storage keeps alive, and
//   only &T is ever exposed through them (T: Sync); the last owner may drop
//   the values on any thread (T: Send).
// The manual impls exist because *const T suppresses the automatic ones.
unsafe impl<T: Send + Sync> Send for CowVec<T> {}
unsafe impl<T: Send + Sync> Sync for CowVec<T> {}

impl<T> CowVec<T> {
    /// Returns a mutable reference to the items vector.
    ///
    /// If the items Arc is shared with other CowVec instances, this will
    /// clone the vector first (copy-on-write semantics).
    #[inline]
    fn items_mut(&mut self) -> &mut Vec<*const T> {
        Arc::make_mut(&mut self.items)
    }

    /// Creates a new empty `CowVec`.
    pub fn new() -> Self {
        Self {
            storage: Storage::new(),
            items: Arc::new(Vec::new()),
        }
    }

    /// Creates a new `CowVec` with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: Storage::with_capacity(capacity),
            items: Arc::new(Vec::with_capacity(capacity)),
        }
    }

    /// Returns the number of elements in this vector.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if this vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns `true` if the structure (element pointers/order) is shared with other clones.
    ///
    /// When this returns `true`, the next mutation will trigger a copy of the
    /// internal pointer vector (copy-on-write semantics).
    pub fn is_structure_shared(&self) -> bool {
        Arc::strong_count(&self.items) > 1
    }

    /// Returns `true` if the storage (arenas with actual values) may be
    /// shared with other clones.
    ///
    /// This typically returns `true` after any clone operation. It is
    /// conservative: storage that retains frozen ancestor arenas reports
    /// `true` even if every other owner has already been dropped.
    pub fn is_storage_shared(&self) -> bool {
        self.storage.is_shared()
    }

    /// Returns the number of values this vector's storage keeps alive,
    /// including values no longer reachable (replaced by `set`, dropped by
    /// `pop`, and so on).
    ///
    /// Useful for deciding when to reclaim memory with
    /// [`clone_compacted`](Self::clone_compacted). After
    /// [`append`](Self::append) this is an upper bound, since the two
    /// vectors may have kept overlapping history alive.
    pub fn storage_allocations(&self) -> usize {
        self.storage.allocated()
    }

    /// Returns the elements as a slice of references.
    ///
    /// This provides efficient access to all elements without iteration,
    /// useful when you need to pass the data to APIs expecting `&[&T]`.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let vec = CowVec::from(vec![1, 2, 3]);
    /// let slice: &[&i32] = vec.as_slice();
    /// assert_eq!(slice.len(), 3);
    /// assert_eq!(*slice[0], 1);
    /// ```
    pub fn as_slice(&self) -> &[&T] {
        // SAFETY: This transmute is sound because:
        // 1. `*const T` and `&T` have identical memory layouts (both are pointers)
        // 2. All pointers in `self.items` are valid for the arena's lifetime
        // 3. The arena outlives this `CowVec` (guaranteed by Arc)
        // 4. The returned slice borrows `&self`, so it cannot outlive the CowVec
        // 5. The arena is append-only, so pointers are never invalidated
        unsafe { std::mem::transmute(self.items.as_slice()) }
    }

    /// Returns a reference to the element at the given index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index).map(|ptr| {
            // SAFETY: The pointer is valid because:
            // 1. It was obtained from arena.alloc()
            // 2. The arena never moves or deallocates items
            // 3. The arena lives as long as this CowVec (via Arc)
            unsafe { &**ptr }
        })
    }

    /// Appends an element to the back of this vector.
    ///
    /// The element is stored in the shared arena, and this instance's
    /// pointer list is updated to include it.
    pub fn push(&mut self, value: T) {
        let ptr = self.storage.alloc(value);
        self.items_mut().push(ptr);
    }

    /// Returns the internal pointer slice for iterator construction.
    #[inline]
    pub(crate) fn items_slice(&self) -> &[*const T] {
        &self.items
    }

    /// Returns an iterator over references to the elements.
    pub fn iter(&self) -> CowVecIter<'_, T> {
        CowVecIter::new(self)
    }

    /// Returns a reference to the first element, or `None` if empty.
    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }

    /// Returns a reference to the last element, or `None` if empty.
    pub fn last(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            self.get(self.len() - 1)
        }
    }

    /// Removes the last element and returns it, or `None` if empty.
    ///
    /// The returned value is cloned out of the shared arena, where the
    /// original stays allocated (other clones may still reference it).
    /// To drop elements without `T: Clone`, use [`truncate`](Self::truncate).
    pub fn pop(&mut self) -> Option<T>
    where
        T: Clone,
    {
        self.items_mut().pop().map(|ptr| {
            // SAFETY: Same as get() - pointer is valid for arena's lifetime
            unsafe { &*ptr }.clone()
        })
    }

    /// Removes and returns the element at the given index.
    ///
    /// All elements after the index are shifted left.
    ///
    /// The returned value is cloned out of the shared arena, where the
    /// original stays allocated (other clones may still reference it).
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn remove(&mut self, index: usize) -> T
    where
        T: Clone,
    {
        let ptr = self.items_mut().remove(index);
        // SAFETY: Same as get() - pointer is valid for arena's lifetime
        unsafe { &*ptr }.clone()
    }

    /// Swaps two elements in the vector.
    ///
    /// # Panics
    /// Panics if either index is out of bounds.
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items_mut().swap(a, b);
    }

    /// Reverses the order of elements in the vector.
    pub fn reverse(&mut self) {
        self.items_mut().reverse();
    }

    /// Sorts the vector with a comparator function (stable).
    ///
    /// Only the internal pointers are permuted; elements are never moved or
    /// cloned, which makes sorting cheap even for large element types.
    pub fn sort_by<F>(&mut self, mut compare: F)
    where
        F: FnMut(&T, &T) -> std::cmp::Ordering,
    {
        // SAFETY: Pointers are valid for the arena's lifetime (see get()).
        self.items_mut()
            .sort_by(|a, b| compare(unsafe { &**a }, unsafe { &**b }));
    }

    /// Sorts the vector (stable). See [`sort_by`](Self::sort_by).
    pub fn sort(&mut self)
    where
        T: Ord,
    {
        self.sort_by(T::cmp);
    }

    /// Sorts the vector with a key extraction function (stable).
    /// See [`sort_by`](Self::sort_by).
    pub fn sort_by_key<K, F>(&mut self, mut key: F)
    where
        K: Ord,
        F: FnMut(&T) -> K,
    {
        // SAFETY: Pointers are valid for the arena's lifetime (see get()).
        self.items_mut().sort_by_key(|ptr| key(unsafe { &**ptr }));
    }

    /// Sorts the vector with a comparator function (unstable, typically
    /// faster). Only pointers are permuted; see [`sort_by`](Self::sort_by).
    pub fn sort_unstable_by<F>(&mut self, mut compare: F)
    where
        F: FnMut(&T, &T) -> std::cmp::Ordering,
    {
        // SAFETY: Pointers are valid for the arena's lifetime (see get()).
        self.items_mut()
            .sort_unstable_by(|a, b| compare(unsafe { &**a }, unsafe { &**b }));
    }

    /// Sorts the vector (unstable, typically faster).
    /// See [`sort_by`](Self::sort_by).
    pub fn sort_unstable(&mut self)
    where
        T: Ord,
    {
        self.sort_unstable_by(T::cmp);
    }

    /// Rotates the vector so that the element at `mid` becomes the first.
    ///
    /// # Panics
    /// Panics if `mid > len()`.
    pub fn rotate_left(&mut self, mid: usize) {
        self.items_mut().rotate_left(mid);
    }

    /// Rotates the vector so that the element at `len() - k` becomes the
    /// first.
    ///
    /// # Panics
    /// Panics if `k > len()`.
    pub fn rotate_right(&mut self, k: usize) {
        self.items_mut().rotate_right(k);
    }

    /// Removes consecutive elements for which `same_bucket` returns `true`,
    /// keeping the first of each run.
    ///
    /// Note: Removed values remain in the shared arena.
    pub fn dedup_by<F>(&mut self, mut same_bucket: F)
    where
        F: FnMut(&T, &T) -> bool,
    {
        // SAFETY: Pointers are valid for the arena's lifetime (see get()).
        self.items_mut()
            .dedup_by(|a, b| same_bucket(unsafe { &**a }, unsafe { &**b }));
    }

    /// Removes consecutive equal elements. See [`dedup_by`](Self::dedup_by).
    pub fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup_by(T::eq);
    }

    /// Binary searches this vector with a comparator function.
    ///
    /// The vector must be sorted consistently with the comparator.
    pub fn binary_search_by<F>(&self, mut f: F) -> Result<usize, usize>
    where
        F: FnMut(&T) -> std::cmp::Ordering,
    {
        // SAFETY: Pointers are valid for the arena's lifetime (see get()).
        self.items.binary_search_by(|ptr| f(unsafe { &**ptr }))
    }

    /// Binary searches this sorted vector for the given value.
    pub fn binary_search(&self, value: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        self.binary_search_by(|item| item.cmp(value))
    }

    /// Shortens the vector, keeping the first `len` elements.
    ///
    /// If `len` is greater than or equal to the current length, this has no
    /// effect. When the pointer table is shared with clones, only the kept
    /// prefix is copied, not the whole table.
    ///
    /// Note: Removed values remain in the shared arena.
    pub fn truncate(&mut self, len: usize) {
        if len >= self.items.len() {
            return;
        }
        match Arc::get_mut(&mut self.items) {
            Some(items) => items.truncate(len),
            None => self.items = Arc::new(self.items[..len].to_vec()),
        }
    }

    /// Clears the vector, removing all elements.
    ///
    /// Copies nothing, even when the pointer table is shared with clones.
    ///
    /// Note: Values remain in the shared arena but are no longer
    /// accessible through this `CowVec` instance.
    pub fn clear(&mut self) {
        match Arc::get_mut(&mut self.items) {
            Some(items) => items.clear(),
            None => self.items = Arc::new(Vec::new()),
        }
    }

    /// Inserts an element at position `index`, shifting all elements after it to the right.
    ///
    /// # Panics
    /// Panics if `index > len()`.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let mut vec = CowVec::from(vec![1, 2, 3]);
    /// vec.insert(1, 10);
    /// assert_eq!(vec.to_vec(), vec![1, 10, 2, 3]);
    /// ```
    pub fn insert(&mut self, index: usize, value: T) {
        let ptr = self.storage.alloc(value);
        self.items_mut().insert(index, ptr);
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// Removes all elements for which the predicate returns `false`. When
    /// the pointer table is shared with clones, only the kept pointers are
    /// copied.
    ///
    /// Note: Removed values remain in the shared arena.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    /// vec.retain(|&x| x % 2 == 0);
    /// assert_eq!(vec.to_vec(), vec![2, 4]);
    /// ```
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        // SAFETY (both arms): pointers are valid for the arena's lifetime.
        match Arc::get_mut(&mut self.items) {
            Some(items) => items.retain(|ptr| f(unsafe { &**ptr })),
            None => {
                let kept: Vec<*const T> = self
                    .items
                    .iter()
                    .copied()
                    .filter(|ptr| f(unsafe { &**ptr }))
                    .collect();
                self.items = Arc::new(kept);
            }
        }
    }

    /// Splits the vector into two at the given index.
    ///
    /// Returns a new `CowVec` containing elements from `at` to the end.
    /// After this call, `self` contains elements `[0, at)` and the returned
    /// `CowVec` contains elements `[at, len)`.
    ///
    /// Both vectors share the same arena, so this is an efficient operation.
    ///
    /// # Panics
    /// Panics if `at > len()`.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    /// let tail = vec.split_off(3);
    /// assert_eq!(vec.to_vec(), vec![1, 2, 3]);
    /// assert_eq!(tail.to_vec(), vec![4, 5]);
    /// ```
    pub fn split_off(&mut self, at: usize) -> Self {
        let tail_items = self.items_mut().split_off(at);
        Self {
            storage: self.storage.clone(),
            items: Arc::new(tail_items),
        }
    }

    /// Appends all elements of `other` to this vector by copying pointers.
    ///
    /// No element is moved or cloned: this vector's storage additionally
    /// keeps `other`'s arenas alive, and only the 8-byte pointers are
    /// copied. `other` is unaffected.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let mut a = CowVec::from(vec![1, 2]);
    /// let b = CowVec::from(vec![3, 4]);
    /// a.append(&b);
    /// assert_eq!(a, vec![1, 2, 3, 4]);
    /// assert_eq!(b, vec![3, 4]);
    /// ```
    pub fn append(&mut self, other: &CowVec<T>) {
        self.storage.absorb(&other.storage);
        let other_items: &[*const T] = &other.items;
        self.items_mut().extend_from_slice(other_items);
    }

    /// Removes the specified range and replaces it with elements from the iterator.
    ///
    /// Returns the removed elements, cloned out of the shared arena.
    ///
    /// # Panics
    /// Panics if the range is out of bounds.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    /// let removed: Vec<i32> = vec.splice(1..3, vec![10, 20, 30]);
    /// assert_eq!(removed, vec![2, 3]);
    /// assert_eq!(vec.to_vec(), vec![1, 10, 20, 30, 4, 5]);
    /// ```
    pub fn splice<R, I>(&mut self, range: R, replace_with: I) -> Vec<T>
    where
        T: Clone,
        R: RangeBounds<usize>,
        I: IntoIterator<Item = T>,
    {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len(),
        };

        // Allocate new elements in one batch.
        let new_ptrs = self.storage.alloc_extend(replace_with);

        // Splice the pointer vector and clone out the removed values
        self.items_mut()
            .splice(start..end, new_ptrs)
            .map(|ptr| {
                // SAFETY: Pointer is valid for arena's lifetime
                unsafe { &*ptr }.clone()
            })
            .collect()
    }
}

impl<T: PartialEq> CowVec<T> {
    /// Returns `true` if the vector contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        self.iter().any(|item| item == value)
    }
}

impl<T: Clone> CowVec<T> {
    /// Converts this `CowVec` into a `Vec` by cloning all elements.
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    /// Clones this `CowVec`, compacting into a fresh arena if the current one
    /// holds more than `max_allocations` values.
    ///
    /// If the arena's total allocation count exceeds `max_allocations`, the
    /// clone gets a new arena containing only the currently visible elements.
    /// Otherwise this behaves exactly like `clone()`.
    ///
    /// This is the way to reclaim memory from garbage accumulated by `set`,
    /// `pop`, `remove`, and similar operations, whose old values stay in the
    /// shared arena.
    pub fn clone_compacted(&self, max_allocations: usize) -> Self {
        if self.storage.allocated() <= max_allocations {
            return self.clone();
        }

        // Create fresh storage holding just the current elements,
        // contiguously.
        let mut storage = Storage::new();
        let new_items = storage.alloc_extend(self.iter().cloned());

        Self {
            storage,
            items: Arc::new(new_items),
        }
    }
}

impl<T> CowVec<T> {
    /// Sets the value at the given index.
    ///
    /// This implements copy-on-write semantics: a new entry is allocated in the
    /// arena with the given value, and only this instance's pointer is updated.
    /// Other clones of this `CowVec` continue to see the original value.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn set(&mut self, index: usize, value: T) {
        if index >= self.items.len() {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len(),
                index
            );
        }
        let ptr = self.storage.alloc(value);
        self.items_mut()[index] = ptr;
    }

    /// Returns a mutable reference to the element at `index`, first copying
    /// its value to a fresh arena slot (copy-on-write).
    ///
    /// Like [`Arc::make_mut`], the name makes the cost explicit: every call
    /// clones the current value into a new arena allocation, even if nothing
    /// is written through the returned reference. Other clones of this
    /// `CowVec` keep seeing the original value.
    ///
    /// For replacing a value wholesale, prefer [`set`](Self::set), which
    /// moves the new value in without cloning the old one.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    ///
    /// # Example
    /// ```
    /// use cow_vec::CowVec;
    ///
    /// let vec1 = CowVec::from(vec![String::from("hello")]);
    /// let mut vec2 = vec1.clone();
    /// vec2.make_mut(0).push_str(" world");
    /// assert_eq!(vec1[0], "hello");
    /// assert_eq!(vec2[0], "hello world");
    /// ```
    pub fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone,
    {
        if index >= self.items.len() {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len(),
                index
            );
        }
        // Clone the current value to a new arena location (copy-on-write).
        // SAFETY: Same as get() - pointer is valid for arena's lifetime
        let current = unsafe { &*self.items[index] }.clone();
        let ptr = self.storage.alloc(current);
        self.items_mut()[index] = ptr;
        // SAFETY: The pointer was just allocated with write provenance and no
        // other CowVec references it. We have exclusive access via &mut self,
        // and the returned borrow keeps &mut self alive.
        unsafe { &mut *(ptr as *mut T) }
    }
}

impl<T> Default for CowVec<T> {
    /// Creates an empty `CowVec`.
    ///
    /// Equivalent to [`CowVec::new()`].
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for CowVec<T> {
    /// Clones this `CowVec` in O(1) time.
    ///
    /// Both the arena and the items vector are shared via `Arc`. The items
    /// vector will be automatically cloned on the first mutation to either
    /// copy (copy-on-write semantics).
    ///
    /// This makes cloning extremely cheap, with the cost of copying the items
    /// vector deferred until (and only if) a mutation occurs.
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            items: Arc::clone(&self.items),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for CowVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T> From<Vec<T>> for CowVec<T> {
    /// Creates a `CowVec` from a `Vec`.
    ///
    /// The vector's buffer becomes the storage as is: no element is moved
    /// or copied.
    fn from(vec: Vec<T>) -> Self {
        let mut storage = Storage::new();
        let items = storage.alloc_vec(vec);
        Self {
            storage,
            items: Arc::new(items),
        }
    }
}

impl<T> Extend<T> for CowVec<T> {
    /// Extends the vector with elements from an iterator.
    ///
    /// All elements are allocated in one batch and stored contiguously.
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let ptrs = self.storage.alloc_extend(iter);
        self.items_mut().extend(ptrs);
    }
}

impl<T> FromIterator<T> for CowVec<T> {
    /// Creates a `CowVec` from an iterator.
    ///
    /// All elements are allocated in one batch and stored contiguously.
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut storage = Storage::new();
        let items = storage.alloc_extend(iter);
        Self {
            storage,
            items: Arc::new(items),
        }
    }
}

impl<T: Clone> From<&[T]> for CowVec<T> {
    /// Creates a `CowVec` by cloning the elements of a slice.
    fn from(slice: &[T]) -> Self {
        slice.iter().cloned().collect()
    }
}

impl<T: PartialEq> PartialEq for CowVec<T> {
    /// Compares two vectors element by element.
    ///
    /// Vectors that share their structure (e.g. un-diverged clones) compare
    /// equal in O(1) without touching any elements. That shortcut also
    /// applies when `T`'s equality is not reflexive: a vector containing
    /// `f64::NAN` equals its own clone, where `Vec` would say otherwise.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.items, &other.items)
            || (self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b))
    }
}

impl<T: Eq> Eq for CowVec<T> {}

impl<T: PartialEq> PartialEq<[T]> for CowVec<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for CowVec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self == other.as_slice()
    }
}

impl<T: PartialOrd> PartialOrd for CowVec<T> {
    /// Lexicographic comparison, like `Vec`.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T: Ord> Ord for CowVec<T> {
    /// Lexicographic comparison, like `Vec`.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<T: std::hash::Hash> std::hash::Hash for CowVec<T> {
    /// Hashes the length followed by each element, so equal vectors hash
    /// identically regardless of how their storage is shared.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for item in self {
            item.hash(state);
        }
    }
}

impl<T> Index<usize> for CowVec<T> {
    type Output = T;

    /// Returns a reference to the element at the given index.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index out of bounds")
    }
}
