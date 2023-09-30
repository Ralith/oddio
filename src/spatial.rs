use alloc::{boxed::Box, sync::Arc};
use core::ops::{Index, IndexMut};

use crate::{
    math::{add, dot, invert_quat, mix, norm, rotate, scale, sub, Float},
    ring::Ring,
    set::{set, Set, SetHandle},
    swap::Swap,
    Sample, Seek, Signal,
};

type ErasedSpatialBuffered = Box<SpatialSignalBuffered<dyn Signal<Frame = Sample> + Send>>;
type ErasedSpatial = Box<SpatialSignal<dyn Seek<Frame = Sample> + Send>>;

/// An individual buffered spatialized signal
struct SpatialSignalBuffered<T: ?Sized> {
    rate: u32,
    max_delay: f32,
    common: Common,
    /// Delay queue of sound propagating through the medium
    ///
    /// Accounts only for the source's velocity. Listener velocity and attenuation are handled at
    /// output time.
    queue: Ring,
    inner: T,
}

impl<T> SpatialSignalBuffered<T> {
    fn new(
        rate: u32,
        inner: T,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        max_delay: f32,
        radius: f32,
    ) -> Self {
        let mut queue = Ring::new((max_delay * rate as f32).ceil() as usize + 1);
        queue.delay(
            rate,
            (norm(position.into()) / SPEED_OF_SOUND).min(max_delay),
        );
        Self {
            rate,
            max_delay,
            common: Common::new(radius, position, velocity),
            queue: queue,
            inner,
        }
    }
}

/// An individual seekable spatialized signal
struct SpatialSignal<T: ?Sized> {
    common: Common,
    inner: T,
}

impl<T> SpatialSignal<T> {
    fn new(
        inner: T,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        radius: f32,
    ) -> Self {
        Self {
            common: Common::new(radius, position, velocity),
            inner,
        }
    }
}

struct Common {
    radius: f32,
    motion: Arc<Swap<Motion>>,
    state: State,
    /// How long ago the signal finished, if it did
    finished_for: Option<f32>,
    stopped: bool,
}

impl Common {
    fn new(radius: f32, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) -> Self {
        Self {
            radius,
            motion: Arc::new(Swap::new(|| Motion {
                position,
                velocity,
                discontinuity: false,
            })),
            state: State::new(position),
            finished_for: None,
            stopped: false,
        }
    }
}

/// Control for updating the motion of a spatial signal
pub struct Spatial(Arc<Swap<Motion>>);

impl Spatial {
    /// Update the position and velocity of the signal
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    ///
    /// Set `discontinuity` when the signal or listener has teleported. This prevents inference of a
    /// very high velocity, with associated intense Doppler effects.
    ///
    /// If your sounds seem to be lagging behind their intended position by about half a second,
    /// make sure you're providing an accurate `velocity`!
    pub fn set_motion(
        &mut self,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
        discontinuity: bool,
    ) {
        unsafe {
            *self.0.pending() = Motion {
                position,
                velocity,
                discontinuity,
            };
        }
        self.0.flush();
    }
}

/// [`Signal`] for stereo output from a spatial scene
pub struct SpatialScene {
    rot: Arc<Swap<mint::Quaternion<f32>>>,
    recv_buffered: Set<ErasedSpatialBuffered>,
    recv: Set<ErasedSpatial>,
}

impl SpatialScene {
    /// Create a [`Signal`] for spatializing mono signals for stereo output
    ///
    /// Samples its component signals at `rate`.
    pub fn new() -> (SpatialSceneControl, Self) {
        let (seek_handle, seek_set) = set();
        let (buffered_handle, buffered_set) = set();
        let rot = Arc::new(Swap::new(|| mint::Quaternion {
            s: 1.0,
            v: [0.0; 3].into(),
        }));
        let control = SpatialSceneControl {
            rot: rot.clone(),
            seek: seek_handle,
            buffered: buffered_handle,
        };
        let signal = SpatialScene {
            rot,
            recv_buffered: buffered_set,
            recv: seek_set,
        };
        (control, signal)
    }
}

