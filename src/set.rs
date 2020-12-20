use std::{cell::UnsafeCell, collections::VecDeque, mem, ops::Deref};

use crate::spsc;

/// Build a set
pub fn set<T>() -> (SetHandle<T>, Set<T>) {
    let (msg_send, msg_recv) = spsc::channel(INITIAL_CHANNEL_CAPACITY);
    let (free_send, free_recv) = spsc::channel(INITIAL_SOURCES_CAPACITY);
    let remote = SetHandle {
        sender: msg_send,
        free: free_recv,
        next_free: VecDeque::new(),
        old_senders: VecDeque::new(),
        source_capacity: INITIAL_SOURCES_CAPACITY,
        active_sources: 0,
    };
    let mixer = Set(UnsafeCell::new(SetInner {
        recv: msg_recv,
        free: free_send,
        sources: SourceTable::with_capacity(remote.source_capacity),
    }));
    (remote, mixer)
}

#[cfg(not(miri))]
const INITIAL_CHANNEL_CAPACITY: usize = 127; // because the ring buffer wastes a slot
#[cfg(not(miri))]
const INITIAL_SOURCES_CAPACITY: usize = 128;

// Smaller versions for the sake of runtime
#[cfg(miri)]
const INITIAL_CHANNEL_CAPACITY: usize = 3;
#[cfg(miri)]
const INITIAL_SOURCES_CAPACITY: usize = 4;

/// Handle for adding sources to a [`Set`] from another thread
///
/// Constructed by calling [`set`].
pub struct SetHandle<T> {
    sender: spsc::Sender<Msg<T>>,
    free: spsc::Receiver<Free<T>>,
    next_free: VecDeque<spsc::Receiver<Free<T>>>,
    old_senders: VecDeque<spsc::Sender<Msg<T>>>,
    source_capacity: usize,
    active_sources: usize,
}

impl<T> SetHandle<T> {
    /// Add `source` to the set
    pub fn insert(&mut self, source: T) {
        self.gc();
        if self.active_sources == self.source_capacity {
            self.source_capacity *= 2;
            let sources = SourceTable::with_capacity(self.source_capacity);
            let (free_send, free_recv) = spsc::channel(self.source_capacity + 1); // save a slot for table free msg
            self.send(Msg::ReallocSources(sources, free_send));
            self.next_free.push_back(free_recv);
        }
        self.send(Msg::Insert(source));
        self.active_sources += 1;
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

    // Free old resources
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
                Free::Source(_) => {
                    self.active_sources -= 1;
                }
                Free::Table(x) => {
                    debug_assert_eq!(x.len(), 0, "sources were transferred to new table");
                }
            }
        }
    }
}

unsafe impl<T> Send for SetHandle<T> {}

/// A collection of heterogeneous [`Source`]s, controlled from another thread by a [`SetHandle`]
///
/// Constructed by calling [`set`]. A useful primitive for building aggregate [`Source`]s like
/// [`Mixer`](crate::Mixer).
pub struct Set<T>(UnsafeCell<SetInner<T>>);

struct SetInner<T> {
    recv: spsc::Receiver<Msg<T>>,
    free: spsc::Sender<Free<T>>,
    sources: SourceTable<T>,
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
                ReallocSources(sources, free) => {
                    // Move all existing slots into the new storage
                    let mut old = mem::replace(&mut self.sources, sources);
                    self.sources.extend(old.drain(..));
                    self.free = free;
                    self.free
                        .send(Free::Table(old), 0)
                        .unwrap_or_else(|_| unreachable!("fresh channel must have capacity"));
                }
                Insert(source) => {
                    assert!(
                        self.sources.len() < self.sources.capacity(),
                        "mixer never does its own realloc"
                    );
                    self.sources.push(source);
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
            .send(Free::Source(this.sources.swap_remove(index)), 0)
            .unwrap_or_else(|_| unreachable!("free queue has capacity for every source"));
    }
}

impl<T> Deref for Set<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        let this = unsafe { &mut (*self.0.get()) };
        &this.sources
    }
}

type SourceTable<T> = Vec<T>;

enum Msg<T> {
    ReallocChannel(spsc::Receiver<Msg<T>>),
    ReallocSources(SourceTable<T>, spsc::Sender<Free<T>>),
    Insert(T),
}

enum Free<T> {
    Table(Vec<T>),
    Source(T),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frames, FramesSource};

    const RATE: u32 = 10;

    #[test]
    fn realloc_sources() {
        let (mut remote, mut s) = set();
        let source = FramesSource::from(Frames::from_slice(RATE, &[[0.0; 2]; RATE as usize]));
        for i in 1..=(INITIAL_SOURCES_CAPACITY + 2) {
            remote.insert(source.clone());
            s.update();
            assert_eq!(unsafe { (*s.0.get()).sources.len() }, i);
        }
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mut s) = set();
        let source = FramesSource::from(Frames::from_slice(RATE, &[[0.0; 2]; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.insert(source.clone());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(unsafe { (*s.0.get()).sources.len() }, 0);
        s.update();
        assert_eq!(
            unsafe { (*s.0.get()).sources.len() },
            INITIAL_CHANNEL_CAPACITY + 2
        );
    }
}
