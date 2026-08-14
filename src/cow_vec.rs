use std::fmt;
use std::ops::{Bound, Index, RangeBounds};
use std::sync::{Arc, Mutex};

use typed_arena::Arena;

use super::CowVecIter;

/// Shared arena that stores values allocated by `CowVec` instances.
///
/// The arena is append-only: values are never removed or moved once allocated.
/// This guarantees that pointers to arena items remain valid for the arena's lifetime.
struct CowArena<T> {
    arena: Mutex<Arena<T>>,
}

impl<T> CowArena<T> {
    fn new() -> Self {
        Self {
            arena: Mutex::new(Arena::new()),
        }
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            arena: Mutex::new(Arena::with_capacity(capacity)),
        }
    }

    /// Allocates a value in the arena and returns a raw pointer to it.
    ///
    /// # Safety
    /// The returned pointer is valid for the lifetime of the arena.
    /// Since the arena is append-only and wrapped in Arc, the pointer
    /// remains valid as long as any CowVec holds a reference to this arena.
    fn alloc(&self, value: T) -> *const T {
        let arena = self.arena.lock().unwrap();
        let reference = arena.alloc(value);
        // Cast through *mut so the pointer keeps write provenance; make_mut
        // writes through it after casting back. A direct `&mut T as *const T`
        // reborrows as shared and makes that write undefined behavior.
        reference as *mut T as *const T
    }

    /// Allocates every value from the iterator under a single lock and
    /// returns their pointers.
    ///
    /// The values are placed contiguously in the arena, so bulk-loaded
    /// elements are adjacent in memory.
    ///
    /// # Safety
    /// Same guarantees as [`alloc`](Self::alloc).
    fn alloc_extend<I: IntoIterator<Item = T>>(&self, iter: I) -> Vec<*const T> {
        let arena = self.arena.lock().unwrap();
        let slice = arena.alloc_extend(iter);
        slice.iter_mut().map(|r| r as *mut T as *const T).collect()
    }

    /// Returns the total number of allocations in this arena.
    fn len(&self) -> usize {
        self.arena.lock().unwrap().len()
    }
}

/// A vector-like container optimized for efficient cloning.
///
/// `CowVec` uses a shared arena (via `Arc`) for storing values. Each instance
/// maintains its own vector of pointers to items in the shared arena.
/// When cloned, only the pointer vector is cloned while the arena is shared.
///
/// # Copy-on-Write Semantics
/// The `set` method implements copy-on-write: it allocates a new value in the
/// arena and updates only this instance's pointer. Other clones continue to
/// see the original value.
///
/// # Thread Safety
/// `CowVec<T>` is `Send` and `Sync` when `T: Send + Sync`.
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
    arena: Arc<CowArena<T>>,
    items: Arc<Vec<*const T>>,
}

// SAFETY: CowVec is Send+Sync because:
// - Arc<CowArena<T>> is Send+Sync when T: Send+Sync (CowArena contains Mutex<Arena<T>>)
// - *const T pointers are valid as long as arena lives (guaranteed by Arc)
// - All mutation goes through Mutex
// - We only provide &T access, never &mut T
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
            arena: Arc::new(CowArena::new()),
            items: Arc::new(Vec::new()),
        }
    }

    /// Creates a new `CowVec` with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            arena: Arc::new(CowArena::with_capacity(capacity)),
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

    /// Returns `true` if the storage (arena with actual values) is shared with other clones.
    ///
    /// This typically returns `true` after any clone operation, as all clones share
    /// the same arena for value storage.
    pub fn is_storage_shared(&self) -> bool {
        Arc::strong_count(&self.arena) > 1
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
        let ptr = self.arena.alloc(value);
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

    /// Shortens the vector, keeping the first `len` elements.
    ///
    /// If `len` is greater than or equal to the current length, this has no effect.
    ///
    /// Note: Removed values remain in the shared arena.
    pub fn truncate(&mut self, len: usize) {
        self.items_mut().truncate(len);
    }

    /// Clears the vector, removing all elements.
    ///
    /// Note: Values remain in the shared arena but are no longer
    /// accessible through this `CowVec` instance.
    pub fn clear(&mut self) {
        self.items_mut().clear();
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
        let ptr = self.arena.alloc(value);
        self.items_mut().insert(index, ptr);
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// Removes all elements for which the predicate returns `false`.
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
        self.items_mut().retain(|ptr| {
            // SAFETY: Pointer is valid for arena's lifetime
            let value = unsafe { &**ptr };
            f(value)
        });
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
            arena: Arc::clone(&self.arena),
            items: Arc::new(tail_items),
        }
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

        // Allocate new elements in arena under a single lock
        let new_ptrs = self.arena.alloc_extend(replace_with);

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
        if self.arena.len() <= max_allocations {
            return self.clone();
        }

        // Create a fresh arena with just the current elements.
        let new_arena = Arc::new(CowArena::with_capacity(self.len()));
        let new_items: Vec<*const T> = self
            .iter()
            .map(|item| new_arena.alloc(item.clone()))
            .collect();

        Self {
            arena: new_arena,
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
        let ptr = self.arena.alloc(value);
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
        let ptr = self.arena.alloc(current);
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
            arena: Arc::clone(&self.arena),
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
    /// All elements are allocated under a single arena lock and stored
    /// contiguously.
    fn from(vec: Vec<T>) -> Self {
        let arena = Arc::new(CowArena::with_capacity(vec.len()));
        let items = arena.alloc_extend(vec);
        Self {
            arena,
            items: Arc::new(items),
        }
    }
}

impl<T> Extend<T> for CowVec<T> {
    /// Extends the vector with elements from an iterator.
    ///
    /// All elements are allocated under a single arena lock and stored
    /// contiguously.
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let ptrs = self.arena.alloc_extend(iter);
        self.items_mut().extend(ptrs);
    }
}

impl<T> FromIterator<T> for CowVec<T> {
    /// Creates a `CowVec` from an iterator.
    ///
    /// All elements are allocated under a single arena lock and stored
    /// contiguously.
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let arena = Arc::new(CowArena::new());
        let items = arena.alloc_extend(iter);
        Self {
            arena,
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
    /// equal in O(1) without touching any elements.
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