fn walk_set<T, U>(
    set: &mut Set<Box<T>>,
    get_common: impl Fn(&mut T) -> &mut Common,
    get_inner: impl Fn(&T) -> &U,
    prev_rot: &mint::Quaternion<f32>,
    rot: &mint::Quaternion<f32>,
    elapsed: f32,
    mut mix_signal: impl FnMut(&mut T, mint::Point3<f32>, mint::Point3<f32>),
) where
    T: ?Sized,
    U: Signal + ?Sized,
{
    set.update();
    for i in (0..set.len()).rev() {
        let signal = &mut set[i];
        let common = get_common(signal);

        let prev_position;
        let next_position;
        unsafe {
            // Compute the signal's smoothed start/end positions over the sampled period
            // TODO: Use historical positions
            let state = &mut common.state;

            // Update motion
            let orig_next = *common.motion.received();
            if common.motion.refresh() {
                state.prev_position = if (*common.motion.received()).discontinuity {
                    (*common.motion.received()).position
                } else {
                    state.smoothed_position(0.0, &orig_next)
                };
                state.dt = 0.0;
            } else {
                debug_assert_eq!(orig_next.position, (*common.motion.received()).position);
            }

            prev_position = rotate(
                prev_rot,
                &state.smoothed_position(0.0, &*common.motion.received()),
            );
            next_position = rotate(
                rot,
                &state.smoothed_position(elapsed, &*common.motion.received()),
            );

            // Set up for next time
            state.dt += elapsed;
        }

        // Discard finished sources. If a source is moving away faster than the speed of sound, you
        // might get a pop.
        let distance = norm(prev_position.into());
        match common.finished_for {
            Some(t) => {
                if t > distance / SPEED_OF_SOUND {
                    common.stopped = true;
                } else {
                    common.finished_for = Some(t + elapsed);
                }
            }
            None => {
                if get_inner(signal).is_finished() {
                    get_common(signal).finished_for = Some(elapsed);
                }
            }
        }
        if get_common(signal).stopped {
            set.remove(i);
            continue;
        }

        mix_signal(signal, prev_position, next_position);
    }
}

/// Control for modifying a [`SpatialScene`]
pub struct SpatialSceneControl {
    rot: Arc<Swap<mint::Quaternion<f32>>>,
    seek: SetHandle<ErasedSpatial>,
    buffered: SetHandle<ErasedSpatialBuffered>,
}

impl SpatialSceneControl {
    /// Begin playing `signal`
    ///
    /// Note that `signal` must be single-channel. Signals in a spatial scene are modeled as
    /// isotropic point sources, and cannot sensibly emit multichannel audio.
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated, with velocity relative to the listener. Units are meters and meters per
    /// second.
    ///
    /// Returns a handle that can be used to adjust the signal's movement in the future, pause or
    /// stop it, and access other controls.
    ///
    /// The type of signal given determines what additional controls can be used. See the
    /// examples for a detailed guide.
    pub fn play<S>(&mut self, signal: S, options: SpatialOptions) -> Spatial
    where
        S: Seek<Frame = Sample> + Send + 'static,
    {
        let signal = Box::new(SpatialSignal::new(
            signal,
            options.position,
            options.velocity,
            options.radius,
        ));
        let handle = Spatial(signal.common.motion.clone());
        self.seek.insert(signal);
        handle
    }

