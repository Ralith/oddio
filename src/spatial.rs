use std::{
    cell::RefCell,
    ops::{Index, IndexMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{
    handle::SignalData,
    math::{add, dot, invert_quat, mix, norm, rotate, scale, sub},
    ring::Ring,
    set::{set, Set, SetHandle},
    swap::Swap,
    Controlled, Filter, Handle, Sample, Signal,
};

/// Create a [`Signal`] for spatializing mono signals for stereo output
///
/// Samples its component signals at `rate`. Sampling more than `buffer_duration` seconds at once
/// may produce audible glitches when sounds exceed the `max_distance` they're constructed with. If
/// in doubt, 0.1 is a reasonable guess.
///
/// The scene can be controlled through [`SpatialSceneHandle`], and the resulting audio is produced by
/// the [`SpatialScene`] [`Signal`].
pub fn spatial(rate: u32, buffer_duration: f32) -> (SpatialSceneHandle, SpatialScene) {
    let (handle, set) = set();
    let rot = Arc::new(Swap::new(mint::Quaternion {
        s: 1.0,
        v: [0.0; 3].into(),
    }));
    (
        SpatialSceneHandle {
            set: handle,
            rot: rot.clone(),
            rate,
            buffer_duration,
        },
        SpatialScene(RefCell::new(Inner { rate, set, rot })),
    )
}

/// Handle for modifying a spatial scene
pub struct SpatialSceneHandle {
    set: SetHandle<ErasedSpatial>,
    rot: Arc<Swap<mint::Quaternion<f32>>>,
    rate: u32,
    buffer_duration: f32,
}

impl SpatialSceneHandle {
    /// Begin playing `signal` at `position`, moving at `velocity`, with accurate propagation delay
    /// out to `max_distance`
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    ///
    /// Returns a [`Handle`] that can be used to adjust the signal's movement in the future, and
    /// access other controls.
    pub fn play<S>(
        &mut self,
        signal: S,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        max_distance: f32,
    ) -> Handle<Spatial<S>>
    where
        S: Signal<Frame = Sample> + Send + 'static,
    {
        let signal = Spatial::new(
            self.rate,
            signal,
            position,
            velocity,
            max_distance / SPEED_OF_SOUND + self.buffer_duration,
        );
        let handle = Handle {
            shared: Arc::new(SignalData {
                stop: AtomicBool::new(false),
                signal,
            }),
        };
        self.set.insert(handle.shared.clone());
        handle
    }

    /// Set the listener's rotation
    ///
    /// An unrotated listener faces -Z, with +X to the right and +Y up.
    pub fn set_listener_rotation(&mut self, rotation: mint::Quaternion<f32>) {
        let signal_rotation = invert_quat(&rotation);
        unsafe {
            *self.rot.pending() = signal_rotation;
        }
        self.rot.flush();
    }
}

unsafe impl Send for SpatialSceneHandle {}
unsafe impl Sync for SpatialSceneHandle {}

type ErasedSpatial = Arc<SignalData<Spatial<dyn Signal<Frame = Sample> + Send>>>;

/// An individual spatialized signal
pub struct Spatial<T: ?Sized> {
    max_delay: f32,
    motion: Swap<Motion>,
    state: RefCell<State>,
    /// Delay queue of sound propagating through the medium
    ///
    /// Accounts only for the source's velocity. Listener velocity and attenuation are handled at
    /// output time.
    queue: RefCell<Ring>,
    inner: T,
}

impl<T> Spatial<T> {
    fn new(
        rate: u32,
        inner: T,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        max_delay: f32,
    ) -> Self {
        let mut queue = Ring::new((max_delay * rate as f32).ceil() as usize + 1);
        queue.delay(
            rate,
            (norm(position.into()) / SPEED_OF_SOUND).min(max_delay),
        );
        Self {
            max_delay,
            motion: Swap::new(Motion { position, velocity }),
            state: RefCell::new(State::new(position)),
            queue: RefCell::new(queue),
            inner,
        }
    }
}

impl<T> Spatial<T>
where
    T: ?Sized + Signal<Frame = Sample>,
{
    fn remaining(&self) -> f32 {
        let position = self
            .state
            .borrow()
            .smoothed_position(0.0, unsafe { &*self.motion.received() });
        let distance = norm(position.into());
        self.inner.remaining() + distance / SPEED_OF_SOUND
    }
}

impl<T> Filter for Spatial<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

/// Control for updating the motion of a spatial signal
pub struct SpatialControl<'a, T>(&'a Spatial<T>);

unsafe impl<'a, T: 'a> Controlled<'a> for Spatial<T> {
    type Control = SpatialControl<'a, T>;

    fn make_control(signal: &'a Spatial<T>) -> Self::Control {
        SpatialControl(signal)
    }
}

impl<'a, T> SpatialControl<'a, T> {
    /// Update the position and velocity of the signal
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    pub fn set_motion(&mut self, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) {
        unsafe {
            *self.0.motion.pending() = Motion { position, velocity };
        }
        self.0.motion.flush();
    }
}

/// [`Signal`] for stereo output from a spatial scene, created by [`spatial`]
pub struct SpatialScene(RefCell<Inner>);

unsafe impl Send for SpatialScene {}

struct Inner {
    rate: u32,
    set: Set<ErasedSpatial>,
    rot: Arc<Swap<mint::Quaternion<f32>>>,
}

impl Signal for SpatialScene {
    type Frame = [Sample; 2];

