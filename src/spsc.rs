use crate::alloc::{alloc, sync::Arc};
use core::{
    cell::UnsafeCell,
    fmt, mem,
    mem::MaybeUninit,
    ops::Index,
    ptr, slice,
    sync::atomic::{AtomicUsize, Ordering},
};

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let shared = Shared::new(capacity + 1);
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared, len: 0 },
    )
}

pub struct Sender<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Sender<T> {
    /// Append a prefix of `data` to the channel
    ///
    /// Returns the number of items sent.
    pub fn send_from_slice(&mut self, data: &[T]) -> usize
    where
        T: Copy,
    {
        let write = self.shared.header.write.load(Ordering::Relaxed);
        let read = self.shared.header.read.load(Ordering::Relaxed);
        let size = self.shared.data.len();
        unsafe {
            let base = self.shared.data.as_ptr() as *mut T;
            let free = if write < read {
                (
                    slice::from_raw_parts_mut(base.add(write), read - write - 1),
                    &mut [][..],
                )
            } else if let Some(max) = read.checked_sub(1) {
                (
                    slice::from_raw_parts_mut(base.add(write), size - write),
                    slice::from_raw_parts_mut(base, max),
                )
            } else {
                (
                    slice::from_raw_parts_mut(base.add(write), size - write - 1),
                    &mut [][..],
                )
            };
            let n1 = free.0.len().min(data.len());
            ptr::copy_nonoverlapping(data.as_ptr(), free.0.as_mut_ptr(), n1);
            let mut n2 = 0;
            if let Some(rest) = data.len().checked_sub(n1) {
                n2 = free.1.len().min(rest);
                ptr::copy_nonoverlapping(data.as_ptr().add(n1), free.1.as_mut_ptr(), n2);
            }
            let n = n1 + n2;
            self.shared
                .header
                .write
                .store((write + n) % size, Ordering::Release);
            n
        }
    }

    pub fn capacity(&self) -> usize {
        self.shared.data.len() - 1
    }

    /// Append a single item, leaving at least `reserve_slots` for future use
    pub fn send(&mut self, data: T, reserve_slots: usize) -> Result<(), T> {
        let write = self.shared.header.write.load(Ordering::Relaxed);
        let read = self.shared.header.read.load(Ordering::Relaxed);
        let size = self.shared.data.len();
        if ((write + reserve_slots + 1) % size).wrapping_sub(read) < reserve_slots + 1 {
            return Err(data);
        }
        unsafe {
            *self.shared.data[write].get() = MaybeUninit::new(data);
        }
        self.shared
            .header
            .write
            .store((write + 1) % size, Ordering::Release);
        Ok(())
    }

    /// Whether the receiver has been dropped
    // Could be `&self` since we don't allow new references to be created, but :effort:
    pub fn is_closed(&mut self) -> bool {
        Arc::get_mut(&mut self.shared).is_some()
    }
}

pub struct Receiver<T> {
    shared: Arc<Shared<T>>,
    len: usize,
}

impl<T> Receiver<T> {
    /// Number of elements available to read
    pub fn len(&self) -> usize {
        self.len
    }

    /// Extend with newly sent items
    pub fn update(&mut self) {
        let old_len = self.len;
        self.len = self.shared.readable_len();
        debug_assert!(self.len >= old_len);
    }

    /// Release the first `n` elements for reuse by the sender
    pub fn release(&mut self, n: usize) {
        debug_assert!(n <= self.len);
        let n = self.len.min(n);
        unsafe {
            self.shared.release(n);
        }
        self.len -= n;
    }

    pub fn drain(&mut self) -> Drain<'_, T> {
        Drain { recv: self }
    }

    /// Whether the sender has been dropped
    // Could be `&self` since we don't allow new references to be created, but :effort:
    pub fn is_closed(&mut self) -> bool {
        Arc::get_mut(&mut self.shared).is_some()
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let read = self.shared.header.read.load(Ordering::Relaxed);
        let value = unsafe { (*self.shared.data[read].get()).as_ptr().read() };
        self.shared
            .header
            .read
            .store((read + 1) % self.shared.data.len(), Ordering::Relaxed);
        self.len -= 1;
        Some(value)
    }
}

