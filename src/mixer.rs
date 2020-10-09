use std::ops::{Index, IndexMut};

use crate::{Sample, Source};

/// State of the playback of a single sound for a single listener
#[derive(Debug, Clone)]
pub struct State([EarState; 2]);

impl State {
    pub fn new(position_wrt_listener: mint::Point3<f32>) -> Self {
        Self([
            EarState::new(position_wrt_listener, Ear::Left),
            EarState::new(position_wrt_listener, Ear::Right),
        ])
    }
}

#[derive(Debug, Clone)]
struct EarState {
    /// How far behind current this sound was most recently sampled
    delay: f32,
    /// Attenuation most recently applied
    attenuation: f32,
}

impl EarState {
    fn new(position_wrt_listener: mint::Point3<f32>, ear: Ear) -> Self {
        let distance = norm(sub(position_wrt_listener, ear.pos()));
        let delay = distance * (-1.0 / SPEED_OF_SOUND);
        let distance_attenuation = 1.0 / distance.max(0.1);
        let head_occlusion = if distance == 0.0 {
            0.5
        } else {
            dot(
                ear.dir(),
                scale(position_wrt_listener.into(), 1.0 / distance),
            )
            .max(0.5)
        };
        Self {
            delay,
            attenuation: head_occlusion * distance_attenuation,
        }
    }
}

/// Helper for mixing sounds into a unified scene from a listener's point of view
///
/// Cheap to construct; make fresh ones as needed.
pub struct Mixer<'a> {
    /// Output samples
    pub samples: &'a mut [[Sample; 2]],
    /// Sample rate
    pub rate: u32,
}

impl<'a> Mixer<'a> {
    pub fn new(sample_rate: u32, samples: &'a mut [[Sample; 2]]) -> Self {
        Self {
            samples,
            rate: sample_rate,
        }
    }

    /// Mix in sound from a single input
    pub fn mix(&mut self, mut input: Input<'_, impl Source>) {
        self.mix_mono(&mut input, Ear::Left);
        self.mix_mono(&mut input, Ear::Right);
    }

    fn mix_mono(&mut self, input: &mut Input<'_, impl Source>, ear: Ear) {
        let state = &input.state[ear];
        let next_state = EarState::new(input.position_wrt_listener, ear);
        let t0 = state.delay;
        let dt = self.samples.len() as f32 / self.rate as f32;
        let t1 = next_state.delay + dt;

        let d_samples = (t1 - t0) * input.source.rate() as f32;
        let d_attenuation = next_state.attenuation - state.attenuation;

        let start_sample = t0 * input.source.rate() as f32;
        let step = 1.0 / self.samples.len() as f32;
        for (i, x) in self.samples.iter_mut().enumerate() {
            let t = i as f32 * step;
            x[ear as usize] = input.source.sample(start_sample + t * d_samples)
                * (state.attenuation + t as f32 * d_attenuation);
        }

        input.state[ear] = next_state;
    }
}

/// Characterization of a sound to be mixed for a particular listener
pub struct Input<'a, T> {
    /// The sound source to play
    pub source: &'a T,
    /// The playback state for the listener to mix for
    pub state: &'a mut State,
    /// The position at the end of the output
    pub position_wrt_listener: mint::Point3<f32>,
}

fn norm(x: mint::Vector3<f32>) -> f32 {
    x.as_ref().iter().map(|&x| x.powi(2)).sum::<f32>().sqrt()
}

fn dot(x: mint::Vector3<f32>, y: mint::Vector3<f32>) -> f32 {
    x.as_ref()
        .iter()
        .zip(y.as_ref().iter())
        .map(|(&x, &y)| x * y)
        .sum::<f32>()
}

fn scale(v: mint::Vector3<f32>, f: f32) -> mint::Vector3<f32> {
    [v.x * f, v.y * f, v.z * f].into()
}

fn sub(a: mint::Point3<f32>, b: mint::Point3<f32>) -> mint::Vector3<f32> {
    [a.x - b.x, a.y - b.y, a.z - b.z].into()
}

#[derive(Debug, Copy, Clone)]
enum Ear {
    Left,
    Right,
}

impl Index<Ear> for State {
    type Output = EarState;
    fn index(&self, x: Ear) -> &EarState {
        &self.0[x as usize]
    }
}

impl IndexMut<Ear> for State {
    fn index_mut(&mut self, x: Ear) -> &mut EarState {
        &mut self.0[x as usize]
    }
}

impl Ear {
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

    fn dir(self) -> mint::Vector3<f32> {
        [
            match self {
                Ear::Left => -1.0,
                Ear::Right => 1.0,
            },
            0.0,
            0.0,
        ]
        .into()
    }
}

/// Rate sound travels from sources to listeners (m/s)
const SPEED_OF_SOUND: f32 = 343.0;

/// Distance from center of head to an ear (m)
const HEAD_RADIUS: f32 = 0.1075;