    fn sample(&self, interval: f32, out: &mut [[Sample; 2]]) {
        let this = &mut *self.0.borrow_mut();
        // Update set contents
        this.set.update();

        // Update listener rotation
        let (prev_rot, rot) = unsafe {
            let prev = *this.rot.received();
            this.rot.refresh();
            (prev, *this.rot.received())
        };

        // Zero output in preparation for mixing
        for frame in &mut out[..] {
            *frame = [0.0; 2];
        }

        let elapsed = interval * out.len() as f32;
        for i in (0..this.set.len()).rev() {
            let data = &this.set[i];
            // Discard finished sources
            if data.signal.remaining() < 0.0 {
                data.stop.store(true, Ordering::Relaxed);
            }
            if data.stop.load(Ordering::Relaxed) {
                this.set.remove(i);
                continue;
            }

            let spatial = &data.signal;

            debug_assert!(spatial.max_delay >= elapsed);

            // Extend delay queue with new data
            spatial
                .queue
                .borrow_mut()
                .write(&spatial.inner, this.rate, elapsed);

            // Compute the signal's smoothed start/end positions over the sampled period
            // TODO: Use historical positions
            let prev_position;
            let next_position;
            unsafe {
                let mut state = spatial.state.borrow_mut();

                // Update motion
                let orig_next = *spatial.motion.received();
                if spatial.motion.refresh() {
                    state.prev_position = state.smoothed_position(0.0, &orig_next);
                    state.dt = 0.0;
                } else {
                    debug_assert_eq!(orig_next.position, (*spatial.motion.received()).position);
                }

                prev_position = rotate(
                    &prev_rot,
                    &state.smoothed_position(0.0, &*spatial.motion.received()),
                );
                next_position = rotate(
                    &rot,
                    &state.smoothed_position(elapsed, &*spatial.motion.received()),
                );
            }

            // Mix into output
            for &ear in &[Ear::Left, Ear::Right] {
                let prev_state = EarState::new(prev_position, ear);
                let next_state = EarState::new(next_position, ear);

                // Clamp into the max length of the delay queue
                let prev_offset = (prev_state.offset - elapsed).max(-spatial.max_delay);
                let next_offset = next_state.offset.max(-spatial.max_delay);

                let dt = (next_offset - prev_offset) / out.len() as f32;
                let d_gain = (next_state.gain - prev_state.gain) / out.len() as f32;

                for (i, frame) in out.iter_mut().enumerate() {
                    let gain = prev_state.gain + i as f32 * d_gain;
                    let t = prev_offset + i as f32 * dt;
                    frame[ear as usize] += spatial.queue.borrow().sample(this.rate, t) * gain;
                }
            }

            // Set up for next time
            spatial.state.borrow_mut().dt += elapsed;
        }
    }

    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}

#[derive(Copy, Clone)]
struct Motion {
    position: mint::Point3<f32>,
    velocity: mint::Vector3<f32>,
}

struct State {
    /// Smoothed position estimate when position/vel were updated
    prev_position: mint::Point3<f32>,
    /// Seconds since position/vel were updated
    dt: f32,
}

impl State {
    fn new(position: mint::Point3<f32>) -> Self {
        Self {
            prev_position: position,
            dt: 0.0,
        }
    }

    fn smoothed_position(&self, dt: f32, next: &Motion) -> mint::Point3<f32> {
        let dt = self.dt + dt;
        let position_change = scale(next.velocity, dt);
        let naive_position = add(self.prev_position, position_change);
        let intended_position = add(next.position, position_change);
        mix(
            naive_position,
            intended_position,
            (dt / POSITION_SMOOTHING_PERIOD).min(1.0),
        )
    }
}

/// Seconds over which to smooth position discontinuities
///
/// Discontinuities arise because we only process commands at discrete intervals, and because the
/// caller probably isn't running at perfectly even intervals either. If smoothed over too short a
/// period, discontinuities will cause abrupt changes in effective velocity, which are distinctively
/// audible due to the doppler effect.
const POSITION_SMOOTHING_PERIOD: f32 = 0.5;

#[derive(Debug, Clone)]
struct EarState {
    /// Time offset at which this sound was most recently sampled
    offset: f32,
    /// Gain most recently applied
    gain: f32,
}

impl EarState {
    fn new(position_wrt_listener: mint::Point3<f32>, ear: Ear) -> Self {
        let distance = norm(sub(position_wrt_listener, ear.pos())).max(0.1);
        let offset = distance * (-1.0 / SPEED_OF_SOUND);
        let distance_gain = 1.0 / distance;
        let stereo_gain = 1.0
            + dot(
                ear.dir(),
                scale(position_wrt_listener.into(), 1.0 / distance),
            );
        Self {
            offset,
            gain: stereo_gain * distance_gain,
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Ear {
    Left,
    Right,
}

impl<T> Index<Ear> for [T] {
    type Output = T;
    fn index(&self, x: Ear) -> &T {
        &self[x as usize]
    }
}

impl<T> IndexMut<Ear> for [T] {
    fn index_mut(&mut self, x: Ear) -> &mut T {
        &mut self[x as usize]
    }
}

impl Ear {
    /// Location of the ear wrt a head facing -Z
    fn pos(self) -> mint::Point3<f32> {
        [
            match self {
                Ear::Left => -HEAD_RADIUS,
                Ear::Right => HEAD_RADIUS,
            },
            0.0,
            0.0,
        ]
        .into()
    }

    /// Unit vector along which sound is least attenuated
    fn dir(self) -> mint::Vector3<f32> {
        let x = 2.0f32.sqrt() / 2.0;
        [
            match self {
                Ear::Left => -x,
                Ear::Right => x,
            },
            0.0,
            -x,
        ]
        .into()
    }
}

/// Rate sound travels from signals to listeners (m/s)
const SPEED_OF_SOUND: f32 = 343.0;

/// Distance from center of head to an ear (m)
const HEAD_RADIUS: f32 = 0.1075;
