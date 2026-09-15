use std::fmt;
use std::iter::FusedIterator;
use std::ops::Index;
use std::sync::Arc;

use crate::storage::Storage;

/// A fixed-size page of element pointers.
///
/// Pages are shared between clones via `Arc` and copied on write, so
/// diverging one element copies one page (`N` pointers), not the whole
/// vector.
struct Page<T, const N: usize> {
    slots: [*const T; N],
}

impl<T, const N: usize> Page<T, N> {
    fn empty() -> Self {
        Self {
            slots: [std::ptr::null(); N],
        }
    }
}

impl<T, const N: usize> Clone for Page<T, N> {
    /// Copies `N` pointers; elements are never touched.
    fn clone(&self) -> Self {
        Self { slots: self.slots }
    }
}

// SAFETY: A page holds raw pointers into arenas owned by the PagedVec's
// storage; the containing PagedVec's Send/Sync bounds (T: Send + Sync)
// justify moving and sharing them across threads. See the PagedVec impls.
unsafe impl<T: Send + Sync, const N: usize> Send for Page<T, N> {}
unsafe impl<T: Send + Sync, const N: usize> Sync for Page<T, N> {}

/// A vector-like container with O(1) clone and page-granular copy-on-write.
///
/// Where [`CowVec`](crate::CowVec) keeps one flat pointer array - and must
/// copy all of it on the first mutation after a clone - `PagedVec` splits
/// the pointer array into fixed-size pages behind a small root table:
///
/// ```text
/// root: Arc<Vec<Arc<Page>>>      one entry per PAGE_SIZE elements
/// page: [*const T; PAGE_SIZE]    pointers into shared arena storage
/// ```
///
/// Mutating an element after a clone copies the root table and the one
/// touched page instead of the whole pointer array. For a vector of 4M
/// elements with the default page size, diverging 50 scattered elements
/// copies ~430 KB instead of 32 MB.
///
/// The cost is one extra pointer hop on access compared to `CowVec`:
/// root table entry, then page slot, then value. The root table is small
/// (8 bytes per 1024 elements) and stays cache-resident, so indexed access
/// remains O(1) with a fixed depth.
///
/// # Page size
/// `PAGE_SIZE` must be a power of two. Larger pages mean cheaper indexing
/// and iteration but more copying per diverged page; the default of 1024
/// (8 KB pages) suits most workloads.
///
/// # Thread Safety
/// `PagedVec<T>` is `Send` and `Sync` when `T: Send + Sync`. Allocation
/// takes no lock, as with `CowVec`.
///
/// # Example
/// ```
/// use cow_vec::PagedVec;
///
/// let v1: PagedVec<i32> = (0..10_000).collect();
/// let mut v2 = v1.clone(); // O(1)
/// v2.set(5_000, -1);       // copies the root table and one 8 KB page
/// assert_eq!(v1[5_000], 5_000);
/// assert_eq!(v2[5_000], -1);
/// ```
pub struct PagedVec<T, const PAGE_SIZE: usize = 1024> {
    pages: Arc<Vec<Arc<Page<T, PAGE_SIZE>>>>,
    len: usize,
    storage: Storage<T>,
}

// SAFETY: Same reasoning as CowVec (see cow_vec.rs): storage allocation is
// exclusive-owner-only under &mut self, pointers stay valid for the
// storage's lifetime, and only &T is exposed. The manual impls exist
// because the raw pointers in pages suppress the automatic ones.
unsafe impl<T: Send + Sync, const N: usize> Send for PagedVec<T, N> {}
unsafe impl<T: Send + Sync, const N: usize> Sync for PagedVec<T, N> {}

impl<T, const PAGE_SIZE: usize> PagedVec<T, PAGE_SIZE> {
    /// Compile-time check; referenced by every constructor.
    const PAGE_SIZE_IS_POWER_OF_TWO: () = assert!(
        PAGE_SIZE.is_power_of_two(),
        "PAGE_SIZE must be a power of two"
    );

