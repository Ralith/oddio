use std::{
    cell::UnsafeCell,
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::{
    math::{add, mix, scale},
    mixer, spsc, Sample, Source,
};

/// Begin building an audio worker
pub fn worker() -> Builder {
    Builder::default()
}

/// Configuration that audio workers are built from
#[must_use]
#[derive(Debug, Clone)]
pub struct Builder {
    max_delay: f32,
}

impl Builder {
    /// Sources are dropped when they ended more than `delay` ago
    ///
    /// If this is set too low, distance listeners may hear sources appear to cut off early due to
    /// sound travel time. Good settings are proportional to the square root of the maximum source
    /// amplitude.
    pub fn max_delay(&mut self, delay: Duration) -> &mut Self {
        self.max_delay = delay.as_secs_f32();
        self
    }

    /// Construct a remote control and the worker it controls from this configuration
    #[must_use]
    pub fn build(&self) -> (Remote, Worker) {
        let (send, recv) = spsc::channel(INITIAL_CHANNEL_CAPACITY);
        let sources = SourceTable::with_capacity(INITIAL_SOURCES_CAPACITY);
        let remote = Remote {
            sender: send,
            sources: sources.clone(),
            first_free: 0,
        };
        let worker = Worker {
            max_delay: self.max_delay,
            recv,
            sources,
            first_populated: usize::MAX,
            last_free: INITIAL_SOURCES_CAPACITY - 1,
        };
        (remote, worker)
    }
}

const INITIAL_CHANNEL_CAPACITY: usize = 127; // because the ring buffer wastes a slot
const INITIAL_SOURCES_CAPACITY: usize = 128;

impl Default for Builder {
    fn default() -> Self {
        Builder { max_delay: 4.0 }
    }
}

/// Handle for controlling a `Worker` from another thread
pub struct Remote {
    sender: spsc::Sender<Msg>,
    sources: Arc<SourceTable>,
    first_free: usize,
}

impl Remote {
    /// Begin playing `source`, returning an ID that can be used to manipulate its playback
    pub fn play<S: Source + 'static>(
        &mut self,
        mut source: S,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
    ) -> SourceId {
        let data = SourceData {
            mix: Box::new(move |mixer, state, pos| process_source(&mut source, mixer, state, pos)),
            state: mixer::State::new(position),
            ref_position: position,
            velocity,
            dt: 0.0,
            prev_position: position,
        };
        let id = match self.alloc(data) {
            Ok(id) => id,
            Err(data) => {
                let sources = SourceTable::with_capacity(2 * self.sources.capacity());
                // Ensure we don't overwrite any slots that will be imported from the old table
                self.first_free = self.sources.capacity();
                self.sources = sources.clone();
                self.send(Msg::ReallocSources(sources));
                self.alloc(data).unwrap_or_else(|_| unreachable!())
            }
        };
        self.send(Msg::Play(id.index));
        id
    }

    /// Stop playing `source` and discard it
    pub fn stop(&mut self, source: SourceId) {
        self.send(Msg::Stop(source));
    }

    /// Update the position and velocity of `source`
    ///
    /// Large discontinuities in position imply high velocities, which can lead to interesting
    /// doppler effects even if the explicit velocities are small.
    pub fn set_motion(
        &mut self,
        source: SourceId,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
    ) {
        self.send(Msg::SetMotion(source, position, velocity));
    }

    fn send(&mut self, msg: Msg) {
        if let Err(msg) = self.sender.send(msg, 1) {
            // Channel would become full; allocate a new one
            let (mut send, recv) = spsc::channel(2 * self.sender.capacity() + 1);
            self.sender
                .send(Msg::ReallocChannel(recv), 0)
                .unwrap_or_else(|_| unreachable!());
            send.send(msg, 0).unwrap_or_else(|_| unreachable!());
            self.sender = send;
        }
    }

    fn alloc(&mut self, source: SourceData) -> Result<SourceId, SourceData> {
        if self.first_free == usize::MAX {
            return Err(source);
        }
        let index = self.first_free;
        let slot = unsafe { &mut *self.sources.slots[index].get() };
        // Acquire ordering ensures we don't read the next free slot's generation until it's
        // updated.
        self.first_free = slot.next.load(Ordering::Acquire);
        slot.source = Some(source);
        Ok(SourceId {
            index: index as u32,
            generation: slot.generation,
        })
    }
}

unsafe impl Send for Remote {}

/// Lightweight handle for a source actively being played on a worker
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct SourceId {
    index: u32,
    generation: u32,
}

/// Writes output audio samples on demand
///
/// For real-time audio, this should be passed into the audio worker thread, e.g. the data callback
/// in cpal's `build_output_stream`.
pub struct Worker {
    max_delay: f32,
    recv: spsc::Receiver<Msg>,
    sources: Arc<SourceTable>,
    first_populated: usize,
    last_free: usize,
}

