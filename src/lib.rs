//! Lightweight game audio
//!
//! ```no_run
//! let (mut remote, mixer) = oddio::mixer();
//!
//! // In audio callback:
//! # let data = &mut [][..];
//! # let output_sample_rate = 44100;
//! let out_frames = oddio::frame_stereo(data);
//! oddio::run(&mixer, output_sample_rate, out_frames);
//!
//! // In game logic:
//! # let samples = [];
//! # let sample_rate = 44100;
//! # let position = [0.0, 0.0, 0.0].into();
//! # let velocity = [0.0, 0.0, 0.0].into();
//! let samples = oddio::SamplesSource::from(oddio::Samples::from_slice(sample_rate, &samples));
//! let mut handle = remote.play(oddio::Spatial::new(samples, position, velocity));
//!
//! // When position/velocity changes:
//! handle.set_motion(position, velocity);
//! ```

#![warn(missing_docs)]

mod math;
mod mixer;
mod samples;
mod source;
mod spatial;
mod spsc;
mod stream;
pub mod strided;
mod swap;

pub use mixer::*;
pub use samples::*;
pub use source::*;
pub use spatial::Spatial;
pub use stream::{stream, Receiver as StreamReceiver, Sender as StreamSender};
pub use strided::StridedMut;
pub use swap::Swap;

/// Unitless instantaneous sound wave amplitude measurement
pub type Sample = f32;

/// Populate `out` with samples from `source` at `sample_rate`
///
/// Convenience wrapper around the [`Source`] interface.
pub fn run<S: Source>(source: &S, sample_rate: u32, out: &mut [S::Frame]) {
    let sample_len = 1.0 / sample_rate as f32;
    source.sample(0.0, sample_len, out.into());
    source.advance(sample_len * out.len() as f32);
}

/// Convert a slice of interleaved stereo data into a slice of stereo frames
pub fn frame_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}

fn split_stereo<'a>(xs: &'a mut StridedMut<'_, [Sample; 2]>) -> [StridedMut<'a, Sample>; 2] {
    unsafe {
        [
            StridedMut::from_raw_parts(xs.as_ptr().cast(), xs.stride() * 2, xs.len()),
            StridedMut::from_raw_parts(
                xs.as_ptr().cast::<Sample>().add(1),
                xs.stride() * 2,
                xs.len(),
            ),
        ]
    }
}
