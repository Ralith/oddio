use std::cell::UnsafeCell;
use std::ops::{Index, IndexMut};

use crate::{
    math::{add, dot, mix, norm, scale, sub},
    swap::Swap,
    Action, Handle, Sample, Seek, Source,
};

/// Places a mono source at an adjustable position and velocity wrt. the listener
///
/// The listener faces directly along the -Z axis, with +X to the right.
pub struct Spatial<T> {
    source: T,
    motion: Swap<Motion>,
    state: UnsafeCell<State>,
}

impl<T> Spatial<T> {
    /// Construct a spatial source with an initial position and velocity
    pub fn new(source: T, position: mint::Point3<f32>, velocity: mint::Vector3<f32>) -> Self {
        Self {
            source,
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
    T: Seek + Source<Frame = Sample>,
{
    type Frame = [Sample; 2];

    fn update(&self) -> Action {
        unsafe {
            let orig_next = *self.motion.received();
            if self.motion.refresh() {
                let state = &mut *self.state.get();
                state.prev_position = state.smoothed_position(&orig_next);
                state.dt = 0.0;
            } else {
                debug_assert_eq!(orig_next.position, (*self.motion.received()).position);
            }
        }
        self.source.update()
    }

    fn sample(&self, sample_duration: f32, count: usize, mut out: impl FnMut(usize, Self::Frame)) {
        unsafe {
            let state = &mut *self.state.get();
            state.dt += count as f32 * sample_duration;
            let next_position = state.smoothed_position(&*self.motion.received());
            sample_helper(
                &self.source,
                state,
                next_position,
                count,
                &mut out,
                sample_duration,
            );
        }
    }
}

fn sample_helper(
    source: &impl Seek<Frame = Sample>,
    state: &mut State,
    next_position: mint::Point3<f32>,
    count: usize,
    out: &mut impl FnMut(usize, [Sample; 2]),
    sample_duration: f32,
) {
    let mut delay0 = [0.0; 2];
    let mut dd = [0.0; 2];
    let mut attenuation0 = [0.0; 2];
    let mut d_attenuation = [0.0; 2];
    let dt_world = count as f32 * sample_duration;
    for &ear in [Ear::Left, Ear::Right].iter() {
        delay0[ear] = state.ears[ear].delay;
        let next_state = EarState::new(next_position, ear);
        let delay_shrink = state.ears[ear].delay - next_state.delay;
        dd[ear] = (dt_world + delay_shrink) / count as f32;
        attenuation0[ear] = state.ears[ear].attenuation;
        d_attenuation[ear] = (next_state.attenuation - state.ears[ear].attenuation) / count as f32;
        state.ears[ear] = next_state;
    }
    for i in 0..count {
        let mut frame = [0.0; 2];
        for &ear in [Ear::Left, Ear::Right].iter() {
            source.sample_at(dd[ear], 1, delay0[ear] - dd[ear] * i as f32, |_, x| {
                frame[ear] = (attenuation0[ear] + d_attenuation[ear] * i as f32) * x;
            });
        }
        out(i, frame);
    }
    source.advance(dt_world);
}

impl<T> Handle<Spatial<T>> {
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
    fn smoothed_position(&self, next: &Motion) -> mint::Point3<f32> {
        let position_change = scale(next.velocity, self.dt);
        let naive_position = add(self.prev_position, position_change);
        let intended_position = add(next.position, position_change);
        mix(
            naive_position,
            intended_position,
            (self.dt / POSITION_SMOOTHING_PERIOD).min(1.0),
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
    /// How far behind current this sound was most recently sampled
    delay: f32,
    /// Attenuation most recently applied
    attenuation: f32,
}

impl EarState {
    fn new(position_wrt_listener: mint::Point3<f32>, ear: Ear) -> Self {
        let distance = norm(sub(position_wrt_listener, ear.pos())).max(0.1);
        let delay = distance * (1.0 / SPEED_OF_SOUND);
        let distance_attenuation = 1.0 / distance;
        let stereo_attenuation = 1.0
            + dot(
                ear.dir(),
                scale(position_wrt_listener.into(), 1.0 / distance),
            );
        Self {
            delay,
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
