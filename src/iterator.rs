use std::iter::FusedIterator;
use std::marker::PhantomData;

use super::CowVec;

/// An iterator over the elements of a `CowVec`.
///
/// Wraps a slice iterator over the internal pointer array, so it inherits
/// the slice iterator's performance characteristics and supports iteration
/// from both ends.
pub struct CowVecIter<'a, T> {
    inner: std::slice::Iter<'a, *const T>,
    /// Ties the yielded `&'a T` values to the borrow of the `CowVec`.
    _values: PhantomData<&'a T>,
}

impl<'a, T> CowVecIter<'a, T> {
    pub(super) fn new(vec: &'a CowVec<T>) -> Self {
        Self {
            inner: vec.items_slice().iter(),
            _values: PhantomData,
        }
    }
}

/// Dereferences a pointer stored in the items vector.
///
/// # Safety
/// The pointer must originate from the `CowVec` borrowed for `'a`. Its arena
/// is kept alive by that `CowVec` and never moves or drops values, so the
/// reference is valid for `'a`.
#[inline]
unsafe fn deref<'a, T>(ptr: &*const T) -> &'a T {
    &**ptr
}

impl<'a, T> Iterator for CowVecIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        // SAFETY: See `deref` - pointers come from the borrowed CowVec.
        self.inner.next().map(|ptr| unsafe { deref(ptr) })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<&'a T> {
        // SAFETY: See `deref`.
        self.inner.nth(n).map(|ptr| unsafe { deref(ptr) })
    }

    #[inline]
    fn count(self) -> usize {
        self.inner.count()
    }

    #[inline]
    fn last(self) -> Option<&'a T> {
        // SAFETY: See `deref`.
        self.inner.last().map(|ptr| unsafe { deref(ptr) })
    }

    #[inline]
    fn fold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, &'a T) -> B,
    {
        // SAFETY: See `deref`.
        self.inner
            .fold(init, |acc, ptr| f(acc, unsafe { deref(ptr) }))
    }
}

impl<'a, T> DoubleEndedIterator for CowVecIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        // SAFETY: See `deref`.
        self.inner.next_back().map(|ptr| unsafe { deref(ptr) })
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<&'a T> {
        // SAFETY: See `deref`.
        self.inner.nth_back(n).map(|ptr| unsafe { deref(ptr) })
    }

    #[inline]
    fn rfold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, &'a T) -> B,
    {
        // SAFETY: See `deref`.
        self.inner
            .rfold(init, |acc, ptr| f(acc, unsafe { deref(ptr) }))
    }
}

impl<T> ExactSizeIterator for CowVecIter<'_, T> {
    #[inline]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<T> FusedIterator for CowVecIter<'_, T> {}

impl<T> Clone for CowVecIter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _values: PhantomData,
        }
    }
}

impl<'a, T> IntoIterator for &'a CowVec<T> {
    type Item = &'a T;
    type IntoIter = CowVecIter<'a, T>;

    /// Creates an iterator over references to the elements.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