    /// Splits an element index into (page, slot).
    ///
    /// PAGE_SIZE is a power of two, so this compiles to a shift and a mask.
    #[inline]
    const fn split(index: usize) -> (usize, usize) {
        (index / PAGE_SIZE, index % PAGE_SIZE)
    }

    /// Copy-on-write access to the root table.
    ///
    /// A shared root is copied only up to the last page `len` reaches. Any
    /// trailing pages a shared `truncate` could not release go with it,
    /// whether the root was shared or has since become private.
    fn pages_mut(&mut self) -> &mut Vec<Arc<Page<T, PAGE_SIZE>>> {
        let needed = self.len.div_ceil(PAGE_SIZE);
        if Arc::strong_count(&self.pages) > 1 {
            self.pages = Arc::new(self.pages[..needed].to_vec());
        }
        let pages = Arc::make_mut(&mut self.pages);
        pages.truncate(needed);
        pages
    }

    /// Creates a new empty `PagedVec`.
    pub fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::PAGE_SIZE_IS_POWER_OF_TWO;
        Self {
            pages: Arc::new(Vec::new()),
            len: 0,
            storage: Storage::new(),
        }
    }

    /// Creates a new `PagedVec` with room for `capacity` elements.
    pub fn with_capacity(capacity: usize) -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::PAGE_SIZE_IS_POWER_OF_TWO;
        Self {
            pages: Arc::new(Vec::with_capacity(capacity.div_ceil(PAGE_SIZE))),
            len: 0,
            storage: Storage::with_capacity(capacity),
        }
    }

    /// Returns the number of elements in this vector.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if this vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` if the structure (the root page table) is shared with
    /// other clones. Individual pages may remain shared even when this
    /// returns `false`.
    pub fn is_structure_shared(&self) -> bool {
        Arc::strong_count(&self.pages) > 1
    }

    /// Returns `true` if the storage (arenas with actual values) may be
    /// shared with other clones. Conservative, like
    /// [`CowVec::is_storage_shared`](crate::CowVec::is_storage_shared).
    pub fn is_storage_shared(&self) -> bool {
        self.storage.is_shared()
    }

    /// Returns the number of values this vector's storage keeps alive,
    /// including values no longer reachable. See
    /// [`CowVec::storage_allocations`](crate::CowVec::storage_allocations).
    pub fn storage_allocations(&self) -> usize {
        self.storage.allocated()
    }

    /// Returns a reference to the element at the given index, or `None` if
    /// out of bounds. Two pointer hops: root table entry to the page, page
    /// slot to the value.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let (page, slot) = Self::split(index);
        let ptr = self.pages[page].slots[slot];
        // SAFETY: Every slot below self.len holds a pointer produced by
        // self.storage, which keeps its arenas alive for as long as this
        // vector (or any absorbing vector) exists.
        Some(unsafe { &*ptr })
    }

    /// Appends an element to the back of this vector.
    ///
    /// Copy-on-write: if the root table or the last page is shared with
    /// clones, only those are copied (the page holds `PAGE_SIZE` pointers).
    pub fn push(&mut self, value: T) {
        let ptr = self.storage.alloc(value);
        let (page, slot) = Self::split(self.len);
        let pages = self.pages_mut();
        if page == pages.len() {
            pages.push(Arc::new(Page::empty()));
        }
        Arc::make_mut(&mut pages[page]).slots[slot] = ptr;
        self.len += 1;
    }

    /// Sets the value at the given index (copy-on-write).
    ///
    /// Allocates the new value in this instance's storage and rewrites one
    /// pointer, copying the root table and the touched page only if they
    /// are shared. Other clones keep seeing the original value.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn set(&mut self, index: usize, value: T) {
        if index >= self.len {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len, index
            );
        }
        let ptr = self.storage.alloc(value);
        let (page, slot) = Self::split(index);
        let pages = self.pages_mut();
        Arc::make_mut(&mut pages[page]).slots[slot] = ptr;
    }

    /// Returns a mutable reference to the element at `index`, first copying
    /// its value to a fresh storage slot (copy-on-write). See
    /// [`CowVec::make_mut`](crate::CowVec::make_mut) for the cost model.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone,
    {
        if index >= self.len {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len, index
            );
        }
        let (page, slot) = Self::split(index);
        // SAFETY: In-bounds slot; see get().
        let current = unsafe { &*self.pages[page].slots[slot] }.clone();
        let ptr = self.storage.alloc(current);
        let pages = self.pages_mut();
        Arc::make_mut(&mut pages[page]).slots[slot] = ptr;
        // SAFETY: Freshly allocated with write provenance; no other vector
        // references it, and the returned borrow keeps &mut self alive.
        unsafe { &mut *(ptr as *mut T) }
    }

    /// Removes the last element and returns it, or `None` if empty.
    ///
    /// This is O(1) and copies nothing: only the length shrinks. The value
    /// is cloned out of the shared storage, where the original stays
    /// allocated. To drop elements without `T: Clone`, use
    /// [`truncate`](Self::truncate).
    pub fn pop(&mut self) -> Option<T>
    where
        T: Clone,
    {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        let (page, slot) = Self::split(self.len);
        let ptr = self.pages[page].slots[slot];
        // SAFETY: The slot was in bounds before the decrement; see get().
        Some(unsafe { &*ptr }.clone())
    }

    /// Shortens the vector, keeping the first `len` elements.
    ///
    /// Copies nothing. Whole trailing pages are released when the root
    /// table is not shared; if it is, they are released by the next write
    /// that takes ownership of the root.
    pub fn truncate(&mut self, len: usize) {
        if len < self.len {
            self.len = len;
        }
        if let Some(pages) = Arc::get_mut(&mut self.pages) {
            let needed = self.len.div_ceil(PAGE_SIZE);
            if pages.len() > needed {
                pages.truncate(needed);
            }
        }
    }

    /// Clears the vector, removing all elements.
    pub fn clear(&mut self) {
        self.truncate(0);
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
            self.get(self.len - 1)
        }
    }

    /// Returns an iterator over references to the elements.
    pub fn iter(&self) -> PagedVecIter<'_, T, PAGE_SIZE> {
        PagedVecIter {
            pages: &self.pages,
            front: 0,
            back: self.len,
            front_page: usize::MAX,
            front_slots: None,
            back_page: usize::MAX,
            back_slots: None,
        }
    }

    /// Bulk-appends pointers, filling pages chunk by chunk.
    fn extend_ptrs(&mut self, ptrs: Vec<*const T>) {
        let mut len = self.len;
        let pages = self.pages_mut();
        let mut i = 0;
        while i < ptrs.len() {
            let (page, slot) = Self::split(len);
            if page == pages.len() {
                pages.push(Arc::new(Page::empty()));
            }
            let target = Arc::make_mut(&mut pages[page]);
            let take = (PAGE_SIZE - slot).min(ptrs.len() - i);
            target.slots[slot..slot + take].copy_from_slice(&ptrs[i..i + take]);
            len += take;
            i += take;
        }
        self.len = len;
    }

    /// Test hook: address of a page's allocation, for asserting sharing.
    #[cfg(test)]
    pub(crate) fn page_addr(&self, page: usize) -> usize {
        Arc::as_ptr(&self.pages[page]) as usize
    }

    /// Test hook: number of pages the root table currently holds.
    #[cfg(test)]
    pub(crate) fn page_count(&self) -> usize {
        self.pages.len()
    }
}

