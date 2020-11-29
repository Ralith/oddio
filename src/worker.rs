use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{spsc, Mix, Sample, Sampler, Source};

/// Build a remote/worker pair
pub fn worker() -> (Remote, Worker) {
    let (msg_send, msg_recv) = spsc::channel(INITIAL_CHANNEL_CAPACITY);
    let (free_send, free_recv) = spsc::channel(INITIAL_SOURCES_CAPACITY);
    let remote = Remote {
        sender: msg_send,
        free: free_recv,
        next_free: VecDeque::new(),
        old_senders: VecDeque::new(),
        source_capacity: INITIAL_SOURCES_CAPACITY,
        active_sources: 0,
    };
    let worker = Worker {
        recv: msg_recv,
        free: free_send,
        sources: SourceTable::with_capacity(remote.source_capacity),
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

/// Handle for controlling a [`Worker`] from another thread
pub struct Remote {
    sender: spsc::Sender<Msg>,
    free: spsc::Receiver<Free>,
    next_free: VecDeque<spsc::Receiver<Free>>,
    old_senders: VecDeque<spsc::Sender<Msg>>,
    source_capacity: usize,
    active_sources: usize,
}

impl Remote {
    /// Begin playing `source`, returning an ID that can be used to manipulate its playback
    ///
    /// Finished sources are automatically stopped, and their storage reused for future `play`
    /// calls.
    pub fn play<S>(&mut self, source: S) -> Handle<S>
    where
        S: Source + Send + 'static,
        S::Sampler: Sampler<S, Frame = [Sample; 2]>,
    {
        self.gc();
        let source = Arc::new(SourceData {
            stop: AtomicBool::new(false),
            source: UnsafeCell::new(source),
        });
        if self.active_sources == self.source_capacity {
            self.source_capacity *= 2;
            let sources = SourceTable::with_capacity(self.source_capacity);
            let (free_send, free_recv) = spsc::channel(self.source_capacity + 1); // save a slot for table free msg
            self.send(Msg::ReallocSources(sources, free_send));
            self.next_free.push_back(free_recv);
        }
        self.send(Msg::Play(source.clone()));
        self.active_sources += 1;
        Handle { inner: source }
    }

    /// Send a message to the worker thread, allocating more storage to do so if necessary
    fn send(&mut self, msg: Msg) {
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
                break;
            }
            // Run the inner loop again to guard against data added between the first run and the
            // channel becoming closed
            self.gc_inner();
            self.free = self
                .next_free
                .pop_back()
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

    /// Whether the source is no longer being played
    pub fn is_stopped(&self) -> bool {
        self.inner.stop.load(Ordering::Relaxed)
    }

    /// Access the source
    ///
    /// Because sources have interior mutability and are hence usually `!Sync`, this must be used to
    /// construct safe interfaces when access to shared state is required.
    pub fn get(&self) -> *mut T {
        self.inner.source.get()
    }
}

/// Writes output audio samples on demand
///
/// For real-time audio, this should be passed into the audio worker thread, e.g. the data callback
/// in cpal's `build_output_stream`.
pub struct Worker {
    recv: spsc::Receiver<Msg>,
    free: spsc::Sender<Free>,
    sources: SourceTable,
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
        for i in (0..self.sources.len()).rev() {
            let source = &self.sources[i];
            if source.stop.load(Ordering::Relaxed)
                || unsafe { (*source.source.get()).mix(sample_duration, samples) }
            {
                source.stop.store(true, Ordering::Relaxed);
                self.free
                    .send(Free::Source(self.sources.swap_remove(i)), 0)
                    .unwrap_or_else(|_| unreachable!("free queue has capacity for every source"));
            }
        }
    }

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
                Play(source) => {
                    assert!(
                        self.sources.len() < self.sources.capacity(),
                        "worker never does its own realloc"
                    );
                    self.sources.push(source);
                }
            }
        }
    }
}

unsafe impl Send for Worker {}

type SourceTable = Vec<ErasedSource>;

/// Type-erased internal reference to a source
type ErasedSource = Arc<SourceData<dyn Mix>>;

enum Msg {
    ReallocChannel(spsc::Receiver<Msg>),
    ReallocSources(SourceTable, spsc::Sender<Free>),
    Play(ErasedSource),
}

struct SourceData<S: ?Sized> {
    stop: AtomicBool,
    source: UnsafeCell<S>,
}

enum Free {
    Table(Vec<ErasedSource>),
    Source(ErasedSource),
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
        for i in 1..=(INITIAL_SOURCES_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
            worker.render(RATE, &mut []); // Process messages
            assert_eq!(worker.sources.len(), i);
        }
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mut worker) = worker();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(worker.sources.len(), 0);
        worker.render(RATE, &mut []); // Process messages
        assert_eq!(worker.sources.len(), INITIAL_CHANNEL_CAPACITY + 2);
    }
}