impl Worker {
    /// Write frames of stereo audio to `samples` for playback at `rate`
    ///
    /// Adds to the existing contents of `samples`. Be sure you zero it out first!
    pub fn render(&mut self, rate: u32, samples: &mut [[Sample; 2]]) {
        self.drain_msgs();

        let dt = samples.len() as f32 / rate as f32;
        let mut mixer = mixer::Mixer::new(rate, samples);
        let mut i = self.first_populated;
        while i != usize::MAX {
            let slot = unsafe { &mut *self.sources.slots[i].get() };
            let source = slot.source.as_mut().unwrap();

            source.dt += dt;
            let position = source.smoothed_position();
            let remaining = (source.mix)(&mut mixer, &mut source.state, position);
            if remaining < -self.max_delay {
                unsafe {
                    self.sources
                        .drop_source(i, &mut self.last_free, &mut self.first_populated);
                }
            }

            i = slot.next.load(Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    fn source_count(&self) -> usize {
        let mut n = 0;
        let mut i = self.first_populated;
        while i != usize::MAX {
            let slot = unsafe { &mut *self.sources.slots[i].get() };
            n += 1;
            i = slot.next.load(Ordering::Relaxed);
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
                    }
                    mem::forget(mem::replace(&mut self.sources, sources));
                    // Reconnect existing freed slots in the prefix to the tail of newly freed
                    // ones. We walk the old freelist backwards, appending to the new freelist as we
                    // go.
                    //
                    // The remote could fill up the available space again before this runs, but an
                    // extra realloc isn't the end of the world, particularly since the odds of a
                    // large proportion of existing slots being freed after the initial realloc
                    // began but before the worker learns about it is low.
                    let mut i = self.last_free;
                    self.last_free = self.sources.slots.len() - 1;
                    while i != usize::MAX {
                        let slot = unsafe { &mut *self.sources.slots[i as usize].get() };
                        // If we hit an allocated slot, we've reached the end of the freelist. It's
                        // sound to access the source of freed slots in the old range because the
                        // remote is guaranteed not to be writing into them until we wire them back
                        // up.
                        if slot.source.is_some() {
                            break;
                        }
                        unsafe {
                            (*self.sources.slots[self.last_free].get())
                                .next
                                .store(i, Ordering::Relaxed);
                        }
                        i = slot.prev;
                        slot.next.store(slot.prev, Ordering::Relaxed);
                        slot.prev = self.last_free;
                    }
                }
                Play(index) => {
                    let slot = unsafe { &mut *self.sources.slots[index as usize].get() };
                    slot.next.store(self.first_populated, Ordering::Relaxed);
                    self.first_populated = index as usize;
                }
                Stop(id) => unsafe {
                    if self.sources.try_get(id).is_some() {
                        self.sources.drop_source(
                            id.index as usize,
                            &mut self.last_free,
                            &mut self.first_populated,
                        );
                    }
                },
                SetMotion(id, pos, vel) => {
                    if let Some(slot) = unsafe { self.sources.try_get(id) } {
                        let source = slot.source.as_mut().unwrap();
                        source.prev_position = source.smoothed_position();
                        source.ref_position = pos;
                        source.velocity = vel;
                        source.dt = 0.0;
                    }
                }
            }
        }
        if let Some(recv) = new_channel {
            self.recv = recv;
        }
    }
}

unsafe impl Send for Worker {}

struct SourceTable {
    slots: [UnsafeCell<Slot>],
}

impl SourceTable {
    fn with_capacity(n: usize) -> Arc<Self> {
        let slots = (0..n)
            .map(|i| {
                UnsafeCell::new(Slot {
                    source: None,
                    prev: i.checked_sub(1).unwrap_or(usize::MAX),
                    next: AtomicUsize::new(if i + 1 == n { usize::MAX } else { i + 1 }),
                    generation: 0,
                })
            })
            .collect::<Arc<[_]>>();
        unsafe { mem::transmute(slots) }
    }

    fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Worker only
    unsafe fn drop_source(&self, index: usize, last_free: &mut usize, first_populated: &mut usize) {
        let slot = &mut *self.slots[index].get();

        // Update external references
        if slot.prev != usize::MAX {
            let prev = &mut *self.slots[slot.prev].get();
            // Release ordering ensures the write to slot.generation above is visible.
            prev.next
                .store(slot.next.load(Ordering::Relaxed), Ordering::Release);
        }
        let next_val = slot.next.load(Ordering::Relaxed);
        if next_val != usize::MAX {
            let next = &mut *self.slots[next_val].get();
            next.prev = slot.prev;
        }
        if index == *first_populated {
            *first_populated = next_val;
        }

        // Update own fields
        slot.source = None;
        slot.generation = slot.generation.wrapping_add(1);
        slot.next.store(usize::MAX, Ordering::Relaxed);
        slot.prev = *last_free;
        (*self.slots[*last_free].get())
            .next
            .store(index, Ordering::Relaxed);
        *last_free = index;
    }

