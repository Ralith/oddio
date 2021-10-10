use core::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{AtomicUsize, Ordering},
};

/// SPSC queue that only retains the last element sent
///
/// Useful for custom controllable signals.
pub struct Swap<T> {
    slots: [UnsafeCell<T>; 3],
    send: Cell<usize>,
    shared: AtomicUsize,
    recv: Cell<usize>,
}

impl<T> Swap<T> {
    /// Create a channel initially holding `x`
    pub fn new(x: T) -> Self
    where
        T: Clone,
    {
        Self {
            slots: [
                UnsafeCell::new(x.clone()),
                UnsafeCell::new(x.clone()),
                UnsafeCell::new(x),
            ],
            send: Cell::new(0),
            shared: AtomicUsize::new(1),
            recv: Cell::new(2),
        }
    }

    /// Access the value that will be sent next. Producer only.
    pub fn pending(&self) -> *mut T {
        self.slots[self.send.get()].get()
    }

    /// Send the value from `pending`. Producer only.
    pub fn flush(&self) {
        self.send.set(
            self.shared
                .swap(self.send.get() | FRESH_BIT, Ordering::AcqRel)
                & INDEX_MASK,
        );
    }

    /// Update the value exposed by `recv`. Returns whether new data was obtained. Consumer only.
    pub fn refresh(&self) -> bool {
        if self.shared.load(Ordering::Relaxed) & FRESH_BIT == 0 {
            return false;
        }
        self.recv
            .set(self.shared.swap(self.recv.get(), Ordering::AcqRel) & INDEX_MASK);
        true
    }

    /// Access the most recent data as of the last `refresh` call. Consumer only.
    pub fn received(&self) -> *mut T {
        self.slots[self.recv.get()].get()
    }
}

impl<T: Default> Default for Swap<T> {
    fn default() -> Self {
        Self {
            slots: Default::default(),
            send: Cell::new(0),
            shared: AtomicUsize::new(1),
            recv: Cell::new(2),
        }
    }
}

const FRESH_BIT: usize = 0b100;
const INDEX_MASK: usize = 0b011;

#[cfg(test)]
mod tests {
    use super::Swap;

    #[test]
    fn smoke() {
        let s = Swap::new(0);
        unsafe {
            *s.pending() = 1;
            assert_eq!(*s.received(), 0);
            s.flush();
            assert_eq!(*s.received(), 0);
            assert!(s.refresh());
            assert_eq!(*s.received(), 1);
            assert!(!s.refresh());
            assert_eq!(*s.received(), 1);
            *s.pending() = 2;
            assert!(!s.refresh());
            assert_eq!(*s.received(), 1);
            s.flush();
            assert_eq!(*s.received(), 1);
            assert!(s.refresh());
            assert_eq!(*s.received(), 2);
        }
    }
}
