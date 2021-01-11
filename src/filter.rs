use std::marker::PhantomData;

use crate::Handle;

/// A wrapper which transforms a [`Source`](crate::Source)
///
/// Allows [`Handle::control`] to expose the transformed source as well as the transformer. For
/// example, a `Handle<Spatial<Gain<_>>>` allows both gain and motion state to be updated.
pub trait Filter {
    /// Type of source transformed by this filter
    type Inner;

    /// Access the inner source
    fn inner(&self) -> &Self::Inner;
}

/// A [`Source`] or transformer that can be safely controlled from another thread
///
/// # Safety
///
/// `make_control` and `Control` must not permit access to `&Self` that constitutes a data race with
/// concurrent invocation of any [`Source`] method even if `Self: !Sync`. For example, an
/// implementation could restrict itself to atomic operations.
///
/// [`Source`]: crate::Source
pub unsafe trait Controlled<'a>: Sized + 'a {
    /// The interface through which this source can be safely controlled
    type Control;

    /// Construct a `Control` for `source`
    fn make_control(source: &'a Self) -> Self::Control;
}

impl<T> Handle<T> {
    /// Get the control for [`Controlled`] source `S` in a chain of sources
    ///
    /// `Index` can usually be inferred.
    ///
    /// # Example
    /// ```
    /// # use oddio::*;
    /// fn quiet(source: &mut Handle<Spatial<Gain<FramesSource<Sample>>>>) {
    ///     source.control::<Gain<_>, _>().set_gain(0.5);
    /// }
    /// ```
    pub fn control<'a, S, Index>(&'a mut self) -> S::Control
    where
        T: FilterHaving<S, Index>,
        S: Controlled<'a>,
    {
        let source: &S = self.shared.source.get();
        S::make_control(source)
    }
}

/// Filter chains that contain a `T` at any position
///
/// Helper trait for [`Handle::control()`]. `Index` is [`Here`] or [`There`], and can generally be
/// inferred.
pub trait FilterHaving<T, Index> {
    /// Get the `T` element of a filter chain
    fn get(&self) -> &T;
}

/// `Index` value for [`FilterHaving`] representing the first filter in the chain
pub struct Here(());

/// `Index` value for [`FilterHaving`] representing the filter at position `T+1`
pub struct There<T>(PhantomData<T>);

impl<T> FilterHaving<T, Here> for T {
    fn get(&self) -> &T {
        self
    }
}

impl<T: Filter, U, I> FilterHaving<U, There<I>> for T
where
    T::Inner: FilterHaving<U, I>,
{
    fn get(&self) -> &U {
        self.inner().get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _foo<A, B, C>(x: &A)
    where
        A: Filter<Inner = B>,
        B: Filter<Inner = C>,
    {
        let _: &A = x.get();
        let _: &B = x.get();
        let _: &C = x.get();
    }
}
