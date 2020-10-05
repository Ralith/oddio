use crate::{Sample, Sound};

/// State of the playback of a single sound for a single listener
pub struct State {
    /// Point at which the listener most recently sampled this sound
    t: Option<f64>,
}

impl State {
    pub fn new() -> Self {
        Self { t: None }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper for mixing sounds into a unified scene from a listener's point of view
pub struct Mixer<'a> {
    /// Output samples
    pub samples: &'a mut [Sample],
    /// Sample rate
    pub rate: u32,
    /// Velocity of the listener with respect to the medium
    pub velocity: mint::Vector3<f32>,
}

impl<'a> Mixer<'a> {
    /// Mix in sound from a single input
    pub fn mix(&mut self, input: Input<'_>) {
        let distance = norm(input.position_wrt_listener.into());
        // Ratio to scale playback speed by to produce doppler effect
        let doppler_shift = {
            if distance == 0.0 {
                1.0
            } else {
                let dir = mint::Vector3 {
                    x: input.position_wrt_listener.x / distance,
                    y: input.position_wrt_listener.y / distance,
                    z: input.position_wrt_listener.z / distance,
                };
                let out_speed = dot(dir, self.velocity);
                let src_speed = dot(dir, input.velocity);
                let sign = src_speed.signum();
                (SPEED_OF_SOUND - sign * out_speed) / (SPEED_OF_SOUND + src_speed)
            }
        };
        // Amount of time covered by output
        let dt = self.samples.len() as f64 / f64::from(self.rate);
        // Signed length of interval to play from src
        let src_dt = f64::from(doppler_shift) * dt;
        // Time at src corresponding to the first output sample
        let src_start = input.state.t.unwrap_or_else(|| {
            let delay = distance * (-1.0 / SPEED_OF_SOUND);
            -f64::from(delay)
        });

        // Number of sample steps to advance per output step. May be negative.
        let step_size = src_dt / self.samples.len() as f64;

        for (i, x) in self.samples.iter_mut().enumerate() {
            let t = src_start + i as f64 * step_size;
            *x = input.sound.sample(t * f64::from(input.sound.rate())) / distance;
        }

        input.state.t = Some(src_start + src_dt);
    }
}

/// Characterization of a sound to be mixed for a particular listener
pub struct Input<'a> {
    /// The sound data
    pub sound: &'a Sound,
    /// The playback state for the listener to mix for
    pub state: &'a mut State,
    /// The position of the sound's source relative to the listener
    pub position_wrt_listener: mint::Point3<f32>,
    /// The velocity of the sound's source relative to the medium
    pub velocity: mint::Vector3<f32>,
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

/// Rate sound travels from sources to listeners (m/s)
const SPEED_OF_SOUND: f32 = 343.0;
