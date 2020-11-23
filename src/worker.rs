use std::{
    cell::UnsafeCell,
    mem,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use crate::{spsc, Action, Mix, Sample, Source};

/// Build a remote/worker pair
pub fn worker() -> (Remote, Worker) {
    let (send, recv) = spsc::channel(INITIAL_CHANNEL_CAPACITY);
    let sources = SourceTable::with_capacity(INITIAL_SOURCES_CAPACITY);
    let remote = Remote {
        sender: send,
        sources: sources.clone(),
        first_free: 0,
    };
    let worker = Worker {
        recv,
        sources,
        first_populated: usize::MAX,
        last_free: INITIAL_SOURCES_CAPACITY - 1,
    };
    (remote, worker)
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

/// Handle for controlling a `Worker` from another thread
pub struct Remote {
    sender: spsc::Sender<Msg>,
    sources: Arc<SourceTable>,
    first_free: usize,
}

impl Remote {
    /// Begin playing `source`, returning an ID that can be used to manipulate its playback
    pub fn play<S: Source<Frame = [Sample; 2]> + Send + 'static>(
        &mut self,
        source: S,
    ) -> Handle<S> {
        let source = Arc::new(SourceData {
            stop: AtomicBool::new(false),
            source: UnsafeCell::new(source),
        });
        let id = match self.alloc(source.clone()) {
            Ok(id) => id,
            Err(source) => {
                let sources = SourceTable::with_capacity(2 * self.sources.capacity());
                // Ensure we don't overwrite any slots that will be imported from the old table
                self.first_free = self.sources.capacity();
                self.sources = sources.clone();
                self.send(Msg::ReallocSources(sources));
                self.alloc(source)
                    .unwrap_or_else(|_| unreachable!("newly allocated nonzero-capacity buffer"))
            }
        };
        self.send(Msg::Play(id.index));
        Handle { inner: source }
    }

    fn send(&mut self, msg: Msg) {
        if let Err(msg) = self.sender.send(msg, 1) {
            // Channel would become full; allocate a new one
            let (mut send, recv) = spsc::channel(2 * self.sender.capacity() + 1);
            self.sender
                .send(Msg::ReallocChannel(recv), 0)
                .unwrap_or_else(|_| unreachable!("a space was reserved for this message"));
            send.send(msg, 0)
                .unwrap_or_else(|_| unreachable!("newly allocated nonzero-capacity queue"));
            self.sender = send;
        }
    }

    fn alloc(
        &mut self,
        source: Arc<SourceData<dyn Mix>>,
    ) -> Result<SourceId, Arc<SourceData<dyn Mix>>> {
        if self.first_free == usize::MAX {
            return Err(source);
        }
        let index = self.first_free;
        let slot = &self.sources.slots[index];
        // Acquire ordering ensures the next `next` value is read correctly
        self.first_free = slot.next.load(Ordering::Acquire);
        unsafe {
            (*slot.source.get()) = Some(source);
            Ok(SourceId {
                index: index as u32,
            })
        }
    }
}

unsafe impl Send for Remote {}

/// Handle to an active source
pub struct Handle<T> {
    inner: Arc<SourceData<T>>,
}

// Sound because `T` is not exposed by any safe interface
unsafe impl<T> Sync for Handle<T> {}

impl<T> Handle<T> {
    /// Stop playing the source, allowing it to be dropped on a future `play` invocation
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }

    /// Access the source
    ///
    /// Because sources have interior mutability and are hence usually `!Sync`, this must be used to
    /// construct safe interfaces when access to shared state is requird.
    pub fn get(&self) -> *mut T {
        self.inner.source.get()
    }
}

/// Lightweight handle for a source actively being played on a worker
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct SourceId {
    index: u32,
}

/// Writes output audio samples on demand
///
/// For real-time audio, this should be passed into the audio worker thread, e.g. the data callback
/// in cpal's `build_output_stream`.
pub struct Worker {
    recv: spsc::Receiver<Msg>,
    sources: Arc<SourceTable>,
    first_populated: usize,
    last_free: usize,
}

