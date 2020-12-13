//! Strided memory access

use std::{
    marker::PhantomData,
    ops::{Index, IndexMut},
    ptr::NonNull,
};

/// A view of every `stride`th location in a region of memory
///
/// Similar to a slice, but not necessarily contiguous.
#[derive(Debug)]
pub struct StridedMut<'a, T> {
    stride: usize,
    len: usize,
    data: NonNull<T>,
    marker: PhantomData<&'a mut T>,
}

impl<'a, T> StridedMut<'a, T> {
    /// Construct a strided view
    pub fn new(slice: &'a mut [T], stride: usize) -> Self {
        assert!(stride > 0);
        Self {
            stride,
            len: slice.len(),
            data: unsafe { NonNull::new_unchecked(slice.as_mut_ptr()) },
            marker: PhantomData,
        }
    }

    /// Construct a strided view from a pointer
    ///
    /// # Safety
    ///
    /// `data` must be non-null and address a sequence of `len * stride` `T` values which will
    /// outlive `'a`, subject to standard aliasing rules for the elements accessible through the
    /// resulting `StridedMut`.
    pub unsafe fn from_raw_parts(data: *mut T, stride: usize, len: usize) -> Self {
        Self {
            stride,
            len,
            data: NonNull::new_unchecked(data),
            marker: PhantomData,
        }
    }

    /// Construct a new view that borrows the same data
    ///
    /// Useful when you want to pass a view along without losing `self`.
    pub fn borrow(&mut self) -> StridedMut<'_, T> {
        StridedMut {
            stride: self.stride,
            len: self.len,
            data: self.data,
            marker: PhantomData,
        }
    }

    /// Number of accessible elements
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no elements are accessible
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over the contents
    pub fn iter_mut<'b>(&'b mut self) -> IterMut<'b, 'a, T> {
        IterMut {
            inner: self,
            index: 0,
        }
    }

    /// Pointer to the first element
    pub fn as_ptr(&self) -> *mut T {
        self.data.as_ptr()
    }

    /// Distance between elements, in element units
    pub fn stride(&self) -> usize {
        self.stride
    }
}

impl<T> Default for StridedMut<'_, T> {
    fn default() -> Self {
        Self {
            stride: 1,
            len: 0,
            data: NonNull::dangling(),
            marker: PhantomData,
        }
    }
}

impl<'a, T> From<&'a mut [T]> for StridedMut<'a, T> {
    fn from(x: &'a mut [T]) -> Self {
        Self::new(x, 1)
    }
}

impl<T> Index<usize> for StridedMut<'_, T> {
    type Output = T;

    fn index(&self, i: usize) -> &T {
        assert!(i < self.len);
        unsafe { &*self.data.as_ptr().add(i * self.stride) }
    }
}

impl<T> IndexMut<usize> for StridedMut<'_, T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        assert!(i < self.len);
        unsafe { &mut *self.data.as_ptr().add(i * self.stride) }
    }
}

/// Iterator over a [`StridedMut`]
#[derive(Debug)]
pub struct IterMut<'a, 'b, T> {
    inner: &'a mut StridedMut<'b, T>,
    index: usize,
}

impl<'a, 'b, T> Iterator for IterMut<'a, 'b, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<&'a mut T> {
        if self.index >= self.inner.len {
            return None;
        }
        let x = unsafe { &mut *self.inner.data.as_ptr().add(self.index * self.inner.stride) };
        self.index += 1;
        Some(x)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl<T> ExactSizeIterator for IterMut<'_, '_, T> {
    fn len(&self) -> usize {
        self.inner.len - self.index
    }
}

impl<'a, 'b, T> IntoIterator for &'a mut StridedMut<'b, T> {
    type IntoIter = IterMut<'a, 'b, T>;
    type Item = &'a mut T;

    fn into_iter(self) -> IterMut<'a, 'b, T> {
        IterMut {
            inner: self,
            index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterate() {
        let mut data = [0, 1, 2];
        let mut xs = StridedMut::from(&mut data[..]);
        assert_eq!(xs.iter_mut().count(), 3);
        let mut i = 0;
        for &mut x in &mut xs {
            assert_eq!(x, i);
            i += 1;
        }
    }
}