impl<T: PartialEq, const N: usize> PagedVec<T, N> {
    /// Returns `true` if the vector contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        self.iter().any(|item| item == value)
    }
}

impl<T: Clone, const N: usize> PagedVec<T, N> {
    /// Converts this `PagedVec` into a `Vec` by cloning all elements.
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    /// Clones this `PagedVec`, compacting into fresh storage (and fresh
    /// pages) if the current storage holds more than `max_allocations`
    /// values. See [`CowVec::clone_compacted`](crate::CowVec::clone_compacted).
    pub fn clone_compacted(&self, max_allocations: usize) -> Self {
        if self.storage.allocated() <= max_allocations {
            return self.clone();
        }
        let mut fresh = Self::with_capacity(self.len);
        let ptrs = fresh.storage.alloc_extend(self.iter().cloned());
        fresh.extend_ptrs(ptrs);
        fresh
    }
}

impl<T, const N: usize> Clone for PagedVec<T, N> {
    /// Clones this `PagedVec` in O(1) time.
    ///
    /// The root table, every page, and the storage are shared. Copying
    /// happens page-by-page as either side mutates.
    fn clone(&self) -> Self {
        Self {
            pages: Arc::clone(&self.pages),
            len: self.len,
            storage: self.storage.clone(),
        }
    }
}

