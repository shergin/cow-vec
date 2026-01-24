use std::fmt;
use std::ops::{Index, IndexMut};
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
        reference as *const T
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
    items: Vec<*const T>,
}

// SAFETY: CowVec is Send+Sync because:
// - Arc<CowArena<T>> is Send+Sync when T: Send+Sync (CowArena contains Mutex<Arena<T>>)
// - *const T pointers are valid as long as arena lives (guaranteed by Arc)
// - All mutation goes through Mutex
// - We only provide &T access, never &mut T
unsafe impl<T: Send + Sync> Send for CowVec<T> {}
unsafe impl<T: Send + Sync> Sync for CowVec<T> {}

impl<T> CowVec<T> {
    /// Creates a new empty `CowVec`.
    pub fn new() -> Self {
        Self {
            arena: Arc::new(CowArena::new()),
            items: Vec::new(),
        }
    }

    /// Creates a new `CowVec` with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            arena: Arc::new(CowArena::with_capacity(capacity)),
            items: Vec::with_capacity(capacity),
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
        self.items.push(ptr);
    }

    /// Returns an iterator over references to the elements.
    pub fn iter(&self) -> CowVecIter<'_, T> {
        CowVecIter {
            vec: self,
            position: 0,
        }
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
    /// Note: The value remains in the shared arena but is no longer
    /// accessible through this `CowVec` instance.
    pub fn pop(&mut self) -> Option<&T> {
        self.items.pop().map(|ptr| {
            // SAFETY: Same as get() - pointer is valid for arena's lifetime
            unsafe { &*ptr }
        })
    }

    /// Removes and returns the element at the given index.
    ///
    /// All elements after the index are shifted left.
    ///
    /// Note: The value remains in the shared arena but is no longer
    /// accessible through this `CowVec` instance.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn remove(&mut self, index: usize) -> &T {
        let ptr = self.items.remove(index);
        // SAFETY: Same as get() - pointer is valid for arena's lifetime
        unsafe { &*ptr }
    }

    /// Swaps two elements in the vector.
    ///
    /// # Panics
    /// Panics if either index is out of bounds.
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }

    /// Reverses the order of elements in the vector.
    pub fn reverse(&mut self) {
        self.items.reverse();
    }

    /// Shortens the vector, keeping the first `len` elements.
    ///
    /// If `len` is greater than or equal to the current length, this has no effect.
    ///
    /// Note: Removed values remain in the shared arena.
    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    /// Clears the vector, removing all elements.
    ///
    /// Note: Values remain in the shared arena but are no longer
    /// accessible through this `CowVec` instance.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Extends the vector with elements from an iterator.
    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }

    /// Returns the index of the first element matching the predicate.
    pub fn position<P>(&self, mut predicate: P) -> Option<usize>
    where
        P: FnMut(&T) -> bool,
    {
        self.iter().position(|item| predicate(item))
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

    /// Clones this `CowVec`, creating a fresh arena if the current one exceeds max_capacity.
    ///
    /// If the arena's allocation count exceeds `max_capacity`, a new arena is created
    /// containing only the current elements (compacting the data). Otherwise, the arena
    /// is shared as with regular `clone()`.
    ///
    /// This is useful for controlling memory growth when the arena has accumulated
    /// many allocations from `push`, `set`, or garbage from `pop`/`remove` operations.
    pub fn clone_with_max_capacity(&self, max_capacity: usize) -> Self {
        if self.arena.len() <= max_capacity {
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
            items: new_items,
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
        self.items[index] = ptr;
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
    /// Clones this `CowVec`.
    ///
    /// This is an efficient operation: the arena is shared via `Arc`, and only
    /// the pointer vector is cloned.
    fn clone(&self) -> Self {
        Self {
            arena: Arc::clone(&self.arena),
            items: self.items.clone(),
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
    fn from(vec: Vec<T>) -> Self {
        let arena = Arc::new(CowArena::with_capacity(vec.len()));
        let items: Vec<*const T> = vec.into_iter().map(|item| arena.alloc(item)).collect();
        Self { arena, items }
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

/// # WARNING: HIDDEN ALLOCATION ON EVERY MUTABLE ACCESS
///
/// Unlike `Vec`, mutable indexing on `CowVec` allocates a NEW value in the arena
/// on EVERY access, even if you don't actually modify the value. This is because
/// `CowVec` implements copy-on-write semantics and cannot know at the time of
/// `index_mut()` whether you intend to write.
///
/// ## Examples of Hidden Allocations
///
/// ```
/// use cow_vec::CowVec;
///
/// let mut vec = CowVec::from(vec![1, 2, 3]);
///
/// vec[0] = 5;       // Allocates new value (expected)
/// vec[0] += 1;      // Allocates new value (might be surprising)
/// let _ = &mut vec[0];  // Allocates even if never written to!
///
/// // This loop allocates 100 times:
/// for _ in 0..100 {
///     vec[0] += 1;  // Each iteration allocates
/// }
/// ```
///
/// ## Recommendation
///
/// Prefer using `set()` for mutations - it's explicit about the allocation:
///
/// ```
/// use cow_vec::CowVec;
///
/// let mut vec = CowVec::from(vec![1, 2, 3]);
/// vec.set(0, 5);              // Clear: allocates once
/// vec.set(0, vec[0] + 1);     // Clear: allocates once
/// ```
///
/// Only use `IndexMut` when you need compatibility with code expecting `&mut T`.
impl<T: Clone> IndexMut<usize> for CowVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.items.len() {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len(),
                index
            );
        }
        // Clone the current value to a new arena location (copy-on-write).
        let current = unsafe { &*self.items[index] }.clone();
        let ptr = self.arena.alloc(current);
        self.items[index] = ptr;
        // SAFETY: The pointer was just allocated and is valid. We have exclusive
        // access via &mut self. The arena allocates mutable memory.
        unsafe { &mut *(ptr as *mut T) }
    }
}
