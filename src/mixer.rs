use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    mem,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{spsc, Sample, Source, StridedMut};

/// Build a remote/mixer pair
pub fn mixer() -> (Remote, Mixer) {
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
    let mixer = Mixer(UnsafeCell::new(MixerInner {
        recv: msg_recv,
        free: free_send,
        sources: SourceTable::with_capacity(remote.source_capacity),
        buffer: vec![[0.0; 2]; 1024].into(),
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

/// Handle for controlling a [`Mixer`] from another thread
pub struct Remote {
    sender: spsc::Sender<Msg>,
    free: spsc::Receiver<Free>,
    next_free: VecDeque<spsc::Receiver<Free>>,
    old_senders: VecDeque<spsc::Sender<Msg>>,
    source_capacity: usize,
    active_sources: usize,
}

impl Remote {
    /// Begin playing `source`, returning a handle controlling its playback
    ///
    /// Finished sources are automatically stopped, and their storage reused for future `play`
    /// calls.
    pub fn play<S>(&mut self, source: S) -> Control<S>
    where
        S: Source<Frame = [Sample; 2]> + Send + 'static,
    {
        self.gc();
        let source = Arc::new(SourceData {
            stop: AtomicBool::new(false),
            source,
        });
        if self.active_sources == self.source_capacity {
            self.source_capacity *= 2;
            let sources = SourceTable::with_capacity(self.source_capacity);
            let (free_send, free_recv) = spsc::channel(self.source_capacity + 1); // save a slot for table free msg
            self.send(Msg::ReallocSources(sources, free_send));
            self.next_free.push_back(free_recv);
        }
        self.send(Msg::Play(Output {
            inner: source.clone(),
        }));
        self.active_sources += 1;
        Control { inner: source }
    }

    /// Send a message to the mixer, allocating more storage to do so if necessary
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

/// Handle for manipulating a source while it plays
pub struct Control<T: ?Sized> {
    inner: Arc<SourceData<T>>,
}

// Sound because `T` is not accessible through any safe interface unless `T: Sync`
unsafe impl<T> Send for Control<T> {}

impl<T> Control<T> {
    /// Stop playing the source, allowing it to be dropped on a future `play` invocation
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the source is no longer being played
    pub fn is_stopped(&self) -> bool {
        self.inner.stop.load(Ordering::Relaxed)
    }

    /// Access a potentially `!Sync` source
    ///
    /// Building block for safe abstractions over nontrivial shared memory.
    pub fn get(&self) -> *const T {
        &self.inner.source
    }
}

impl<T: Sync> AsRef<T> for Control<T> {
    fn as_ref(&self) -> &T {
        &self.inner.source
    }
}

/// Type-erased handle for playing a source
struct Output<T> {
    inner: Arc<SourceData<dyn Source<Frame = T> + Send>>,
}

impl<T> Output<T> {
    /// Stop playing the source, allowing it to be dropped on a future `play` invocation
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the source is no longer being played
    pub fn is_stopped(&self) -> bool {
        self.inner.stop.load(Ordering::Relaxed)
    }
}

impl<T> Deref for Output<T> {
    type Target = dyn Source<Frame = T>;

    fn deref(&self) -> &(dyn Source<Frame = T> + 'static) {
        &self.inner.source
    }
}

/// A [`Source`] that mixes a dynamic set of [`Source`]s, controlled by a [`Remote`]
pub struct Mixer(UnsafeCell<MixerInner>);

struct MixerInner {
    recv: spsc::Receiver<Msg>,
    free: spsc::Sender<Free>,
    sources: SourceTable,
    // Temporary storage for inner source data before mixing
    buffer: Box<[[Sample; 2]]>,
}

impl MixerInner {
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
                        "mixer never does its own realloc"
                    );
                    self.sources.push(source);
                }
            }
        }
    }
}

unsafe impl Send for Mixer {}

impl Source for Mixer {
    type Frame = [Sample; 2];

    fn sample(&self, offset: f32, sample_duration: f32, mut out: StridedMut<'_, Self::Frame>) {
        let this = unsafe { &mut *self.0.get() }; // Sound because `Self: !Sync`
        this.drain_msgs();

        for o in &mut out {
            *o = [0.0; 2];
        }

        for i in (0..this.sources.len()).rev() {
            let source = &this.sources[i];
            if source.remaining() < 0.0 {
                source.stop();
            }
            if source.is_stopped() {
                this.free
                    .send(Free::Source(this.sources.swap_remove(i)), 0)
                    .unwrap_or_else(|_| unreachable!("free queue has capacity for every source"));
                continue;
            }

            // Sample into `buffer`, then mix into `out`
            let mut iter = out.iter_mut();
            let mut i = 0;
            while iter.len() > 0 {
                let n = iter.len().min(this.buffer.len());
                let staging = &mut this.buffer[..n];
                source.sample(
                    offset + i as f32 * sample_duration,
                    sample_duration,
                    staging.into(),
                );
                for (staged, o) in staging.iter().zip(&mut iter) {
                    o[0] += staged[0];
                    o[1] += staged[1];
                }
                i += n;
            }
        }
    }

    fn advance(&self, dt: f32) {
        let this = unsafe { &mut *self.0.get() };
        for source in &this.sources {
            source.advance(dt);
        }
    }

    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}

type SourceTable = Vec<Output<[Sample; 2]>>;

enum Msg {
    ReallocChannel(spsc::Receiver<Msg>),
    ReallocSources(SourceTable, spsc::Sender<Free>),
    Play(Output<[Sample; 2]>),
}

/// State shared between [`Control`] and [`Output`]
struct SourceData<S: ?Sized> {
    stop: AtomicBool,
    source: S,
}

enum Free {
    Table(Vec<Output<[Sample; 2]>>),
    Source(Output<[Sample; 2]>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Samples, SamplesSource};

    const RATE: u32 = 10;

    #[test]
    fn realloc_sources() {
        let (mut remote, mixer) = mixer();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for i in 1..=(INITIAL_SOURCES_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
            mixer.sample(0.0, 1.0, StridedMut::default()); // Process messages
            assert_eq!(unsafe { (*mixer.0.get()).sources.len() }, i);
        }
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mixer) = mixer();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.play(source.clone().into_stereo());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(unsafe { (*mixer.0.get()).sources.len() }, 0);
        mixer.sample(0.0, 1.0, StridedMut::default()); // Process messages
        assert_eq!(
            unsafe { (*mixer.0.get()).sources.len() },
            INITIAL_CHANNEL_CAPACITY + 2
        );
    }
}