impl<T, const N: usize> Default for PagedVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for PagedVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> FromIterator<T> for PagedVec<T, N> {
    /// Creates a `PagedVec` from an iterator, bulk-allocating contiguously.
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut vec = Self::new();
        let ptrs = vec.storage.alloc_extend(iter);
        vec.extend_ptrs(ptrs);
        vec
    }
}

impl<T, const N: usize> Extend<T> for PagedVec<T, N> {
    /// Extends the vector, bulk-allocating contiguously.
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let ptrs = self.storage.alloc_extend(iter);
        self.extend_ptrs(ptrs);
    }
}

impl<T, const N: usize> From<Vec<T>> for PagedVec<T, N> {
    /// Creates a `PagedVec` from a `Vec`.
    fn from(vec: Vec<T>) -> Self {
        vec.into_iter().collect()
    }
}

impl<T: Clone, const N: usize> From<&[T]> for PagedVec<T, N> {
    /// Creates a `PagedVec` by cloning the elements of a slice.
    fn from(slice: &[T]) -> Self {
        slice.iter().cloned().collect()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for PagedVec<T, N> {
    /// Compares element by element, with an O(1) fast path for clones that
    /// still share their root table. As with `CowVec`, that fast path
    /// ignores non-reflexive equality such as `f64::NAN`.
    fn eq(&self, other: &Self) -> bool {
        (self.len == other.len && Arc::ptr_eq(&self.pages, &other.pages))
            || (self.len == other.len && self.iter().zip(other.iter()).all(|(a, b)| a == b))
    }
}

impl<T: Eq, const N: usize> Eq for PagedVec<T, N> {}

impl<T: PartialEq, const N: usize> PartialEq<[T]> for PagedVec<T, N> {
    fn eq(&self, other: &[T]) -> bool {
        self.len == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl<T: PartialEq, const N: usize> PartialEq<Vec<T>> for PagedVec<T, N> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self == other.as_slice()
    }
}

impl<T: PartialOrd, const N: usize> PartialOrd for PagedVec<T, N> {
    /// Lexicographic comparison, like `Vec`.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T: Ord, const N: usize> Ord for PagedVec<T, N> {
    /// Lexicographic comparison, like `Vec`.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<T: std::hash::Hash, const N: usize> std::hash::Hash for PagedVec<T, N> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.len.hash(state);
        for item in self {
            item.hash(state);
        }
    }
}

impl<T, const N: usize> Index<usize> for PagedVec<T, N> {
    type Output = T;

    /// Returns a reference to the element at the given index.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("index out of bounds")
    }
}

/// An iterator over the elements of a `PagedVec`.
///
/// Walks the pointer pages directly: crossing into a page costs one root
/// table lookup, and every element inside it is then a plain array index.
pub struct PagedVecIter<'a, T, const PAGE_SIZE: usize> {
    pages: &'a [Arc<Page<T, PAGE_SIZE>>],
    front: usize,
    /// Exclusive.
    back: usize,
    /// Index of the page cached in `front_slots`; `usize::MAX` when none.
    front_page: usize,
    front_slots: Option<&'a [*const T; PAGE_SIZE]>,
    /// Index of the page cached in `back_slots`; `usize::MAX` when none.
    back_page: usize,
    back_slots: Option<&'a [*const T; PAGE_SIZE]>,
}

