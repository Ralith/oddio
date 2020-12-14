use std::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{AtomicUsize, Ordering},
};

/// SPSC queue that only retains the last element sent
///
/// Useful for custom controllable sources.
pub struct Swap<T> {
    slots: [Slot<T>; 3],
    generation: Cell<u64>,
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
            slots: [Slot::new(x.clone()), Slot::new(x.clone()), Slot::new(x)],
            generation: Cell::new(0),
            send: Cell::new(0),
            shared: AtomicUsize::new(1),
            recv: Cell::new(2),
        }
    }

    /// Access the value that will be sent next. Producer only.
    pub fn pending(&self) -> *mut T {
        self.slots[self.send.get()].value.get()
    }

    /// Send the value from `pending`. Producer only.
    pub fn flush(&self) {
        self.generation.set(self.generation.get() + 1);
        self.slots[self.send.get()]
            .generation
            .set(self.generation.get());
        self.send
            .set(self.shared.swap(self.send.get(), Ordering::Release));
    }

    /// Update the value exposed by `recv`. Returns whether new data was obtained. Consumer only.
    pub fn refresh(&self) -> bool {
        let generation = self.slots[self.recv.get()].generation.get();
        self.recv
            .set(self.shared.swap(self.recv.get(), Ordering::Acquire));
        let new_gen = self.slots[self.recv.get()].generation.get();
        if new_gen <= generation {
            // Outdated value, roll back
            self.recv
                .set(self.shared.swap(self.recv.get(), Ordering::Relaxed));
            let new_gen = self.slots[self.recv.get()].generation.get();
            debug_assert!(new_gen >= generation);
            new_gen > generation
        } else {
            true
        }
    }

    /// Access the most recent data as of the last `refresh` call. Consumer only.
    pub fn received(&self) -> *mut T {
        self.slots[self.recv.get()].value.get()
    }
}

struct Slot<T> {
    value: UnsafeCell<T>,
    generation: Cell<u64>,
}

impl<T> Slot<T> {
    fn new(x: T) -> Self {
        Self {
            value: UnsafeCell::new(x),
            generation: Cell::new(0),
        }
    }
}

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