    unsafe fn try_get(&self, id: SourceId) -> Option<&mut Slot> {
        let slot = &mut *self.slots[id.index as usize].get();
        if slot.generation != id.generation {
            return None;
        }
        Some(slot)
    }
}

struct Slot {
    source: Option<SourceData>,
    prev: usize,
    next: AtomicUsize,
    generation: u32,
}

struct SourceData {
    /// Invokes `process_source` and returns seconds remaining
    ///
    /// We could use a Box<dyn Source> instead, but by encapsulating all use of the `Source` trait
    /// we can reduce the number of virtual calls to one per source.
    mix: Box<dyn FnMut(&mut mixer::Mixer, &mut mixer::State, mint::Point3<f32>) -> f32>,
    state: mixer::State,
    /// Latest explicitly set position
    ref_position: mint::Point3<f32>,
    /// Latest explicitly set velocity
    velocity: mint::Vector3<f32>,
    /// Seconds since ref_position was set
    dt: f32,
    /// Smoothed position estimate when ref_position was set
    prev_position: mint::Point3<f32>,
}

impl SourceData {
    fn smoothed_position(&self) -> mint::Point3<f32> {
        let position_change = scale(self.velocity, self.dt);
        let naive_position = add(self.prev_position, position_change);
        let intended_position = add(self.ref_position, position_change);
        mix(
            naive_position,
            intended_position,
            (self.dt / POSITION_SMOOTHING_PERIOD).min(1.0),
        )
    }
}

enum Msg {
    ReallocChannel(spsc::Receiver<Msg>),
    ReallocSources(Arc<SourceTable>),
    Play(u32),
    Stop(SourceId),
    SetMotion(SourceId, mint::Point3<f32>, mint::Vector3<f32>),
}

fn process_source<S: Source>(
    source: &mut S,
    mixer: &mut mixer::Mixer,
    state: &mut mixer::State,
    next_pos: mint::Point3<f32>,
) -> f32 {
    mixer.mix(mixer::Input {
        source,
        state,
        position_wrt_listener: next_pos,
    });
    source.advance(mixer.samples.len() as f32 * (source.rate() as f32 / mixer.rate as f32));
    source.remaining() / source.rate() as f32
}

/// Seconds over which to smooth position discontinuities
///
/// Discontinuities arise because we only process commands at discrete intervals, and because the
/// caller probably isn't running at perfectly even intervals either. If smoothed over too short a
/// period, these will cause abrupt changes in velocity, which are distinctively audible due to the
/// doppler effect.
const POSITION_SMOOTHING_PERIOD: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Samples, SamplesSource};

    const RATE: u32 = 10;

    #[test]
    fn drop_finished() {
        let (mut remote, mut worker) = worker().max_delay(Duration::from_secs(1)).build();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        assert_eq!(worker.source_count(), 0);
        remote.play(source, [0.0; 3].into(), [0.0; 3].into());
        worker.render(RATE, &mut [[0.0; 2]; RATE as usize]); // 0-9
        assert_eq!(worker.source_count(), 1);
        worker.render(RATE, &mut [[0.0; 2]; RATE as usize]); // 10-19
        assert_eq!(worker.source_count(), 1);
        worker.render(RATE, &mut [[0.0; 2]; RATE as usize]); // 20-29
        assert_eq!(worker.source_count(), 0);
    }

    #[test]
    fn realloc_sources() {
        let (mut remote, mut worker) = worker().max_delay(Duration::from_secs(1)).build();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        assert_eq!(worker.source_count(), 0);
        for i in 1..=(INITIAL_SOURCES_CAPACITY + 2) {
            remote.play(source.clone(), [0.0; 3].into(), [0.0; 3].into());
            worker.render(RATE, &mut []); // Process messages
            assert_eq!(worker.source_count(), i);
        }
    }

    #[test]
    fn realloc_channel() {
        let (mut remote, mut worker) = worker().max_delay(Duration::from_secs(1)).build();
        let source = SamplesSource::from(Samples::from_slice(RATE, &[0.0; RATE as usize]));
        for _ in 0..(INITIAL_CHANNEL_CAPACITY + 2) {
            remote.play(source.clone(), [0.0; 3].into(), [0.0; 3].into());
        }
        assert_eq!(remote.sender.capacity(), 1 + 2 * INITIAL_CHANNEL_CAPACITY);
        assert_eq!(worker.source_count(), 0);
        worker.render(RATE, &mut []); // Process first channel's worth of messages
        assert_eq!(worker.source_count(), INITIAL_CHANNEL_CAPACITY - 1); // One space taken by realloc message
        worker.render(RATE, &mut []); // Process remaining messages
        assert_eq!(worker.source_count(), INITIAL_CHANNEL_CAPACITY + 2);
    }
}
