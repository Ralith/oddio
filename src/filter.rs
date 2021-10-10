use alloc::sync::Arc;
use core::marker::PhantomData;

/// Handle for manipulating a signal owned elsewhere
///
/// Handle types are typically verbose. Consider using type aliases or newtypes as shorthand for
/// those that arise commonly in your application.
pub struct Handle<T: ?Sized> {
    shared: Arc<T>,
}

impl<T: ?Sized> Handle<T> {
    /// Construct a handle enclosing `signal`
    ///
    /// Used to implement signals like [`Mixer`](crate::Mixer).
    ///
    /// # Safety
    ///
    /// There must never be more than one other `Arc` referencing the same `T`.
    pub unsafe fn from_arc(signal: Arc<T>) -> Self {
        Self { shared: signal }
    }

    /// Get the control for [`Controlled`] signal `S` in a chain of signals
    ///
    /// `Index` can usually be inferred.
    ///
    /// # Example
    /// ```
    /// # use oddio::*;
    /// fn quiet(signal: &mut Handle<Spatial<Gain<FramesSignal<Sample>>>>) {
    ///     signal.control::<Gain<_>, _>().set_gain(-3.0);
    /// }
    /// ```
    pub fn control<'a, S, Index>(&'a mut self) -> S::Control
    where
        T: FilterHaving<S, Index>,
        S: Controlled<'a>,
    {
        let signal: &S = (*self.shared).get();
        unsafe { S::make_control(signal) }
    }
}

// Sound because `T` is not accessible except via `unsafe trait Controlled`
unsafe impl<T> Send for Handle<T> {}
unsafe impl<T> Sync for Handle<T> {}

/// A wrapper which transforms a [`Signal`](crate::Signal)
///
/// Allows [`Handle::control`] to expose the transformed signal as well as the transformer. For
/// example, a `Handle<Spatial<Gain<_>>>` allows both gain and motion state to be updated.
pub trait Filter {
    /// Type of signal transformed by this filter
    type Inner: ?Sized;

    /// Access the inner signal
    fn inner(&self) -> &Self::Inner;
}

/// A [`Signal`] or transformer that can be safely controlled from another thread
///
/// # Safety
///
/// `make_control` and `Control` must not permit access to `&Self` that constitutes a data race with
/// concurrent invocation of any [`Signal`] method even if `Self: !Sync`. For example, an
/// implementation could restrict itself to atomic operations.
///
/// [`Signal`]: crate::Signal
pub unsafe trait Controlled<'a>: Sized + 'a {
    /// The interface through which this signal can be safely controlled
    type Control;

    /// Construct a `Control` for `signal`
    ///
    /// # Safety
    ///
    /// Must not be invoked while another `Control` for this signal exists
    unsafe fn make_control(signal: &'a Self) -> Self::Control;
}

/// Filter chains that contain a `T` at any position
///
/// Helper trait for [`Handle::control()`]. `Index` is [`Here`] or [`There`], and can generally be
/// inferred.
pub trait FilterHaving<T: ?Sized, Index> {
    /// Get the `T` element of a filter chain
    fn get(&self) -> &T;
}

/// `Index` value for [`FilterHaving`] representing the first filter in the chain
pub struct Here(());

/// `Index` value for [`FilterHaving`] representing the filter at position `T+1`
pub struct There<T>(PhantomData<T>);

impl<T: ?Sized> FilterHaving<T, Here> for T {
    fn get(&self) -> &T {
        self
    }
}

impl<T: Filter + ?Sized, U: ?Sized, I> FilterHaving<U, There<I>> for T
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
