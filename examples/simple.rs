//! A minimal example of a dynamic real-time mixer; a good place to start.

use std::{thread, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    // Configure cpal
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let sample_rate = device.default_output_config().unwrap().sample_rate();
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    // Create the root mixer, and divide it into two parts: a handle that we can use to add new
    // signals to play, and an object we can pass to `oddio::run` in cpal's callback to generate
    // output frames.
    let (mut mixer_handle, mixer) = oddio::split(oddio::Mixer::new());

    // Start cpal, taking care not to drop its stream early
    let _stream = device
        .build_output_stream(
            &config,
            move |out_flat: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let out_stereo: &mut [[f32; 2]] = oddio::frame_stereo(out_flat);
                oddio::run(&mixer, sample_rate.0, out_stereo);
            },
            move |err| {
                eprintln!("{}", err);
            },
        )
        .unwrap();

    // Start a 200Hz sine wave. We can do this as many times as we like, whenever we like, with
    // different types of signals as needed.
    mixer_handle
        .control::<oddio::Mixer<_>, _>()
        .play(oddio::MonoToStereo::new(oddio::Sine::new(0.0, 200.0)));

    // Wait a bit before exiting
    thread::sleep(Duration::from_secs(3));
}