    /// Like [`play`](Self::play), but supports propagation delay for sources which do not implement `Seek` by
    /// buffering.
    ///
    /// `max_distance` dictates the amount of propagation delay to allocate a buffer for; larger
    /// values consume more memory. To avoid glitching, the signal should be inaudible at
    /// `max_distance`. `signal` is sampled at `rate` before resampling based on motion.
    ///
    /// Sampling the scene for more than `buffer_duration` seconds at once may produce audible
    /// glitches when the signal exceeds `max_distance` from the listener. If in doubt, 0.1 is a
    /// reasonable guess.
    pub fn play_buffered<S>(
        &mut self,
        signal: S,
        options: SpatialOptions,
        max_distance: f32,
        rate: u32,
        buffer_duration: f32,
    ) -> Spatial
    where
        S: Signal<Frame = Sample> + Send + 'static,
    {
        let signal = Box::new(SpatialSignalBuffered::new(
            rate,
            signal,
            options.position,
            options.velocity,
            max_distance / SPEED_OF_SOUND + buffer_duration,
            options.radius,
        ));
        let handle = Spatial(signal.common.motion.clone());
        self.buffered.insert(signal);
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

/// Passed to [`SpatialSceneControl::play`]
#[derive(Debug, Copy, Clone)]
pub struct SpatialOptions {
    /// Initial position
    pub position: mint::Point3<f32>,
    /// Initial velocity
    pub velocity: mint::Vector3<f32>,
    /// Distance of zero attenuation. Approaching closer does not increase volume.
    pub radius: f32,
}

impl Default for SpatialOptions {
    fn default() -> Self {
        Self {
            position: [0.0; 3].into(),
            velocity: [0.0; 3].into(),
            radius: 0.1,
        }
    }
}

impl Signal for SpatialScene {
    type Frame = [Sample; 2];

    fn sample(&mut self, interval: f32, out: &mut [[Sample; 2]]) {
        let set = &mut self.recv_buffered;
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

        let mut buf = [0.0; 256];
        let elapsed = interval * out.len() as f32;
        walk_set(
            set,
            |signal| &mut signal.common,
            |signal| &signal.inner,
            &prev_rot,
            &rot,
            elapsed,
            |signal, prev_position, next_position| {
                debug_assert!(signal.max_delay >= elapsed);

                // Extend delay queue with new data
                signal.queue.write(&mut signal.inner, signal.rate, elapsed);

                // Mix into output
                for &ear in &[Ear::Left, Ear::Right] {
                    let prev_state = EarState::new(prev_position, ear, signal.common.radius);
                    let next_state = EarState::new(next_position, ear, signal.common.radius);

                    // Clamp into the max length of the delay queue
                    let prev_offset = (prev_state.offset - elapsed).max(-signal.max_delay);
                    let next_offset = next_state.offset.max(-signal.max_delay);

                    let dt = (next_offset - prev_offset) / out.len() as f32;
                    let d_gain = (next_state.gain - prev_state.gain) / out.len() as f32;

                    let mut i = 0;
                    let queue = &mut signal.queue;
                    for chunk in out.chunks_mut(buf.len()) {
                        let t = prev_offset + i as f32 * dt;
                        queue.sample(signal.rate, t, dt, &mut buf[..chunk.len()]);
                        for (s, o) in buf.iter().copied().zip(chunk) {
                            let gain = prev_state.gain + i as f32 * d_gain;
                            o[ear as usize] += s * gain;
                            i += 1;
                        }
                    }
                }
            },
        );

        let set = &mut self.recv;
        // Update set contents
        set.update();
        walk_set(
            set,
            |signal| &mut signal.common,
            |signal| &signal.inner,
            &prev_rot,
            &rot,
            elapsed,
            |signal, prev_position, next_position| {
                for &ear in &[Ear::Left, Ear::Right] {
                    let prev_state = EarState::new(prev_position, ear, signal.common.radius);
                    let next_state = EarState::new(next_position, ear, signal.common.radius);
                    signal.inner.seek(prev_state.offset); // Initial real time -> Initial delayed

                    let effective_elapsed = (elapsed + next_state.offset) - prev_state.offset;
                    let dt = effective_elapsed / out.len() as f32;
                    let d_gain = (next_state.gain - prev_state.gain) / out.len() as f32;

                    let mut i = 0;
                    for chunk in out.chunks_mut(buf.len()) {
                        signal.inner.sample(dt, &mut buf[..chunk.len()]);
                        for (s, o) in buf.iter().copied().zip(chunk) {
                            let gain = prev_state.gain + i as f32 * d_gain;
                            o[ear as usize] += s * gain;
                            i += 1;
                        }
                    }
                    // Final delayed -> Initial real time
                    signal.inner.seek(-effective_elapsed - prev_state.offset);
                }
                // Initial real time -> Final real time
                signal.inner.seek(elapsed);
            },
        );
    }

    #[inline]
    fn is_finished(&self) -> bool {
        false
    }
}

#[derive(Copy, Clone)]
struct Motion {
    position: mint::Point3<f32>,
    velocity: mint::Vector3<f32>,
    discontinuity: bool,
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
    fn new(position_wrt_listener: mint::Point3<f32>, ear: Ear, radius: f32) -> Self {
        let distance = norm(sub(position_wrt_listener, ear.pos()));
        let offset = distance * (-1.0 / SPEED_OF_SOUND);
        let distance_gain = radius / distance.max(radius);
        // 1.0 when ear faces source directly; 0.5 when perpendicular; 0 when opposite
        let stereo_gain = 0.5
            + if distance < 1e-3 {
                0.5
            } else {
                dot(
                    ear.dir(),
                    scale(position_wrt_listener.into(), 0.5 / distance),
                )
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FinishedSignal;

    impl Signal for FinishedSignal {
        type Frame = f32;

        fn sample(&mut self, _: f32, out: &mut [Self::Frame]) {
            out.fill(0.0);
        }

        fn is_finished(&self) -> bool {
            true
        }
    }

    impl Seek for FinishedSignal {
        fn seek(&mut self, _: f32) {}
    }

    /// Verify that a signal is dropped only after accounting for propagation delay
    #[test]
    fn signal_finished() {
        let (mut control, mut scene) = SpatialScene::new();
        control.play(
            FinishedSignal,
            SpatialOptions {
                // Exactly one second of propagation delay
                position: [SPEED_OF_SOUND, 0.0, 0.0].into(),
                ..SpatialOptions::default()
            },
        );
        scene.sample(0.0, &mut []);
        assert_eq!(
            scene.recv.len(),
            1,
            "signal remains after no time has passed"
        );
        scene.sample(0.6, &mut [[0.0; 2]]);
        assert_eq!(
            scene.recv.len(),
            1,
            "signal remains partway through propagation"
        );
        scene.sample(0.6, &mut [[0.0; 2]]);
        assert_eq!(
            scene.recv.len(),
            1,
            "signal remains immediately after propagation delay expires"
        );
        scene.sample(0.0, &mut []);
        assert_eq!(
            scene.recv.len(),
            0,
            "signal dropped on first past after propagation delay expires"
        );
    }
}