impl Worker {
    /// Write frames of stereo audio to `samples` for playback at `rate`
    ///
    /// Guaranteed to be wait-free, suitable for invocation on a real-time audio thread.
    ///
    /// Adds to the existing contents of `samples`. Be sure you zero it out first!
    pub fn render(&mut self, rate: u32, samples: &mut [[Sample; 2]]) {
        self.drain_msgs();

        let sample_duration = 1.0 / rate as f32;
        let mut i = self.first_populated;
        while i != usize::MAX {
            let slot = &self.sources.slots[i];
            let current = i;
            i = slot.next.load(Ordering::Relaxed); // Read next before we might clobber it in drop_source
            unsafe {
                let source = (*slot.source.get()).as_mut().unwrap();
                if source.stop.load(Ordering::Relaxed) {
                    self.sources.drop_source(
                        current,
                        &mut self.last_free,
                        &mut self.first_populated,
                    );
                } else {
                    match (*source.source.get()).mix(sample_duration, samples) {
                        Action::Retain => {}
                        Action::Drop => {
                            self.sources.drop_source(
                                current,
                                &mut self.last_free,
                                &mut self.first_populated,
                            );
                        }
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn source_count(&self) -> usize {
        let mut n = 0;
        let mut i = self.first_populated;
        while i != usize::MAX {
            n += 1;
            i = self.sources.slots[i].next.load(Ordering::Relaxed);
        }
        n
    }

    #[cfg(test)]
    fn free_count(&self) -> usize {
        let mut n = 0;
        let mut i = self.last_free;
        while i != usize::MAX {
            n += 1;
            unsafe {
                i = *self.sources.slots[i].prev.get();
            }
        }
        n
    }

    fn drain_msgs(&mut self) {
        self.recv.update();
        let iter = self.recv.drain();
        let mut new_channel = None;
        for msg in iter {
            use Msg::*;
            match msg {
                ReallocChannel(recv) => {
                    new_channel = Some(recv);
                }
                ReallocSources(sources) => {
                    // Move all existing slots into the new storage
                    unsafe {
                        self.sources
                            .slots
                            .as_ptr()
                            .cast::<Slot>()
                            .copy_to_nonoverlapping(
                                sources.slots.as_ptr().cast::<Slot>() as *mut _,
                                self.sources.slots.len(),
                            );
                        let old = mem::replace(&mut self.sources, sources);
                        for slot in &old.slots {
                            mem::forget((*slot.source.get()).take());
                        }
                    }
                    // Reconnect existing freed slots in the prefix to the tail of newly freed
                    // ones. We walk the old freelist backwards, appending to the new freelist as we
                    // go.
                    //
                    // The remote could fill up the available space again before this runs, but an
                    // extra realloc isn't the end of the world, particularly since the odds of a
                    // large proportion of existing slots being freed after the initial realloc
                    // began but before the worker learns about it is low.
                    let mut i = self.last_free;
                    // The last newly allocated slot is probably free. If not, this'll get fixed up
                    // when handling the corresponding Play message.
                    self.last_free = self.sources.slots.len() - 1;
                    // We know all Play messages affecting the pre-existing slots have already been
                    // processed, so we can rely the freelist being gracefully terminated.
                    while i != usize::MAX {
                        let prev_free = self.last_free;
                        self.last_free = i;
                        let slot = &self.sources.slots[i as usize];
                        self.sources.slots[prev_free]
                            .next
                            .store(i, Ordering::Relaxed);
                        unsafe {
                            i = *slot.prev.get();
                            slot.next.store(i, Ordering::Relaxed);
                            (*slot.prev.get()) = prev_free;
                        }
                    }
                }
                Play(index) => {
                    let index = index as usize;
                    let slot = &self.sources.slots[index];

                    // Remove from freelist
                    let next_free = slot.next.load(Ordering::Relaxed);
                    if next_free != usize::MAX {
                        unsafe {
                            (*self.sources.slots[next_free].prev.get()) = usize::MAX;
                        }
                    }
                    if index == self.last_free {
                        self.last_free = usize::MAX;
                    }

                    // Add to populated list
                    slot.next.store(self.first_populated, Ordering::Relaxed);
                    unsafe {
                        (*slot.prev.get()) = usize::MAX;
                    }
                    self.first_populated = index;
                }
            }
        }
        if let Some(recv) = new_channel {
            self.recv = recv;
        }
    }
}

unsafe impl Send for Worker {}

/// Storage arena for sound sources
///
/// Contains two intrusive lists: the free list, and the populated list. The freelist begins at
/// `Remote::first_free` and ends at `Worker::last_free`, while the populated list begins at
/// `Worker::first_populated`.
///
/// The `Remote` inserts data into slots in the freelist in order starting from `first_free`, and
/// sends `Msg::Play` messages to the `Worker` instructing it to move them into the populated
/// list. The `Worker` moves items from the populated list into the freelist at will.
struct SourceTable {
    slots: [Slot],
}

impl SourceTable {
    fn with_capacity(n: usize) -> Arc<Self> {
        let slots = (0..n)
            .map(|i| Slot {
                source: UnsafeCell::new(None),
                prev: UnsafeCell::new(i.checked_sub(1).unwrap_or(usize::MAX)),
                next: AtomicUsize::new(if i + 1 == n { usize::MAX } else { i + 1 }),
            })
            .collect::<Arc<[_]>>();
        unsafe { mem::transmute(slots) }
    }

    fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Worker only
    unsafe fn drop_source(&self, index: usize, last_free: &mut usize, first_populated: &mut usize) {
        let slot = &self.slots[index];

        // Remove from populated list
        let prev_val = *slot.prev.get();
        let next_val = slot.next.load(Ordering::Relaxed);
        if prev_val != usize::MAX {
            self.slots[prev_val]
                .next
                .store(slot.next.load(Ordering::Relaxed), Ordering::Relaxed);
        } else {
            debug_assert_eq!(index, *first_populated);
            *first_populated = next_val;
        }
        if next_val != usize::MAX {
            (*self.slots[next_val].prev.get()) = *slot.prev.get();
        }

        // Note that we leave `slot.source` populated here, because freeing it might hit a lock in
        // the global allocator, which could undermine our real-time guarantee. Instead, we let the
        // `Remote` drop the old value the next time the slot is reused.

        // Append to freelist
        slot.next.store(usize::MAX, Ordering::Relaxed);
        (*slot.prev.get()) = *last_free;
        // Release ordering ensures the prior write to `slot.next` above is visible.
        self.slots[*last_free].next.store(index, Ordering::Release);
        *last_free = index;
    }
}

struct Slot {
    /// When on the freelist, may be written by `Remote`. When populated, may be read by
    /// `Worker`. Other access is unsound!
    source: UnsafeCell<Option<Arc<SourceData<dyn Mix>>>>,
    /// Accessed by `worker` only.
    prev: UnsafeCell<usize>,
    /// When on the freelist, may be read by `Remote`. Written by `Worker` when moving between
    /// lists.
    next: AtomicUsize,
}

enum Msg {
    ReallocChannel(spsc::Receiver<Msg>),
    ReallocSources(Arc<SourceTable>),
    Play(u32),
}

struct SourceData<S: ?Sized> {
    stop: AtomicBool,
    source: UnsafeCell<S>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Samples, SamplesSource};

    const RATE: u32 = 10;

    #[test]
    fn realloc_sources() {
        let (mut remote, mut worker) = worker();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        assert_eq!(worker.source_count(), 0);
        for i in 1..=(INITIAL_SOURCES_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
            worker.render(RATE, &mut []); // Process messages
            assert_eq!(worker.source_count(), i);
        }
        assert_eq!(worker.free_count(), INITIAL_SOURCES_CAPACITY - 2);
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mut worker) = worker();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(worker.source_count(), 0);
        worker.render(RATE, &mut []); // Process first channel's worth of messages
        assert_eq!(worker.source_count(), INITIAL_CHANNEL_CAPACITY - 1); // One space taken by realloc message
        worker.render(RATE, &mut []); // Process remaining messages
        assert_eq!(worker.source_count(), INITIAL_CHANNEL_CAPACITY + 2);
    }

    #[test]
    fn reuse_slot() {
        let (mut remote, mut worker) = worker();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for _ in 0..INITIAL_SOURCES_CAPACITY {
            let handle = remote.play(source.clone().into_stereo());
            handle.stop();
            worker.render(RATE, &mut []); // Process messages
            assert_eq!(worker.source_count(), 0);
        }
        remote.play(source.clone().into_stereo());
        assert_eq!(remote.sources.slots.len(), INITIAL_SOURCES_CAPACITY);
        worker.render(RATE, &mut []); // Process messages
        assert_eq!(worker.source_count(), 1);
        assert_eq!(worker.free_count(), INITIAL_SOURCES_CAPACITY - 1);
        assert_eq!(worker.first_populated, 0);
    }
}
