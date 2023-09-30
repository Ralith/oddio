use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use core::{
    cell::UnsafeCell,
    mem,
    ops::{Deref, DerefMut},
};

use crate::spsc;

/// Build a set
pub fn set<T>() -> (SetHandle<T>, Set<T>) {
    let (msg_send, msg_recv) = spsc::channel(INITIAL_CHANNEL_CAPACITY);
    let (free_send, free_recv) = spsc::channel(INITIAL_SIGNALS_CAPACITY);
    let remote = SetHandle {
        sender: msg_send,
        free: free_recv,
        next_free: VecDeque::new(),
        old_senders: VecDeque::new(),
        signal_capacity: INITIAL_SIGNALS_CAPACITY,
        active_signals: 0,
    };
    let mixer = Set(UnsafeCell::new(SetInner {
        recv: msg_recv,
        free: free_send,
        signals: SignalTable::with_capacity(remote.signal_capacity),
    }));
    (remote, mixer)
}

#[cfg(not(miri))]
const INITIAL_CHANNEL_CAPACITY: usize = 127; // because the ring buffer wastes a slot
#[cfg(not(miri))]
const INITIAL_SIGNALS_CAPACITY: usize = 128;

// Smaller versions for the sake of runtime
#[cfg(miri)]
const INITIAL_CHANNEL_CAPACITY: usize = 3;
#[cfg(miri)]
const INITIAL_SIGNALS_CAPACITY: usize = 4;

/// Handle for adding signals to a [`Set`] from another thread
///
/// Constructed by calling [`set`].
pub struct SetHandle<T> {
    sender: spsc::Sender<Msg<T>>,
    free: spsc::Receiver<Free<T>>,
    next_free: VecDeque<spsc::Receiver<Free<T>>>,
    old_senders: VecDeque<spsc::Sender<Msg<T>>>,
    signal_capacity: usize,
    active_signals: usize,
}

impl<T> SetHandle<T> {
    /// Add `signal` to the set
    pub fn insert(&mut self, signal: T) {
        self.gc();
        if self.active_signals == self.signal_capacity {
            self.signal_capacity *= 2;
            let signals = SignalTable::with_capacity(self.signal_capacity);
            let (free_send, free_recv) = spsc::channel(self.signal_capacity + 1); // save a slot for table free msg
            self.send(Msg::ReallocSignals(signals, free_send));
            self.next_free.push_back(free_recv);
        }
        self.send(Msg::Insert(signal));
        self.active_signals += 1;
    }

    /// Send a message, allocating more storage to do so if necessary
    fn send(&mut self, msg: Msg<T>) {
        if let Err(msg) = self.sender.send(msg, 1) {
            // Channel would become full; allocate a new one
            let (mut send, recv) = spsc::channel(2 * self.sender.capacity() + 1);
            self.sender
                .send(Msg::ReallocChannel(recv), 0)
                .unwrap_or_else(|_| unreachable!("a space was reserved for this message"));
            send.send(msg, 0)
                .unwrap_or_else(|_| unreachable!("newly allocated nonzero-capacity queue"));
            let old = mem::replace(&mut self.sender, send);
            self.old_senders.push_back(old);
        }
    }

    // Free old signals
    fn gc(&mut self) {
        while self
            .old_senders
            .front_mut()
            .map_or(false, |x| x.is_closed())
        {
            self.old_senders.pop_front();
        }
        loop {
            self.gc_inner();
            if !self.free.is_closed() || self.sender.is_closed() {
                // If the free queue isn't closed, it may get more data in the future. If the
                // message queue is closed, then the mixer's gone and none of this
                // matters. Otherwise, we must be switching to a new free queue.
                break;
            }
            // Drain the queue again to guard against data added between the first run and the
            // channel becoming closed
            self.gc_inner();
            self.free = self
                .next_free
                .pop_front()
                .expect("free channel closed without replacement");
        }
    }

