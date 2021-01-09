use std::{
    thread,
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const DURATION_SECS: u32 = 6;

fn main() {
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
    let boop = oddio::Frames::from_iter(
        sample_rate.0,
        // Generate a simple sine wave
        (0..sample_rate.0 * DURATION_SECS).map(|i| {
            let t = i as f32 / sample_rate.0 as f32;
            (t * 500.0 * 2.0 * std::f32::consts::PI).sin() * 80.0
        }),
    );

    let speed = 50.0;

    let (mut scene_handle, scene) = oddio::spatial();

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = oddio::frame_stereo(data);
                for s in &mut frames[..] {
                    *s = [0.0, 0.0];
                }
                oddio::run(&scene, sample_rate.0, frames);
            },
            move |err| {
                eprintln!("{}", err);
            },
        )
        .unwrap();
    stream.play().unwrap();

    let source = scene_handle.play(
        oddio::FramesSource::from(boop),
        [-speed, 10.0, 0.0].into(),
        [speed, 0.0, 0.0].into(),
    );

    let start = Instant::now();

    loop {
        thread::sleep(Duration::from_millis(50));
        let dt = start.elapsed();
        if dt >= Duration::from_secs(DURATION_SECS as u64) {
            break;
        }
        // This is in principle a no-op because the velocity isn't changing, but due to imprecise
        // sleep times and the fact that the audio thread runs at unaligned intervals means that the
        // this would produce glitches if not for smoothing done by `Spatial`.
        source.control::<oddio::Spatial<_>, _>().set_motion(
            [-speed + speed * dt.as_secs_f32(), 10.0, 0.0].into(),
            [speed, 0.0, 0.0].into(),
        );
    }
}
