use std::cell::UnsafeCell;
use std::ops::{Index, IndexMut};

use crate::{
    math::{add, dot, mix, norm, scale, sub},
    split_stereo,
    swap::Swap,
    Control, Sample, Source, StridedMut,
};

/// Places a mono source at an adjustable position and velocity wrt. the listener
///
/// The listener faces directly along the -Z axis, with +X to the right.
pub struct Spatial<T: ?Sized> {
    motion: Swap<Motion>,
    state: UnsafeCell<State>,
    inner: T,
}

impl<T> Spatial<T> {
    /// Construct a spatial source with an initial position and velocity
    pub fn new(inner: T, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) -> Self {
        Self {
            inner,
            motion: Swap::new(Motion { position, velocity }),
            state: UnsafeCell::new(State {
                ears: [
                    EarState::new(position, Ear::Left),
                    EarState::new(position, Ear::Right),
                ],
                prev_position: position,
                dt: 0.0,
            }),
        }
    }
}

impl<T> Source for Spatial<T>
where
    T: Source<Frame = Sample>,
{
    type Frame = [Sample; 2];

    fn sample(&self, offset: f32, world_dt: f32, mut out: StridedMut<'_, [Sample; 2]>) {
        let state;
        let next_position;
        unsafe {
            // Update motion
            let orig_next = *self.motion.received();
            if self.motion.refresh() {
                let state = &mut *self.state.get();
                state.prev_position = state.smoothed_position(0.0, &orig_next);
                state.dt = 0.0;
            } else {
                debug_assert_eq!(orig_next.position, (*self.motion.received()).position);
            }

            state = &mut *self.state.get();
            next_position =
                state.smoothed_position(world_dt * out.len() as f32, &*self.motion.received());
        }

        // Compute sampling parameters
        let mut t0 = [0.0; 2];
        let mut dt = [0.0; 2];
        let mut initial_attenuation = [0.0; 2];
        let mut attenuation_change = [0.0; 2];
        let recip_samples = 1.0 / out.len() as f32;
        for &ear in [Ear::Left, Ear::Right].iter() {
            t0[ear] = offset + state.ears[ear].offset;
            let next_state = EarState::new(next_position, ear);
            dt[ear] = (next_state.offset - state.ears[ear].offset) * recip_samples + world_dt;
            initial_attenuation[ear] = state.ears[ear].attenuation;
            attenuation_change[ear] =
                (next_state.attenuation - state.ears[ear].attenuation) * recip_samples;
            state.ears[ear] = next_state;
        }

        // Sample
        let mut bufs = split_stereo(&mut out);
        for &ear in [Ear::Left, Ear::Right].iter() {
            self.inner.sample(t0[ear], dt[ear], bufs[ear].borrow());
        }

        // Fix up amplitude
        for &ear in [Ear::Left, Ear::Right].iter() {
            for (t, o) in bufs[ear].iter_mut().enumerate() {
                *o *= initial_attenuation[ear] + t as f32 * attenuation_change[ear];
            }
        }
    }

    fn advance(&self, dt: f32) {
        unsafe {
            (*self.state.get()).dt += dt;
        }
        self.inner.advance(dt);
    }

    fn remaining(&self) -> f32 {
        let position = unsafe {
            let state = &mut *self.state.get();
            state.smoothed_position(0.0, &*self.motion.received())
        };
        let distance = norm(position.into());
        self.inner.remaining() + distance / SPEED_OF_SOUND
    }
}

impl<T> Control<Spatial<T>> {
    /// Update the position and velocity of the source
    pub fn set_motion(&mut self, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) {
        unsafe {
            *(*self.get()).motion.pending() = Motion { position, velocity };
            (*self.get()).motion.flush();
        }
    }
}

#[derive(Copy, Clone)]
struct Motion {
    position: mint::Point3<f32>,
    velocity: mint::Vector3<f32>,
}

struct State {
    ears: [EarState; 2],
    /// Smoothed position estimate when position/vel were updated
    prev_position: mint::Point3<f32>,
    /// Seconds since position/vel were updated
    dt: f32,
}

impl State {
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

/// Rate sound travels from sources to listeners (m/s)
const SPEED_OF_SOUND: f32 = 343.0;

/// Distance from center of head to an ear (m)
const HEAD_RADIUS: f32 = 0.1075;
