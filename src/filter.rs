use std::marker::PhantomData;

use crate::Handle;

/// A [`Source`](crate::Source) which transforms another source
pub trait Filter {
    /// Type of source transformed by this filter
    type Inner;

    /// Access the inner source
    fn inner(&self) -> &Self::Inner;
}

impl<T> Handle<T> {
    /// Get the control for source `S` in a chain of sources
    ///
    /// `Index` can usually be inferred.
    ///
    /// # Example
    /// ```
    /// # use oddio::*;
    /// fn quiet(source: &Handle<Spatial<Gain<FramesSource<Sample>>>>) {
    ///     source.control::<Gain<_>, _>().set_gain(0.5);
    /// }
    /// ```
    pub fn control<S, Index>(&self) -> Control<'_, S>
    where
        T: FilterHaving<S, Index>,
    {
        unsafe { Control(&(*self.get()).get()) }
    }
}

/// Control for a specific element of a chain of sources
///
/// Obtained from [`Handle::control`].
pub struct Control<'a, T>(&'a T);

impl<T> Control<'_, T> {
    /// Access a potentially `!Sync` source
    ///
    /// Building block for safe abstractions over nontrivial shared memory.
    pub fn get(&self) -> *const T {
        self.0
    }
}

impl<T: Sync> AsRef<T> for Control<'_, T> {
    fn as_ref(&self) -> &T {
        self.0
    }
}

/// Filter chains that contain a `T` at any position
///
/// `Index` is [`Here`] or [`There`], and can generally be inferred.
pub trait FilterHaving<T, Index> {
    /// Get the `T` element of a filter chain
    fn get(&self) -> &T;
}

/// `Index` value for `FilterHaving` representing the first filter in the chain
pub struct Here(());

/// `Index` value for `FilterHaving` representing the filter at position `T+1`
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
