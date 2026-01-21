use super::CowVec;

/// An iterator over the elements of a `CowVec`.
pub struct CowVecIter<'a, T> {
    pub(super) vec: &'a CowVec<T>,
    pub(super) position: usize,
}

impl<'a, T> Iterator for CowVecIter<'a, T> {
    type Item = &'a T;

    /// Advances the iterator and returns the next element.
    fn next(&mut self) -> Option<Self::Item> {
        if self.position < self.vec.len() {
            let item = self.vec.get(self.position);
            self.position += 1;
            item
        } else {
            None
        }
    }

    /// Returns the bounds on the remaining length of the iterator.
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.len() - self.position;
        (remaining, Some(remaining))
    }
}

impl<T> ExactSizeIterator for CowVecIter<'_, T> {}

impl<'a, T> IntoIterator for &'a CowVec<T> {
    type Item = &'a T;
    type IntoIter = CowVecIter<'a, T>;

    /// Creates an iterator over references to the elements.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
