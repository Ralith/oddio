//! Lightweight game audio
//!
//! ```no_run
//! let (mut scene_handle, scene) = oddio::split(oddio::SpatialScene::new());
//!
//! // In audio callback:
//! # let data = &mut [][..];
//! # let output_sample_rate = 44100;
//! let out_frames = oddio::frame_stereo(data);
//! oddio::run(&scene, output_sample_rate, out_frames);
//!
//! // In game logic:
//! # let frames = [];
//! # let sample_rate = 44100;
//! # let position = [0.0, 0.0, 0.0].into();
//! # let velocity = [0.0, 0.0, 0.0].into();
//! let frames = oddio::FramesSignal::from(oddio::Frames::from_slice(sample_rate, &frames));
//! let mut handle = scene_handle.control::<oddio::SpatialScene, _>()
//!     .play(frames, oddio::SpatialOptions { position, velocity, ..Default::default() });
//!
//! // When position/velocity changes:
//! handle.control::<oddio::Spatial<_>, _>().set_motion(position, velocity, false);
//! ```
//!
//! To get started, review [the `examples`
//! subdirectory](https://github.com/Ralith/oddio/tree/main/examples) in the crate source.
//!
//! Key primitives:
//! - [`Frames`] stores static audio data, which can be played with a [`FramesSignal`]
//! - [`Mixer`] allows multiple signals to be played concurrently and controlled during playback
//! - [`SpatialScene`] is a mixer that spatializes its signals
//! - [`Handle`] allows control of a signal while it's playing, from a mixer or [`split`]
//! - [`run`] writes frames from a [`Signal`] into an output buffer

#![allow(unused_imports)]
#![warn(missing_docs)]
#![no_std]

extern crate alloc;
#[cfg(not(feature = "no_std"))]
extern crate std;

mod adapt;
mod constant;
mod cycle;
mod downmix;
mod filter;
mod frame;
mod frames;
mod gain;
mod math;
mod mixer;
mod reinhard;
mod ring;
mod set;
mod signal;
mod sine;
mod smooth;
mod spatial;
mod speed;
mod spsc;
mod stop;
mod stream;
mod swap;
mod tanh;

pub use adapt::{Adapt, AdaptOptions};
pub use constant::Constant;
pub use cycle::Cycle;
pub use downmix::Downmix;
pub use filter::*;
pub use frame::Frame;
pub use frames::*;
pub use gain::{FixedGain, Gain, GainControl};
pub use mixer::*;
pub use reinhard::Reinhard;
use set::*;
pub use signal::*;
pub use sine::*;
pub use smooth::{Interpolate, Smoothed};
pub use spatial::*;
pub use speed::{Speed, SpeedControl};
pub use stop::*;
pub use stream::{Stream, StreamControl};
pub use swap::Swap;
pub use tanh::Tanh;

/// Unitless instantaneous sound wave amplitude measurement
pub type Sample = f32;

/// Populate `out` with frames from `signal` at `sample_rate`
///
/// Convenience wrapper for [`Signal::sample`].
pub fn run<S: Signal + ?Sized>(signal: &S, sample_rate: u32, out: &mut [S::Frame]) {
    let interval = 1.0 / sample_rate as f32;
    signal.sample(interval, out);
}

/// Split concurrent controls out of a signal
///
/// The [`Handle`] can be used to control the signal concurrent with the [`SplitSignal`] being
/// played
pub fn split<S: Signal>(signal: S) -> (Handle<S>, SplitSignal<S>) {
    let signal = alloc::sync::Arc::new(signal);
    let handle = unsafe { Handle::from_arc(signal.clone()) };
    (handle, SplitSignal(signal))
}

/// A concurrently controlled [`Signal`]
pub struct SplitSignal<S: ?Sized>(alloc::sync::Arc<S>);

impl<S> Signal for SplitSignal<S>
where
    S: Signal + ?Sized,
{
    type Frame = S::Frame;

    fn sample(&self, interval: f32, out: &mut [Self::Frame]) {
        self.0.sample(interval, out);
    }

    fn remaining(&self) -> f32 {
        self.0.remaining()
    }
}

// Safe due to constraints on [`Controlled`]
unsafe impl<S: ?Sized> Send for SplitSignal<S> {}

/// Convert a slice of interleaved stereo data into a slice of stereo frames
///
/// Useful for adapting output buffers obtained externally.
pub fn frame_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { core::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}

fn flatten_stereo(xs: &mut [[Sample; 2]]) -> &mut [Sample] {
    unsafe { core::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() * 2) }
}