impl<T> Index<usize> for Receiver<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        assert!(i < self.len);
        let read = self.shared.header.read.load(Ordering::Relaxed);
        unsafe { &*(*self.shared.data[(read + i) % self.shared.data.len()].get()).as_ptr() }
    }
}

impl<T: fmt::Debug> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut entries = f.debug_list();
        for i in 0..self.len() {
            entries.entry(&self[i]);
        }
        entries.finish()
    }
}

#[repr(C)]
struct Shared<T> {
    header: Header,
    data: [UnsafeCell<MaybeUninit<T>>],
}

unsafe impl<T: Send> Sync for Shared<T> {}

impl<T> Shared<T> {
    fn new(capacity: usize) -> Arc<Self> {
        let header_layout = alloc::Layout::new::<Header>();
        let layout = header_layout
            .extend(
                alloc::Layout::from_size_align(
                    mem::size_of::<T>() * capacity,
                    mem::align_of::<T>(),
                )
                .unwrap(),
            )
            .unwrap()
            .0
            .pad_to_align();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<Header>().write(Header {
                read: AtomicUsize::new(0),
                write: AtomicUsize::new(0),
            });
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, capacity) as *mut Self).into()
        }
    }

    fn readable_len(&self) -> usize {
        let read = self.header.read.load(Ordering::Relaxed);
        let write = self.header.write.load(Ordering::Acquire);
        if write >= read {
            write - read
        } else {
            write + self.data.len() - read
        }
    }

    unsafe fn release(&self, n: usize) {
        let read = self.header.read.load(Ordering::Relaxed);
        for i in 0..n {
            ptr::drop_in_place((*self.data[(read + i) % self.data.len()].get()).as_mut_ptr());
        }
        self.header
            .read
            .store((read + n) % self.data.len(), Ordering::Relaxed);
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        unsafe { self.release(self.readable_len()) }
    }
}

struct Header {
    read: AtomicUsize,
    write: AtomicUsize,
}

pub struct Drain<'a, T> {
    recv: &'a mut Receiver<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.recv.pop()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.recv.len, Some(self.recv.len))
    }
}

impl<'a, T> ExactSizeIterator for Drain<'a, T> {
    fn len(&self) -> usize {
        self.recv.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recv_empty() {
        let (_, mut recv) = channel::<u32>(4);
        recv.update();
        assert_eq!(recv.len(), 0);
    }

    #[test]
    fn send_recv() {
        let (mut send, mut recv) = channel::<u32>(4);
        send.send_from_slice(&[1, 2, 3]);
        recv.update();
        assert_eq!(recv.len(), 3);
        assert_eq!(recv[0], 1);
        assert_eq!(recv[1], 2);
        assert_eq!(recv[2], 3);
    }

    #[test]
    fn wrap() {
        let (mut send, mut recv) = channel::<u32>(4);
        send.send_from_slice(&[1, 2, 3]);
        recv.update();
        recv.release(2);
        assert_eq!(recv.len(), 1);
        assert_eq!(recv[0], 3);
        send.send_from_slice(&[4, 5]);
        recv.update();
        assert_eq!(recv.len(), 3);
        assert_eq!(recv[0], 3);
        assert_eq!(recv[1], 4);
        assert_eq!(recv[2], 5);
    }

    #[test]
    fn send_excess() {
        let (mut send, mut recv) = channel::<u32>(4);
        assert_eq!(send.send_from_slice(&[1, 2, 3, 4, 5]), 4);
        recv.update();
        assert_eq!(recv.len(), 4);
        assert_eq!(recv[0], 1);
        assert_eq!(recv[1], 2);
        assert_eq!(recv[2], 3);
        assert_eq!(recv[3], 4);
    }

    #[test]
    fn fill_release_fill() {
        let (mut send, mut recv) = channel::<u32>(4);
        assert_eq!(send.send_from_slice(&[1, 2, 3, 4]), 4);
        recv.update();
        recv.release(2);
        assert_eq!(send.send_from_slice(&[5, 6, 7]), 2);
        assert_eq!(send.send_from_slice(&[7]), 0);
    }
}
