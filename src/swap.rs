use core::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::sync::Arc;

/// SPSC queue that only retains the last element sent
///
/// Useful for custom controllable signals.
pub fn swap<T: Send>(mut init: impl FnMut() -> T) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        slots: [
            UnsafeCell::new(init()),
            UnsafeCell::new(init()),
            UnsafeCell::new(init()),
        ],
        index: AtomicUsize::new(1),
    });
    (
        Sender {
            index: 0,
            shared: shared.clone(),
        },
        Receiver { index: 2, shared },
    )
}

pub struct Sender<T> {
    index: usize,
    shared: Arc<Shared<T>>,
}

impl<T> Sender<T> {
    /// Access the value that will be sent next
    pub fn pending(&mut self) -> &mut T {
        unsafe { &mut *self.shared.slots[self.index].get() }
    }

    /// Send the value from `pending`
    pub fn flush(&mut self) {
        self.index = self
            .shared
            .index
            .swap(self.index | FRESH_BIT, Ordering::AcqRel)
            & INDEX_MASK;
    }
}

pub struct Receiver<T> {
    index: usize,
    shared: Arc<Shared<T>>,
}

impl<T> Receiver<T> {
    /// Update the value exposed by `recv`. Returns whether new data was obtained. Consumer only.
    pub fn refresh(&mut self) -> bool {
        if self.shared.index.load(Ordering::Relaxed) & FRESH_BIT == 0 {
            return false;
        }
        self.index = self.shared.index.swap(self.index, Ordering::AcqRel) & INDEX_MASK;
        true
    }

    /// Access the most recent data as of the last `refresh` call. Consumer only.
    pub fn received(&mut self) -> &mut T {
        unsafe { &mut *self.shared.slots[self.index].get() }
    }
}

struct Shared<T> {
    slots: [UnsafeCell<T>; 3],
    index: AtomicUsize,
}

unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

const FRESH_BIT: usize = 0b100;
const INDEX_MASK: usize = 0b011;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let (mut s, mut r) = swap(|| 0);
        *s.pending() = 1;
        assert_eq!(*r.received(), 0);
        s.flush();
        assert_eq!(*r.received(), 0);
        assert!(r.refresh());
        assert_eq!(*r.received(), 1);
        assert!(!r.refresh());
        assert_eq!(*r.received(), 1);
        *s.pending() = 2;
        assert!(!r.refresh());
        assert_eq!(*r.received(), 1);
        s.flush();
        assert_eq!(*r.received(), 1);
        assert!(r.refresh());
        assert_eq!(*r.received(), 2);
    }
}
