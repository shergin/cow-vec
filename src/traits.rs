use std::ops::Index;

use crate::{CowVec, CowVecIter, PagedVec, PagedVecIter};

/// The API shared by [`CowVec`] and [`PagedVec`], for code that wants to
/// be generic over which one it snapshots.
///
/// Both types implement every method here inherently as well, with fuller
/// documentation; this trait only makes the common subset nameable:
///
/// ```
/// use cow_vec::{CowVec, CowVector, PagedVec};
///
/// fn branch<V: CowVector<String>>(base: &V) -> V {
///     let mut next = base.clone(); // O(1)
///     next.set(0, "changed".to_string());
///     next
/// }
///
/// let flat = CowVec::from(vec!["a".to_string(), "b".to_string()]);
/// let paged: PagedVec<String> = flat.to_vec().into();
/// assert_eq!(branch(&flat)[0], "changed");
/// assert_eq!(branch(&paged)[0], "changed");
/// assert_eq!(flat[0], "a");
/// ```
pub trait CowVector<T>:
    Clone + Default + Extend<T> + FromIterator<T> + From<Vec<T>> + Index<usize, Output = T>
{
    /// The iterator returned by [`iter`](Self::iter).
    type Iter<'a>: Iterator<Item = &'a T> + DoubleEndedIterator + ExactSizeIterator
    where
        Self: 'a,
        T: 'a;

    /// Creates an empty vector.
    fn new() -> Self;

    /// Returns the number of elements.
    fn len(&self) -> usize;

    /// Returns `true` if the vector holds no elements.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the element at `index`, or `None` if out of bounds.
    fn get(&self, index: usize) -> Option<&T>;

    /// Returns the first element, or `None` if empty.
    fn first(&self) -> Option<&T> {
        self.get(0)
    }

    /// Returns the last element, or `None` if empty.
    fn last(&self) -> Option<&T> {
        self.get(self.len().checked_sub(1)?)
    }

    /// Iterates over references to the elements.
    fn iter(&self) -> Self::Iter<'_>;

    /// Appends an element.
    fn push(&mut self, value: T);

    /// Removes the last element and returns it, moving it out when the
    /// storage is not shared and cloning it otherwise.
    fn pop(&mut self) -> Option<T>
    where
        T: Clone;

    /// Replaces the element at `index` (copy-on-write).
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    fn set(&mut self, index: usize, value: T);

    /// Returns the element at `index` mutably, cloning it into private
    /// storage first if clones may still see it (copy-on-write).
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone;

    /// Keeps the first `len` elements.
    fn truncate(&mut self, len: usize);

    /// Removes every element.
    fn clear(&mut self) {
        self.truncate(0);
    }

    /// Clones every element into a `Vec`.
    fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.iter().cloned().collect()
    }

    /// Rebuilds the storage to hold just the live elements if it keeps more
    /// than `max_allocations` values alive.
    fn compact(&mut self, max_allocations: usize)
    where
        T: Clone;

    /// Returns a clone whose storage is rebuilt to hold just the live
    /// elements if this storage keeps more than `max_allocations` values
    /// alive; a plain `clone()` otherwise.
    fn clone_compacted(&self, max_allocations: usize) -> Self
    where
        T: Clone;

    /// Number of values the storage keeps alive, reachable or not.
    fn storage_allocations(&self) -> usize;

    /// Returns `true` if the element pointers are shared with clones.
    fn is_structure_shared(&self) -> bool;

    /// Returns `true` if the storage may be shared with clones.
    fn is_storage_shared(&self) -> bool;
}

impl<T> CowVector<T> for CowVec<T> {
    type Iter<'a>
        = CowVecIter<'a, T>
    where
        T: 'a;

    fn new() -> Self {
        Self::new()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.get(index)
    }

    fn iter(&self) -> Self::Iter<'_> {
        self.iter()
    }

    fn push(&mut self, value: T) {
        self.push(value);
    }

    fn pop(&mut self) -> Option<T>
    where
        T: Clone,
    {
        self.pop()
    }

    fn set(&mut self, index: usize, value: T) {
        self.set(index, value);
    }

    fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone,
    {
        self.make_mut(index)
    }

    fn truncate(&mut self, len: usize) {
        self.truncate(len);
    }

    fn clear(&mut self) {
        self.clear();
    }

    fn compact(&mut self, max_allocations: usize)
    where
        T: Clone,
    {
        self.compact(max_allocations);
    }

    fn clone_compacted(&self, max_allocations: usize) -> Self
    where
        T: Clone,
    {
        self.clone_compacted(max_allocations)
    }

    fn storage_allocations(&self) -> usize {
        self.storage_allocations()
    }

    fn is_structure_shared(&self) -> bool {
        self.is_structure_shared()
    }

    fn is_storage_shared(&self) -> bool {
        self.is_storage_shared()
    }
}

impl<T, const N: usize> CowVector<T> for PagedVec<T, N> {
    type Iter<'a>
        = PagedVecIter<'a, T, N>
    where
        T: 'a;

    fn new() -> Self {
        Self::new()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.get(index)
    }

    fn iter(&self) -> Self::Iter<'_> {
        self.iter()
    }

    fn push(&mut self, value: T) {
        self.push(value);
    }

    fn pop(&mut self) -> Option<T>
    where
        T: Clone,
    {
        self.pop()
    }

    fn set(&mut self, index: usize, value: T) {
        self.set(index, value);
    }

    fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone,
    {
        self.make_mut(index)
    }

    fn truncate(&mut self, len: usize) {
        self.truncate(len);
    }

    fn clear(&mut self) {
        self.clear();
    }

    fn compact(&mut self, max_allocations: usize)
    where
        T: Clone,
    {
        self.compact(max_allocations);
    }

    fn clone_compacted(&self, max_allocations: usize) -> Self
    where
        T: Clone,
    {
        self.clone_compacted(max_allocations)
    }

    fn storage_allocations(&self) -> usize {
        self.storage_allocations()
    }

    fn is_structure_shared(&self) -> bool {
        self.is_structure_shared()
    }

    fn is_storage_shared(&self) -> bool {
        self.is_storage_shared()
    }
}
