use crate::{math::Float, Frame, Signal};

/// Smoothly adjusts gain over time to keep average (RMS) signal level within a target range
///
/// Useful for allowing both quiet and loud sounds to be heard without severe distortion.
///
/// Rapid changes in input amplitude can cause the output to rise above 1. If a hard limit on output
/// is required, a stateless compressor like [`Reinhard`](crate::Reinhard) should be chained
/// afterwards.
///
/// This filter is configured in terms of signal root mean square values. For reference, the RMS
/// value of a sine wave is `amplitude / 2.0f32.sqrt()`. Note that these are linear units, whereas
/// perception of loudness is logarithmic.
pub struct Adapt<T: ?Sized> {
    options: AdaptOptions,
    avg_squared: f32,
    inner: T,
}

impl<T> Adapt<T> {
    /// Apply adaptation to `signal`
    ///
    /// Initialized as if an infinite signal with root mean squared level `initial_rms` had been
    /// processed.
    pub fn new(signal: T, initial_rms: f32, options: AdaptOptions) -> Self {
        Self {
            options,
            avg_squared: initial_rms * initial_rms,
            inner: signal,
        }
    }
}

/// Configuration for an [`Adapt`] filter, passed to [`Adapt::new`]
#[derive(Debug, Copy, Clone)]
pub struct AdaptOptions {
    /// How smoothly the filter should respond. Smaller values reduce time spent outside the target
    /// range, at the cost of lower perceived dynamic range. 0.1 is a good place to start.
    pub tau: f32,
    /// Maximum linear gain to apply regardless of input signal
    pub max_gain: f32,
    /// When the average RMS level is below this, the gain will increase over time, up to at most
    /// `max_gain`
    pub low: f32,
    /// When the average RMS level is above this, the gain will decrease over time
    ///
    /// This should usually be set lower than your desired maximum peak output to avoid clipping of
    /// transient spikes.
    pub high: f32,
}

impl Default for AdaptOptions {
    fn default() -> Self {
        Self {
            tau: 0.1,
            max_gain: f32::INFINITY,
            low: 0.1 / 2.0f32.sqrt(),
            high: 0.5 / 2.0f32.sqrt(),
        }
    }
}

impl<T: Signal> Signal for Adapt<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&mut self, interval: f32, out: &mut [T::Frame]) {
        let alpha = 1.0 - (-interval / self.options.tau).exp();
        self.inner.sample(interval, out);
        for x in out {
            let sample = x.channels().iter().sum::<f32>();
            self.avg_squared = sample * sample * alpha + self.avg_squared * (1.0 - alpha);
            let avg_peak = self.avg_squared.sqrt() * 2.0f32.sqrt();
            let gain = if avg_peak < self.options.low {
                (self.options.low / avg_peak).min(self.options.max_gain)
            } else if avg_peak > self.options.high {
                self.options.high / avg_peak
            } else {
                1.0
            };
            for s in x.channels_mut() {
                *s *= gain;
            }
        }
    }

    fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Constant;

    #[test]
    fn smoke() {
        const LOW: f32 = 0.1;
        const HIGH: f32 = 1.0;
        const MAX_GAIN: f32 = 10.0;
        let mut adapt = Adapt::new(
            Constant::new(0.0),
            0.0,
            AdaptOptions {
                tau: 0.5,
                low: LOW,
                high: HIGH,
                max_gain: MAX_GAIN,
            },
        );

        let mut out = [0.0];
        // Silence isn't modified
        for _ in 0..10 {
            adapt.sample(0.1, &mut out);
            assert_eq!(out[0], 0.0);
        }

        // Suddenly loud!
        adapt.inner.0 = 10.0;
        let mut out = [0.0; 10];
        adapt.sample(0.1, &mut out);
        assert!(out[0] > 0.0 && out[0] < 10.0);
        for w in out.windows(2) {
            assert!(w[0] > w[1]);
        }

        // Back to quiet.
        adapt.inner.0 = 0.01;
        adapt.sample(0.1, &mut out);
        assert!(out[0] > 0.0);
        for w in out.windows(2) {
            assert!(w[0] < w[1]);
        }

        // SUPER quiet.
        adapt.inner.0 = 1e-6;
        for _ in 0..100 {
            adapt.sample(0.1, &mut out);
            for &x in &out {
                assert!(x <= adapt.inner.0 * MAX_GAIN);
            }
        }
    }
}
