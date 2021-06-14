use std::{
    cell::RefCell,
    ops::{Index, IndexMut},
    sync::Arc,
};

use crate::{
    math::{add, dot, invert_quat, mix, norm, rotate, scale, sub},
    ring::Ring,
    set::{set, Set, SetHandle},
    swap::Swap,
    Controlled, Filter, Handle, Sample, Signal, Stop,
};

type ErasedSpatial = Arc<Spatial<Stop<dyn Signal<Frame = Sample> + Send>>>;

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

impl<T: ?Sized> Filter for Spatial<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

/// Control for updating the motion of a spatial signal
pub struct SpatialControl<'a>(&'a Swap<Motion>);

unsafe impl<'a, T: 'a> Controlled<'a> for Spatial<T> {
    type Control = SpatialControl<'a>;

    unsafe fn make_control(signal: &'a Spatial<T>) -> Self::Control {
        SpatialControl(&signal.motion)
    }
}

impl<'a> SpatialControl<'a> {
    /// Update the position and velocity of the signal
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    ///
    /// If your sounds seem to be lagging behind their intended position by about half a second,
    /// make sure you're providing an accurate `velocity`!
    pub fn set_motion(&mut self, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) {
        unsafe {
            *self.0.pending() = Motion { position, velocity };
        }
        self.0.flush();
    }
}

/// [`Signal`] for stereo output from a spatial scene
pub struct SpatialScene {
    rate: u32,
    buffer_duration: f32,
    send: RefCell<SetHandle<ErasedSpatial>>,
    rot: Swap<mint::Quaternion<f32>>,
    recv: RefCell<Set<ErasedSpatial>>,
}

impl SpatialScene {
    /// Create a [`Signal`] for spatializing mono signals for stereo output
    ///
    /// Samples its component signals at `rate`. Sampling more than `buffer_duration` seconds at once
    /// may produce audible glitches when sounds exceed the `max_distance` they're constructed with. If
    /// in doubt, 0.1 is a reasonable guess.
    pub fn new(rate: u32, buffer_duration: f32) -> Self {
        let (handle, set) = set();
        let rot = Swap::new(mint::Quaternion {
            s: 1.0,
            v: [0.0; 3].into(),
        });
        SpatialScene {
            rate,
            buffer_duration,
            send: RefCell::new(handle),
            rot,
            recv: RefCell::new(set),
        }
    }
}

unsafe impl Send for SpatialScene {}

/// Control for modifying a [`SpatialScene`]
pub struct SpatialSceneControl<'a>(&'a SpatialScene);

unsafe impl<'a> Controlled<'a> for SpatialScene {
    type Control = SpatialSceneControl<'a>;

    unsafe fn make_control(signal: &'a SpatialScene) -> Self::Control {
        SpatialSceneControl(signal)
    }
}

impl<'a> SpatialSceneControl<'a> {
    /// Begin playing `signal` at `position`, moving at `velocity`, with accurate propagation delay
    /// out to `max_distance`
    ///
    /// Note that `signal` must be single-channel. Signals in a spatial scene are modeled as
    /// isotropic point sources, and cannot sensibly emit multichannel audio.
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    ///
    /// Returns a [`Handle`] that can be used to adjust the signal's movement in the future, pause
    /// or stop it, and access other controls.
    ///
    /// The type of signal given determines what additional controls can be used. See the
    /// examples for a detailed guide.
    pub fn play<S>(
        &mut self,
        signal: S,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        max_distance: f32,
    ) -> Handle<Spatial<Stop<S>>>
    where
        S: Signal<Frame = Sample> + Send + 'static,
    {
        let signal = Arc::new(Spatial::new(
            self.0.rate,
            Stop::new(signal),
            position,
            velocity,
            max_distance / SPEED_OF_SOUND + self.0.buffer_duration,
        ));
        let handle = unsafe { Handle::from_arc(signal.clone()) };
        self.0.send.borrow_mut().insert(signal);
        handle
    }

    /// Set the listener's rotation
    ///
    /// An unrotated listener faces -Z, with +X to the right and +Y up.
    pub fn set_listener_rotation(&mut self, rotation: mint::Quaternion<f32>) {
        let signal_rotation = invert_quat(&rotation);
        unsafe {
            *self.0.rot.pending() = signal_rotation;
        }
        self.0.rot.flush();
    }
}

impl Signal for SpatialScene {
    type Frame = [Sample; 2];

    fn sample(&self, interval: f32, out: &mut [[Sample; 2]]) {
        let set = &mut *self.recv.borrow_mut();
        // Update set contents
        set.update();

        // Update listener rotation
        let (prev_rot, rot) = unsafe {
            let prev = *self.rot.received();
            self.rot.refresh();
            (prev, *self.rot.received())
        };

        // Zero output in preparation for mixing
        for frame in &mut *out {
            *frame = [0.0; 2];
        }

        let elapsed = interval * out.len() as f32;
        for i in (0..set.len()).rev() {
            let signal = &set[i];
            if Arc::strong_count(signal) == 1 {
                signal.inner.handle_dropped();
            }
            // Discard finished sources
            if signal.remaining() <= 0.0 {
                signal.inner.stop();
            }
            if signal.inner.is_stopped() {
                set.remove(i);
                continue;
            }
            if signal.inner.is_paused() {
                continue;
            }

            debug_assert!(signal.max_delay >= elapsed);

            // Extend delay queue with new data
            signal
                .queue
                .borrow_mut()
                .write(&signal.inner, self.rate, elapsed);

            // Compute the signal's smoothed start/end positions over the sampled period
            // TODO: Use historical positions
            let prev_position;
            let next_position;
            unsafe {
                let mut state = signal.state.borrow_mut();

                // Update motion
                let orig_next = *signal.motion.received();
                if signal.motion.refresh() {
                    state.prev_position = state.smoothed_position(0.0, &orig_next);
                    state.dt = 0.0;
                } else {
                    debug_assert_eq!(orig_next.position, (*signal.motion.received()).position);
                }

                prev_position = rotate(
                    &prev_rot,
                    &state.smoothed_position(0.0, &*signal.motion.received()),
                );
                next_position = rotate(
                    &rot,
                    &state.smoothed_position(elapsed, &*signal.motion.received()),
                );
            }

            // Mix into output
            for &ear in &[Ear::Left, Ear::Right] {
                let prev_state = EarState::new(prev_position, ear);
                let next_state = EarState::new(next_position, ear);

                // Clamp into the max length of the delay queue
                let prev_offset = (prev_state.offset - elapsed).max(-signal.max_delay);
                let next_offset = next_state.offset.max(-signal.max_delay);

                let dt = (next_offset - prev_offset) / out.len() as f32;
                let d_gain = (next_state.gain - prev_state.gain) / out.len() as f32;

                for (i, frame) in out.iter_mut().enumerate() {
                    let gain = prev_state.gain + i as f32 * d_gain;
                    let t = prev_offset + i as f32 * dt;
                    frame[ear as usize] += signal.queue.borrow().sample(self.rate, t) * gain;
                }
            }

            // Set up for next time
            signal.state.borrow_mut().dt += elapsed;
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