impl<'a, T, const N: usize> PagedVecIter<'a, T, N> {
    /// Pointer at `index`, going through the front page cache.
    #[inline]
    fn front_ptr(&mut self, index: usize) -> *const T {
        let (page, slot) = PagedVec::<T, N>::split(index);
        let slots = match self.front_slots {
            Some(slots) if page == self.front_page => slots,
            _ => {
                let slots = &self.pages[page].slots;
                self.front_page = page;
                self.front_slots = Some(slots);
                slots
            }
        };
        slots[slot]
    }

    /// Pointer at `index`, going through the back page cache.
    #[inline]
    fn back_ptr(&mut self, index: usize) -> *const T {
        let (page, slot) = PagedVec::<T, N>::split(index);
        let slots = match self.back_slots {
            Some(slots) if page == self.back_page => slots,
            _ => {
                let slots = &self.pages[page].slots;
                self.back_page = page;
                self.back_slots = Some(slots);
                slots
            }
        };
        slots[slot]
    }
}

impl<'a, T, const N: usize> Iterator for PagedVecIter<'a, T, N> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        if self.front < self.back {
            let ptr = self.front_ptr(self.front);
            self.front += 1;
            // SAFETY: In-bounds slot of the borrowed vector; see
            // PagedVec::get.
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<&'a T> {
        self.front = self.front.saturating_add(n).min(self.back);
        self.next()
    }

    #[inline]
    fn count(self) -> usize {
        self.back - self.front
    }

    #[inline]
    fn last(mut self) -> Option<&'a T> {
        self.next_back()
    }

    /// Runs page by page, so the inner loop is a plain slice walk.
    fn fold<B, F>(mut self, init: B, mut f: F) -> B
    where
        F: FnMut(B, &'a T) -> B,
    {
        let mut acc = init;
        while self.front < self.back {
            let (page, slot) = PagedVec::<T, N>::split(self.front);
            let end = (self.back - page * N).min(N);
            for &ptr in &self.pages[page].slots[slot..end] {
                // SAFETY: As in next().
                acc = f(acc, unsafe { &*ptr });
            }
            self.front += end - slot;
        }
        acc
    }
}

impl<'a, T, const N: usize> DoubleEndedIterator for PagedVecIter<'a, T, N> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        if self.front < self.back {
            self.back -= 1;
            let ptr = self.back_ptr(self.back);
            // SAFETY: As in next().
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<&'a T> {
        self.back = self.back.saturating_sub(n).max(self.front);
        self.next_back()
    }

    /// Runs page by page from the back; see `fold`.
    fn rfold<B, F>(mut self, init: B, mut f: F) -> B
    where
        F: FnMut(B, &'a T) -> B,
    {
        let mut acc = init;
        while self.front < self.back {
            let (page, end_slot) = PagedVec::<T, N>::split(self.back - 1);
            let start = self.front.saturating_sub(page * N);
            for &ptr in self.pages[page].slots[start..=end_slot].iter().rev() {
                // SAFETY: As in next().
                acc = f(acc, unsafe { &*ptr });
            }
            self.back -= end_slot + 1 - start;
        }
        acc
    }
}

impl<T, const N: usize> ExactSizeIterator for PagedVecIter<'_, T, N> {
    #[inline]
    fn len(&self) -> usize {
        self.back - self.front
    }
}

impl<T, const N: usize> FusedIterator for PagedVecIter<'_, T, N> {}

impl<T, const N: usize> Clone for PagedVecIter<'_, T, N> {
    fn clone(&self) -> Self {
        Self { ..*self }
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a PagedVec<T, N> {
    type Item = &'a T;
    type IntoIter = PagedVecIter<'a, T, N>;

    /// Creates an iterator over references to the elements.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
