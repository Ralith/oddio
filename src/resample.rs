//! Tools for resampling audio

use std::iter;
use std::marker::PhantomData;
use std::sync::Arc;

/// A piecewise polynomial function
pub struct Spline<I: Interpolator> {
    coeffs: Arc<[f32]>,
    _interp: PhantomData<I>,
}

impl<I: Interpolator> Spline<I> {
    /// Construct a spline that fits `samples`
    pub fn new(samples: &[f32]) -> Self {
        assert!(!samples.is_empty());
        let coeff_count = I::ORDER + 1;
        let mut result = iter::repeat(0.0)
            .take((samples.len() - 1) * coeff_count)
            .collect::<Arc<_>>();
        let window_offset = if I::POINTS > 1 { I::POINTS / 2 - 1 } else { 0 };
        let coeffs = Arc::get_mut(&mut result).unwrap();
        // Future work: only copy when window includes data outside the samples
        let mut window = vec![0.0; I::POINTS];
        for i in 0..samples.len() - 1 {
            for (j, x) in window.iter_mut().enumerate() {
                let k = i + j;
                *x = if let Some(l) = k.checked_sub(window_offset) {
                    if l >= samples.len() {
                        0.0
                    } else {
                        samples[l]
                    }
                } else {
                    0.0
                };
            }
            I::compute_coeffs(&window, &mut coeffs[i * coeff_count..(i + 1) * coeff_count]);
        }
        Self {
            coeffs: result,
            _interp: PhantomData,
        }
    }

    /// Sample the spline from t0 to t1 in [0..1]
    pub fn sample(&self, samples: &mut [f32], t0: f32, t1: f32) {
        if samples.is_empty() { return; }
        // FIXME: return zeroes out of range
        let coeff_count = I::ORDER + 1;
        let input_samples = self.coeffs.len() / coeff_count;
        let output_samples = samples.len();
        let ratio = (t1 - t0) * input_samples as f32 / (samples.len() - 1) as f32;
        for (i, x) in samples.iter_mut().enumerate() {
            let t = i as f32 * ratio + t0 * input_samples as f32;
            let (src, t_coeff) = if t.trunc() as usize == input_samples {
                (input_samples - 1, 1.0)
            } else {
                (t.trunc() as usize, t.fract())
            };
            *x = eval(
                &self.coeffs[src * coeff_count..(src + 1) * coeff_count],
                t_coeff,
            );
        }
    }

    /// Number of points in the source data
    pub fn len(&self) -> usize {
        1 + self.coeffs.len() / (I::ORDER + 1)
    }
}

impl<I: Interpolator> Clone for Spline<I> {
    fn clone(&self) -> Self {
        Self {
            coeffs: self.coeffs.clone(),
            _interp: PhantomData,
        }
    }
}

/// A polynomial interpolator of the form `sum(c_n * t^n)`
pub trait Interpolator {
    /// Number of samples required to compute a set of coefficients
    const POINTS: usize;

    /// Number of coefficients minus one
    const ORDER: usize;

    /// Write coefficients fit to `samples` into `coeffs`
    fn compute_coeffs(samples: &[f32], coeffs: &mut [f32]);
}

/// Samples the polynomial `poly` at point `t`
fn eval(poly: &[f32], t: f32) -> f32 {
    poly.iter()
        .enumerate()
        .map(|(i, c)| c * t.powi(i as i32))
        .sum()
}

/// Trivial "interpolation"
pub struct DropSample;

impl Interpolator for DropSample {
    const POINTS: usize = 1;
    const ORDER: usize = 0;

    fn compute_coeffs(samples: &[f32], coeffs: &mut [f32]) {
        coeffs[0] = samples[0];
    }
}

/// Linear interpolation
pub struct Linear;

impl Interpolator for Linear {
    const POINTS: usize = 2;
    const ORDER: usize = 1;

    fn compute_coeffs(samples: &[f32], coeffs: &mut [f32]) {
        coeffs[0] = samples[0];
        coeffs[1] = samples[1] - samples[0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp() {
        let source = [0.0, 1.0, 1.0, 4.0];
        let spline = Spline::<Linear>::new(&source);
        assert_eq!(spline.len(), source.len());
        assert_eq!(&spline.coeffs[..], [0.0, 1.0, 1.0, 0.0, 1.0, 3.0]);

        let mut equal = [0.0; 4];
        spline.sample(&mut equal, 0.0, 1.0);
        assert_eq!(equal, source);
        spline.sample(&mut equal, 1.0, 0.0);
        assert_eq!(equal, [4.0, 1.0, 1.0, 0.0]);

        let mut short = [0.0; 3];
        spline.sample(&mut short, 0.0, 1.0);
        assert_eq!(short, [0.0, 1.0, 4.0]);

        spline.sample(&mut short, 0.0, 0.5);
        assert_eq!(short, [0.0, 0.75, 1.0]);

        spline.sample(&mut short, 0.5, 1.0);
        assert_eq!(short, [1.0, 1.75, 4.0]);

        let mut long = [0.0; 7];
        spline.sample(&mut long, 0.0, 1.0);
        assert_eq!(long, [0.0, 0.5, 1.0, 1.0, 1.0, 2.5, 4.0]);
    }
}