    fn gc_inner(&mut self) {
        self.free.update();
        for x in self.free.drain() {
            match x {
                Free::Signal(_) => {
                    self.active_signals -= 1;
                }
                Free::Table(x) => {
                    debug_assert_eq!(x.len(), 0, "signals were transferred to new table");
                }
            }
        }
    }
}

unsafe impl<T> Send for SetHandle<T> {}
unsafe impl<T> Sync for SetHandle<T> {}

/// A collection of heterogeneous [`Signal`]s, controlled from another thread by a [`SetHandle`]
///
/// Constructed by calling [`set`]. A useful primitive for building aggregate [`Signal`]s like
/// [`Mixer`](crate::Mixer).
pub struct Set<T>(UnsafeCell<SetInner<T>>);

struct SetInner<T> {
    recv: spsc::Receiver<Msg<T>>,
    free: spsc::Sender<Free<T>>,
    signals: SignalTable<T>,
}

impl<T> SetInner<T> {
    fn drain_msgs(&mut self) {
        self.recv.update();
        while let Some(msg) = self.recv.pop() {
            use Msg::*;
            match msg {
                ReallocChannel(recv) => {
                    self.recv = recv;
                    self.recv.update();
                }
                ReallocSignals(signals, free) => {
                    // Move all existing slots into the new storage
                    let mut old = mem::replace(&mut self.signals, signals);
                    self.signals.append(&mut old);
                    self.free = free;
                    self.free
                        .send(Free::Table(old), 0)
                        .unwrap_or_else(|_| unreachable!("fresh channel must have capacity"));
                }
                Insert(signal) => {
                    assert!(
                        self.signals.len() < self.signals.capacity(),
                        "mixer never does its own realloc"
                    );
                    self.signals.push(signal);
                }
            }
        }
    }
}

unsafe impl<T> Send for Set<T> {}

impl<T> Set<T> {
    /// Process changes to the set
    pub fn update(&mut self) {
        let this = unsafe { &mut (*self.0.get()) };
        this.drain_msgs();
    }

    /// Remove `index` from the set
    ///
    /// The last element in the set replaces it, as in `Vec::swap_remove`.
    pub fn remove(&mut self, index: usize) {
        let this = unsafe { &mut (*self.0.get()) };
        this.free
            .send(Free::Signal(this.signals.swap_remove(index)), 0)
            .unwrap_or_else(|_| unreachable!("free queue has capacity for every signal"));
    }
}

impl<T> Deref for Set<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        let this = unsafe { &mut (*self.0.get()) };
        &this.signals
    }
}

impl<T> DerefMut for Set<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        let this = unsafe { &mut (*self.0.get()) };
        &mut this.signals
    }
}

type SignalTable<T> = Vec<T>;

enum Msg<T> {
    ReallocChannel(spsc::Receiver<Msg<T>>),
    ReallocSignals(SignalTable<T>, spsc::Sender<Free<T>>),
    Insert(T),
}

enum Free<T> {
    Table(Vec<T>),
    Signal(T),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frames, FramesSignal};

    const RATE: u32 = 10;

    #[test]
    fn realloc_signals() {
        let (mut remote, mut s) = set();
        let signal = FramesSignal::from(Frames::from_slice(RATE, &[[0.0; 2]; RATE as usize]));
        for i in 1..=(INITIAL_SIGNALS_CAPACITY + 2) {
            remote.insert(signal.clone());
            s.update();
            assert_eq!(unsafe { (*s.0.get()).signals.len() }, i);
        }
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mut s) = set();
        let signal = FramesSignal::from(Frames::from_slice(RATE, &[[0.0; 2]; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.insert(signal.clone());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(unsafe { (*s.0.get()).signals.len() }, 0);
        s.update();
        assert_eq!(
            unsafe { (*s.0.get()).signals.len() },
            INITIAL_CHANNEL_CAPACITY + 2
        );
    }
}
