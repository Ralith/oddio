use std::{
    cell::RefCell,
    ops::{Index, IndexMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{
    frame,
    handle::SignalData,
    math::{add, dot, invert_quat, mix, norm, rotate, scale, sub},
    set::{set, Set, SetHandle},
    swap::Swap,
    Controlled, Filter, Handle, Sample, Signal,
};

/// Create a [`Signal`] for spatializing mono signals for stereo output
///
/// The scene can be controlled through [`SpatialSceneHandle`], and the resulting audio is produced by
/// the [`SpatialScene`] [`Signal`].
pub fn spatial() -> (SpatialSceneHandle, SpatialScene) {
    let (handle, set) = set();
    let rot = Arc::new(Swap::new(mint::Quaternion {
        s: 1.0,
        v: [0.0; 3].into(),
    }));
    (
        SpatialSceneHandle {
            set: handle,
            rot: rot.clone(),
        },
        SpatialScene(RefCell::new(Inner {
            set,
            buffer: vec![0.0f32; 1024 * 2].into(),
            rot,
        })),
    )
}

/// Handle for modifying a spatial scene
pub struct SpatialSceneHandle {
    set: SetHandle<ErasedSpatial>,
    rot: Arc<Swap<mint::Quaternion<f32>>>,
}

impl SpatialSceneHandle {
    /// Begin playing `signal` at `position`, moving at `velocity`
    ///
    /// Coordinates should be in world space, translated such that the listener is at the origin,
    /// but not rotated.
    ///
    /// Returns a [`Handle`] that can be used to adjust the signal's movement in the future, and
    /// access other controls.
    pub fn play<S>(
        &mut self,
        signal: S,
        position: mint::Point3<f32>,
        velocity: mint::Vector3<f32>,
    ) -> Handle<Spatial<S>>
    where
        S: Signal<Frame = Sample> + Send + 'static,
    {
        let signal = Spatial::new(signal, position, velocity);
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
    motion: Swap<Motion>,
    state: RefCell<State>,
    inner: T,
}

impl<T> Spatial<T> {
    fn new(inner: T, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) -> Self {
        Self {
            motion: Swap::new(Motion { position, velocity }),
            state: RefCell::new(State::new(position)),
            inner,
        }
    }
}

impl<T> Spatial<T>
where
    T: ?Sized + Signal<Frame = Sample>,
{
    fn sample(
        &self,
        prev_rot: &mint::Quaternion<f32>,
        rot: &mint::Quaternion<f32>,
        offset: f32,
        world_dt: f32,
        mut out: [&mut [Sample]; 2],
    ) {
        let prev_position;
        let next_position;
        unsafe {
            let mut state = self.state.borrow_mut();

            // Update motion
            let orig_next = *self.motion.received();
            if self.motion.refresh() {
                state.prev_position = state.smoothed_position(0.0, &orig_next);
                state.dt = 0.0;
            } else {
                debug_assert_eq!(orig_next.position, (*self.motion.received()).position);
            }

            prev_position = rotate(
                prev_rot,
                &state.smoothed_position(offset, &*self.motion.received()),
            );
            next_position = rotate(
                rot,
                &state.smoothed_position(
                    offset + world_dt * out[0].len() as f32,
                    &*self.motion.received(),
                ),
            );
        }

        // Compute sampling parameters
        let mut t0 = [0.0; 2];
        let mut dt = [0.0; 2];
        let mut initial_attenuation = [0.0; 2];
        let mut attenuation_change = [0.0; 2];
        let recip_samples = 1.0 / out[0].len() as f32;
        for &ear in [Ear::Left, Ear::Right].iter() {
            let prev_state = EarState::new(prev_position, ear);
            t0[ear] = prev_state.offset + offset;
            let next_state = EarState::new(next_position, ear);
            dt[ear] = (next_state.offset - prev_state.offset) * recip_samples + world_dt;
            initial_attenuation[ear] = prev_state.attenuation;
            attenuation_change[ear] =
                (next_state.attenuation - prev_state.attenuation) * recip_samples;
        }

        // Sample
        for &ear in [Ear::Left, Ear::Right].iter() {
            self.inner.sample(t0[ear], dt[ear], out[ear]);
        }

        // Fix up amplitude
        for &ear in [Ear::Left, Ear::Right].iter() {
            for (t, o) in out[ear].iter_mut().enumerate() {
                *o *= initial_attenuation[ear] + t as f32 * attenuation_change[ear];
            }
        }
    }

    fn advance(&self, dt: f32) {
        let mut state = self.state.borrow_mut();
        state.dt += dt;
        self.inner.advance(dt);
    }

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
    /// but not rotated.
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
    set: Set<ErasedSpatial>,
    buffer: Box<[Sample]>,
    rot: Arc<Swap<mint::Quaternion<f32>>>,
}

impl Signal for SpatialScene {
    type Frame = [Sample; 2];

    fn sample(&self, offset: f32, sample_duration: f32, out: &mut [[Sample; 2]]) {
        let this = &mut *self.0.borrow_mut();
        this.set.update();
        let (mut prev_rot, rot) = unsafe {
            let prev = *this.rot.received();
            this.rot.refresh();
            (prev, *this.rot.received())
        };

        for o in out.iter_mut() {
            *o = [0.0; 2];
        }

        for i in (0..this.set.len()).rev() {
            let data = &this.set[i];
            if data.signal.remaining() < 0.0 {
                data.stop.store(true, Ordering::Relaxed);
            }
            if data.stop.load(Ordering::Relaxed) {
                this.set.remove(i);
                continue;
            }

            // Sample into `buffer`, then mix into `out`
            let mut iter = out.iter_mut();
            let mut i = 0;
            while iter.len() > 0 {
                let n = iter.len().min(this.buffer.len() / 2);
                let (l, r) = this.buffer[..n * 2].split_at_mut(n);
                data.signal.sample(
                    &prev_rot,
                    &rot,
                    offset + i as f32 * sample_duration,
                    sample_duration,
                    [l, r],
                );
                for ((&l, &r), o) in l.iter().zip(r.iter()).zip(&mut iter) {
                    *o = frame::mix(o, &[l, r]);
                }
                i += n;
                prev_rot = rot;
            }
        }
    }

    fn advance(&self, dt: f32) {
        let this = self.0.borrow();
        for data in this.set.iter() {
            data.signal.advance(dt);
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

    fn smoothed_position(&self, offset: f32, next: &Motion) -> mint::Point3<f32> {
        let dt = self.dt + offset;
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
    /// Attenuation most recently applied
    attenuation: f32,
}

impl EarState {
    fn new(position_wrt_listener: mint::Point3<f32>, ear: Ear) -> Self {
        let distance = norm(sub(position_wrt_listener, ear.pos())).max(0.1);
        let offset = distance * (-1.0 / SPEED_OF_SOUND);
        let distance_attenuation = 1.0 / distance;
        let stereo_attenuation = 1.0
            + dot(
                ear.dir(),
                scale(position_wrt_listener.into(), 1.0 / distance),
            );
        Self {
            offset,
            attenuation: stereo_attenuation * distance_attenuation,
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
